//! Word pattern matcher — faithful port of word_pattern_matcher.py, using plain
//! substring replacement (the patterns are literal substrings; `re.sub` replaces
//! all occurrences).
//!
//! Performance notes: the original used `BTreeSet<String>` everywhere and re-split
//! / re-joined / cloned `Vec<String>` per candidate, scanning all ~28 left + ~73
//! right patterns via `.contains()` on every candidate. This rewrite:
//!   * uses `FxHashSet<String>` for the working/candidate sets (sort only the final
//!     output `Vec`, since intermediate order never affects the final set),
//!   * indexes patterns by their first byte so a candidate part only tests the
//!     handful of patterns whose first byte is actually present in the part, and
//!   * reuses scratch buffers (split parts + join target) across the fixpoint.
//! The left-consonant pass applies patterns in the authored (longest-first) order
//! because it mutates the part in place cumulatively, so order is semantically
//! load-bearing; the right pass does not mutate and copies per success.

use crate::patterns::OrderedPatterns;
use crate::stroke;
use rustc_hash::FxHashSet;

/// A single (pattern, replacement) pair kept in authored order, plus the byte that
/// its key starts with (so a part-level byte-presence bitset can cheaply skip it).
struct Pat {
    pat: Box<str>,
    repl: Box<str>,
    first: u8,
}

/// Patterns in authored order, alongside a 256-bit presence set of first bytes so a
/// part that contains none of a pattern's first byte is skipped without a substring
/// scan. (We must iterate in authored order — the left pass mutates in place — so we
/// keep a single ordered list rather than per-byte buckets.)
struct PatternIndex {
    pats: Vec<Pat>,
}

impl PatternIndex {
    fn new(pats: &OrderedPatterns) -> Self {
        let pats = pats
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(k, v)| Pat {
                pat: k.as_str().into(),
                repl: v.as_str().into(),
                first: k.as_bytes()[0],
            })
            .collect();
        PatternIndex { pats }
    }
}

/// Build a 256-bit set (as 4 u64 words) of the bytes present in `s`.
#[inline]
fn byte_presence(s: &str) -> [u64; 4] {
    let mut bits = [0u64; 4];
    for &b in s.as_bytes() {
        bits[(b >> 6) as usize] |= 1u64 << (b & 63);
    }
    bits
}

#[inline]
fn has_byte(bits: &[u64; 4], b: u8) -> bool {
    bits[(b >> 6) as usize] & (1u64 << (b & 63)) != 0
}

pub struct Matcher {
    vowel: OrderedPatterns,
    left: PatternIndex,
    right: PatternIndex,
}

/// Single-pass literal replace-all of `pat` -> `repl` in `s`, writing the result
/// into `dst` (cleared first). Returns `true` iff at least one replacement happened.
///
/// Mirrors `s.replace(pat, repl)` followed by the original `replace_if_changed`'s
/// char-count guard. For every pattern in these data files a real replacement always
/// changes the char count (verified), so "a replacement happened" is exactly the
/// original's `Some(..)` condition. Scans left-to-right and does not re-scan inserted
/// replacements (matching Python `str.replace` / `re.sub`), like the original.
#[inline]
fn replace_into(s: &str, pat: &str, repl: &str, dst: &mut String) -> bool {
    debug_assert!(!pat.is_empty());
    let bytes = s.as_bytes();
    let pbytes = pat.as_bytes();
    let plen = pbytes.len();
    if plen > bytes.len() {
        return false;
    }
    let first = pbytes[0];

    // Fast reject pass: hand-rolled first-byte scan + tail compare. Patterns here are
    // 1-4 bytes, so this beats `str::find`'s TwoWaySearcher construction (which showed
    // up hot in profiles). Find the first match position without touching `dst`.
    let mut first_at = None;
    let mut i = 0;
    let limit = bytes.len() - plen;
    while i <= limit {
        if bytes[i] == first && (plen == 1 || &bytes[i + 1..i + plen] == &pbytes[1..]) {
            first_at = Some(i);
            break;
        }
        i += 1;
    }
    let Some(first_at) = first_at else {
        return false;
    };

    // A match exists; now build the result.
    dst.clear();
    dst.push_str(&s[..first_at]);
    dst.push_str(repl);
    let mut i = first_at + plen;
    while i < bytes.len() {
        if bytes[i] == first
            && i + plen <= bytes.len()
            && (plen == 1 || &bytes[i + 1..i + plen] == &pbytes[1..])
        {
            dst.push_str(repl);
            i += plen;
        } else {
            // copy one full UTF-8 char (we only ever land on char boundaries)
            let ch_len = utf8_len(bytes[i]);
            dst.push_str(&s[i..i + ch_len]);
            i += ch_len;
        }
    }
    true
}

#[inline]
fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

fn replace_all(s: &str, pat: &str, repl: &str) -> String {
    if pat.is_empty() {
        return s.to_string();
    }
    s.replace(pat, repl)
}

/// Join `parts` into `scratch`, but with `parts[idx]` replaced by `override_part`.
#[inline]
fn join_with_override(parts: &[String], idx: usize, override_part: &str, scratch: &mut String) {
    scratch.clear();
    for (k, p) in parts.iter().enumerate() {
        if k == idx {
            scratch.push_str(override_part);
        } else {
            scratch.push_str(p);
        }
    }
}

/// Concatenate all `parts` into `scratch`.
#[inline]
fn join_parts(parts: &[String], scratch: &mut String) {
    scratch.clear();
    for p in parts {
        scratch.push_str(p);
    }
}

/// If the candidate currently held in `scratch` isn't already in `sink`, insert it.
#[inline]
fn insert_candidate(scratch: &str, sink: &mut FxHashSet<String>) {
    if !sink.contains(scratch) {
        sink.insert(scratch.to_string());
    }
}

impl Matcher {
    pub fn new(vowel: OrderedPatterns, left: OrderedPatterns, right: OrderedPatterns) -> Self {
        Matcher {
            vowel,
            left: PatternIndex::new(&left),
            right: PatternIndex::new(&right),
        }
    }

    pub fn match_word(&self, emphasized_word: &str) -> Vec<String> {
        // 1. vowel substitution (each applied in order, replace-all)
        let mut word = emphasized_word.to_string();
        for (pat, repl) in &self.vowel {
            word = replace_all(&word, pat, repl);
        }

        let mut matches: FxHashSet<String> = FxHashSet::default();
        matches.insert(word);

        let mut final_matches: FxHashSet<String> = FxHashSet::default();

        // Scratch buffers reused across the whole fixpoint.
        let mut parts: Vec<String> = Vec::new();
        let mut scratch = String::new();
        let mut rbuf = String::new();

        loop {
            let mut new_matches: FxHashSet<String> = FxHashSet::default();
            for m in &matches {
                let produced = self.generate_matches(
                    m,
                    &mut new_matches,
                    &mut parts,
                    &mut scratch,
                    &mut rbuf,
                );
                if !produced {
                    final_matches.insert(m.clone());
                }
            }
            if !new_matches.is_empty() {
                matches = new_matches;
            } else {
                break;
            }
        }

        let mut out: Vec<String> = final_matches.into_iter().collect();
        out.sort();
        out
    }

    /// Generate (and validate) all candidates derivable from `m` by one left- or
    /// right-consonant substitution, inserting them into `sink`. Returns `true` if
    /// at least one candidate was produced (so the caller knows `m` is not final).
    fn generate_matches(
        &self,
        m: &str,
        sink: &mut FxHashSet<String>,
        parts: &mut Vec<String>,
        scratch: &mut String,
        rbuf: &mut String,
    ) -> bool {
        // `split_into` reuses the existing String slots in `parts` (and truncates),
        // so we deliberately do NOT clear it — that lets allocations be recycled
        // across candidates.
        stroke::split_into(m, parts);

        let before = sink.len();
        self.generate_left_consonants(parts, scratch, rbuf, sink);
        self.generate_right_consonants(parts, scratch, rbuf, sink);
        sink.len() != before
    }

    /// Mirrors the Python in-place cumulative mutation of word_parts: each part is
    /// mutated in place as successive (authored-order) patterns apply, and every
    /// successful single replacement emits a validated snapshot of the whole join.
    fn generate_left_consonants(
        &self,
        parts: &mut [String],
        scratch: &mut String,
        rbuf: &mut String,
        sink: &mut FxHashSet<String>,
    ) {
        for i in 0..parts.len() {
            if parts[i].starts_with("[e|") {
                break;
            }
            let mut bits = byte_presence(&parts[i]);
            for p in &self.left.pats {
                // Skip patterns whose first byte isn't present in the (current) part.
                if !has_byte(&bits, p.first) {
                    continue;
                }
                if replace_into(&parts[i], &p.pat, &p.repl, rbuf) {
                    std::mem::swap(&mut parts[i], rbuf);
                    // Part changed; recompute its byte presence for later patterns.
                    bits = byte_presence(&parts[i]);
                    // Validate over the (already-mutated) parts directly, BEFORE
                    // materializing the joined string — only allocate the candidate
                    // string when it actually passes validation.
                    if stroke::validate_parts_stripped(parts, usize::MAX, None) {
                        join_parts(parts, scratch);
                        insert_candidate(scratch, sink);
                    }
                }
            }
        }
    }

    /// Right side does NOT mutate in place (copies per success).
    fn generate_right_consonants(
        &self,
        parts: &[String],
        scratch: &mut String,
        rbuf: &mut String,
        sink: &mut FxHashSet<String>,
    ) {
        let mut after_vowel = false;
        for i in 0..parts.len() {
            if after_vowel {
                let bits = byte_presence(&parts[i]);
                for p in &self.right.pats {
                    if !has_byte(&bits, p.first) {
                        continue;
                    }
                    if replace_into(&parts[i], &p.pat, &p.repl, rbuf) {
                        // Validate the override against `parts` directly, then build
                        // the joined string only on success.
                        if stroke::validate_parts_stripped(parts, i, Some(rbuf)) {
                            join_with_override(parts, i, rbuf, scratch);
                            insert_candidate(scratch, sink);
                        }
                    }
                }
            } else if parts[i].starts_with("[e|") {
                after_vowel = true;
            }
        }
    }
}
