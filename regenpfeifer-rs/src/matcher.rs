//! Word pattern matcher — direct transducer.
//!
//! The reference approach is a set-fixpoint: repeatedly apply the left/right substring
//! patterns to a *set* of candidate strings until a fixed point, keeping the dead-end
//! candidates per level, then filter each with the strict `validate` and strip markup.
//! That set-fixpoint dominates runtime.
//!
//! The observation that makes a transducer possible: only the *strict-valid*
//! (fully-bracketed) outputs survive — any output with a leftover lowercase letter is
//! discarded by `stroke::validate` — and that strict-valid subset is order-independent
//! (the fixpoint's order-dependent results are exactly the non-strict-valid strings).
//! So compute only that subset, directly:
//!
//!   1. Vowel-substitute the emphasized word -> `<onset>[e|V]<coda>` (replace-all).
//!   2. ONSET: deterministically left-reduce the letters before the first `[e|`
//!      (cumulative in-place, authored order). If anything but a fully-bracketed
//!      onset remains (leftover vowel/letter), no strict-valid stroke exists -> {}.
//!   3. CODA: explore the right-reduction graph (each edge = one right pattern applied
//!      replace-all to one lowercase run, re-split after, gated by the same lenient
//!      per-part leading-token validation). Collect every reachable state that is
//!      strict-valid (every part bracketed, `*` ignored, key order ok).
//!   4. Output = each strict-valid `<onset-keys>[e|V]<coda-keys>`.
//!
//! This reproduces the generate-and-validate output set exactly, while replacing the
//! whole-word set-fixpoint with a small per-coda graph search.

use crate::patterns::OrderedPatterns;
use crate::stroke;
use rustc_hash::FxHashSet;

/// A single (pattern, replacement) pair kept in authored order, plus its first byte.
struct Pat {
    pat: Box<str>,
    repl: Box<str>,
    first: u8,
}

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

/// Single-pass literal replace-all of `pat` -> `repl` in `s`, writing into `dst`
/// (cleared first). Returns `true` iff at least one replacement happened. Mirrors
/// Python `str.replace` (left-to-right, no re-scan of inserted text).
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

    let mut first_at = None;
    let mut i = 0;
    let limit = bytes.len() - plen;
    while i <= limit {
        if bytes[i] == first && (plen == 1 || bytes[i + 1..i + plen] == pbytes[1..]) {
            first_at = Some(i);
            break;
        }
        i += 1;
    }
    let Some(first_at) = first_at else {
        return false;
    };

    dst.clear();
    dst.push_str(&s[..first_at]);
    dst.push_str(repl);
    let mut i = first_at + plen;
    while i < bytes.len() {
        if bytes[i] == first
            && i + plen <= bytes.len()
            && (plen == 1 || bytes[i + 1..i + plen] == pbytes[1..])
        {
            dst.push_str(repl);
            i += plen;
        } else {
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

const RIGHT_KEYS: &[u8] = b"-FRPBLGTSDZ";

/// Advance `cur` past the first `key` in RIGHT_KEYS at index >= cur. -1 on failure.
#[inline]
fn right_advance(key: u8, cur: i32) -> i32 {
    let mut i = cur.max(0) as usize;
    while i < RIGHT_KEYS.len() {
        if RIGHT_KEYS[i] == key {
            return i as i32 + 1;
        }
        i += 1;
    }
    -1
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
        let mut out = self.transduce(emphasized_word);
        out.sort();
        out
    }

    fn transduce(&self, emphasized_word: &str) -> Vec<String> {
        // 1. vowel substitution (replace-all, authored order). Double-buffer with
        // replace_into to avoid allocating a fresh String per pattern (like reduce_onset).
        let mut word = emphasized_word.to_string();
        let mut buf = String::new();
        for (pat, repl) in &self.vowel {
            if !pat.is_empty() && replace_into(&word, pat, repl, &mut buf) {
                std::mem::swap(&mut word, &mut buf);
            }
        }

        // A `/` is a stroke separator: the original validated/stripped per-segment, so a
        // candidate is valid iff every `/`-segment is a valid stroke. Almost all chunks
        // have no `/`; handle that common case directly, and the multi-segment case via
        // an independent transduce of each segment combined as a cartesian product.
        if !word.contains('/') {
            return self.transduce_segment(&word);
        }
        let mut combos: Vec<String> = vec![String::new()];
        let mut first = true;
        for seg in word.split('/') {
            let seg_outs = self.transduce_segment(seg);
            if seg_outs.is_empty() {
                return Vec::new(); // a segment with no valid stroke kills the whole word
            }
            let mut next = Vec::with_capacity(combos.len() * seg_outs.len());
            let mut seen: FxHashSet<String> = FxHashSet::default();
            for c in &combos {
                for s in &seg_outs {
                    let t = if first {
                        s.clone()
                    } else {
                        let mut t = String::with_capacity(c.len() + 1 + s.len());
                        t.push_str(c);
                        t.push('/');
                        t.push_str(s);
                        t
                    };
                    if seen.insert(t.clone()) {
                        next.push(t);
                    }
                }
            }
            combos = next;
            first = false;
        }
        combos
    }

    /// Transduce a single `/`-free, already-vowel-substituted segment to its strict-valid
    /// strokes. An empty segment yields a single empty stroke (an empty `/`-segment is a
    /// valid empty stroke, e.g. the trailing `/` in `der/`).
    fn transduce_segment(&self, word: &str) -> Vec<String> {
        if word.is_empty() {
            return vec![String::new()];
        }
        // Locate the first `[e|` (the stressed vowel). With NO vowel, the only
        // strict-valid output is the fully-bracketed left-reduced word (e.g. a lone
        // consonant chunk `r` -> `[R]`).
        let Some(vstart) = word.find("[e|") else {
            return match self.reduce_onset(word) {
                Some(k) if !k.is_empty() && stroke::validate(&k) => vec![k],
                _ => Vec::new(),
            };
        };
        let vend = match word[vstart..].find(']') {
            Some(off) => vstart + off + 1,
            None => return Vec::new(),
        };

        // 2. ONSET: left-reduce the prefix before `[e|` deterministically (cumulative
        // in place, per split-part, authored order). If the result still holds any
        // non-bracket character, no strict-valid stroke exists.
        let onset_keys = match self.reduce_onset(&word[..vstart]) {
            Some(k) => k,
            None => return Vec::new(),
        };
        let vowel_tok = &word[vstart..vend]; // e.g. "[e|AEU]"
        let coda = &word[vend..];

        // 3. CODA graph search: collect strict-valid fully-bracketed codas.
        let mut prefix = String::with_capacity(onset_keys.len() + vowel_tok.len() + 8);
        prefix.push_str(&onset_keys);
        prefix.push_str(vowel_tok);

        let mut results: Vec<String> = Vec::new();
        self.explore_coda(coda, &prefix, &mut results);
        results
    }

    /// Deterministically left-reduce the onset letters (everything before `[e|`).
    /// Returns the fully-bracketed onset string, or None if any non-bracket char
    /// remains after reduction (e.g. a leading vowel like the `a` in `abbau`).
    fn reduce_onset(&self, onset: &str) -> Option<String> {
        if onset.is_empty() {
            return Some(String::new());
        }
        // The onset may already contain bracket tokens (e.g. when it begins with a
        // letter the emphasizer left bare). Mirror generate_left_consonants: split
        // into parts, mutate each non-`[e|` part cumulatively in authored order.
        let parts = split_parts(onset);
        let mut reduced: Vec<String> = Vec::with_capacity(parts.len());
        let mut buf = String::new();
        for p in parts {
            // bracket tokens pass through unchanged
            if p.starts_with('[') {
                reduced.push(p.to_string());
                continue;
            }
            let mut cur = p.to_string();
            let mut bits = byte_presence(&cur);
            for pat in &self.left.pats {
                if !has_byte(&bits, pat.first) {
                    continue;
                }
                if replace_into(&cur, &pat.pat, &pat.repl, &mut buf) {
                    std::mem::swap(&mut cur, &mut buf);
                    bits = byte_presence(&cur);
                }
            }
            reduced.push(cur);
        }
        let joined: String = reduced.concat();
        // strict: every char must be inside a bracket token now.
        if all_bracketed(&joined) {
            Some(joined)
        } else {
            None
        }
    }

    /// Explore the coda right-reduction graph from `coda`, pushing each strict-valid
    /// fully-bracketed completion `<prefix><coda-keys>` into `results`.
    fn explore_coda(&self, coda: &str, prefix: &str, results: &mut Vec<String>) {
        if coda.is_empty() {
            // empty coda is itself strict-valid (no right keys)
            results.push(prefix.to_string());
            return;
        }
        SCRATCH.with(|s| {
            let mut sc = s.borrow_mut();
            sc.run(self, coda, prefix, results);
        });
    }
}

/// Per-thread reusable scratch for the coda DFS, so the 1.8M+ `explore_coda` calls don't
/// each allocate fresh collections.
struct CodaScratch {
    seen: FxHashSet<String>,
    out_seen: FxHashSet<String>,
    stack: Vec<String>,
    parts: Vec<(usize, usize)>,
    rbuf: String,
    child: String,
}

thread_local! {
    static SCRATCH: std::cell::RefCell<CodaScratch> = std::cell::RefCell::new(CodaScratch {
        seen: FxHashSet::default(),
        out_seen: FxHashSet::default(),
        stack: Vec::new(),
        parts: Vec::new(),
        rbuf: String::new(),
        child: String::new(),
    });
}

impl CodaScratch {
    fn run(&mut self, m: &Matcher, coda: &str, prefix: &str, results: &mut Vec<String>) {
        self.seen.clear();
        self.out_seen.clear();
        self.stack.clear();
        self.seen.insert(coda.to_string());
        self.stack.push(coda.to_string());

        while let Some(state) = self.stack.pop() {
            // strict-valid? then record completion.
            if let Some(valid) = coda_strict_valid(&state) {
                if valid {
                    let mut full = String::with_capacity(prefix.len() + state.len());
                    full.push_str(prefix);
                    full.push_str(&state);
                    if self.out_seen.insert(full.clone()) {
                        results.push(full);
                    }
                    // Fully bracketed coda: no lowercase runs left, no children.
                    continue;
                }
            }
            // generate children: for each lowercase run, each applicable right pattern.
            split_runs(&state, &mut self.parts);
            for k in 0..self.parts.len() {
                let (rs, re) = self.parts[k];
                let run = &state[rs..re];
                if run.as_bytes()[0] == b'[' {
                    continue; // bracket token, not a reducible run
                }
                let bits = byte_presence(run);
                for pat in &m.right.pats {
                    if !has_byte(&bits, pat.first) {
                        continue;
                    }
                    if replace_into(run, &pat.pat, &pat.repl, &mut self.rbuf) {
                        // candidate child = state with [rs..re] replaced by rbuf
                        self.child.clear();
                        self.child.push_str(&state[..rs]);
                        self.child.push_str(&self.rbuf);
                        self.child.push_str(&state[re..]);
                        // Lenient gate: reject states that can't reach valid right order.
                        if !coda_lenient_valid(&self.child) {
                            continue;
                        }
                        // Single clone: insert into `seen`; only enqueue if newly seen.
                        if self.seen.insert(self.child.clone()) {
                            self.stack.push(self.child.clone());
                        }
                    }
                }
            }
        }
    }
}

/// Split a string into (start,end) byte ranges of parts: each `[...]` bracket token is
/// one part; maximal lowercase/`*` runs between brackets are one part each. Mirrors
/// `split_into` boundaries. Ranges are written into `out` (cleared first).
fn split_runs(s: &str, out: &mut Vec<(usize, usize)>) {
    out.clear();
    let bytes = s.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        if bytes[i] == b'[' {
            // bracket token up to and including ']'
            let start = i;
            i += 1;
            while i < n && bytes[i] != b']' {
                i += 1;
            }
            if i < n {
                i += 1; // include ']'
            }
            out.push((start, i));
        } else {
            // lowercase/`*` run up to next '['
            let start = i;
            while i < n && bytes[i] != b'[' {
                i += 1;
            }
            out.push((start, i));
        }
    }
}

/// Owned-string split into parts (for onset, infrequent path).
fn split_parts(s: &str) -> Vec<&str> {
    let mut ranges = Vec::new();
    split_runs(s, &mut ranges);
    ranges.into_iter().map(|(a, b)| &s[a..b]).collect()
}

/// True iff every character of `s` lies inside a `[...]` bracket token (no bare chars
/// between/around brackets). Empty string -> true.
fn all_bracketed(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        if bytes[i] != b'[' {
            return false;
        }
        while i < n && bytes[i] != b']' {
            i += 1;
        }
        if i >= n {
            return false; // unterminated
        }
        i += 1; // skip ']'
    }
    true
}

/// Strict validation of a CODA string `<keys-and-runs>` that follows the vowel: returns
/// `Some(true)` if it is fully bracketed AND the right-bank key order is valid (with all
/// `*` ignored), `Some(false)` if fully bracketed but order-invalid, `None` if it still
/// contains a lowercase run (not yet a candidate). The vowel/onset are not present here;
/// the coda only contributes right-bank keys, so left/vowel cursors are irrelevant.
#[inline]
fn coda_strict_valid(coda: &str) -> Option<bool> {
    // Must be all bracket tokens (ignoring that `*` may sit outside, e.g. `[-FP]*`).
    // Mirror `validate`: strip `*` first, then require every part bracketed + order ok.
    let bytes = coda.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    let mut cur: i32 = 0;
    let mut hyphen_seen = false;
    while i < n {
        match bytes[i] {
            b'*' => {
                i += 1; // stripped
            }
            b'[' => {
                // token `[-KEYS]` (codas are right-bank, always start with '-')
                let start = i;
                while i < n && bytes[i] != b']' {
                    i += 1;
                }
                if i >= n {
                    return None;
                }
                let tok = &coda[start..=i];
                i += 1;
                // tok looks like "[-FPL]" — strip '[' and ']'
                let inner = &tok[1..tok.len() - 1];
                let ib = inner.as_bytes();
                if ib.is_empty() || ib[0] != b'-' {
                    // a coda token not starting with '-' cannot be right-bank-valid
                    // (the fixpoint only ever emits `[-...]` on the right). Treat as
                    // order-invalid.
                    return Some(false);
                }
                if !hyphen_seen {
                    cur = right_advance(b'-', cur);
                    if cur < 0 {
                        return Some(false);
                    }
                    hyphen_seen = true;
                }
                let mut j = 1;
                while j < ib.len() {
                    let c = ib[j];
                    if c == b'*' {
                        j += 1;
                        continue;
                    }
                    cur = right_advance(c, cur);
                    if cur < 0 {
                        return Some(false);
                    }
                    j += 1;
                }
            }
            _ => {
                // a lowercase letter remains -> not strict-valid yet
                return None;
            }
        }
    }
    Some(true)
}

/// Lenient validation used to gate right-pass children, mirroring the fixpoint's
/// `validate_parts_stripped` restricted to the coda: per part, only the LEADING bracket
/// token contributes to the right-bank order check; everything else (lowercase runs,
/// trailing brackets after the first `]` in a run) is ignored. `*` ignored.
#[inline]
fn coda_lenient_valid(coda: &str) -> bool {
    let bytes = coda.as_bytes();
    let mut i = 0;
    let n = bytes.len();
    let mut cur: i32 = 0;
    let mut hyphen_seen = false;
    while i < n {
        if bytes[i] == b'[' {
            // leading token of this part
            let start = i;
            while i < n && bytes[i] != b']' {
                i += 1;
            }
            let end = if i < n { i + 1 } else { i };
            // advance i to the end of this whole part (skip trailing chars until next '[')
            let mut k = end;
            while k < n && bytes[k] != b'[' {
                k += 1;
            }
            i = k;
            let tok = &coda[start..end];
            // tok = "[-KEYS]" or possibly unterminated; only the leading bracket counts
            let inner = if tok.ends_with(']') {
                &tok[1..tok.len() - 1]
            } else {
                &tok[1..]
            };
            let ib = inner.as_bytes();
            let mut j = 0;
            if !ib.is_empty() && ib[0] == b'-' {
                if !hyphen_seen {
                    cur = right_advance(b'-', cur);
                    if cur < 0 {
                        return false;
                    }
                    hyphen_seen = true;
                }
                j = 1;
            }
            while j < ib.len() {
                let c = ib[j];
                if c != b'*' {
                    cur = right_advance(c, cur);
                    if cur < 0 {
                        return false;
                    }
                }
                j += 1;
            }
        } else {
            // lowercase/`*` run: skip to next '[' (ignored by lenient validation)
            while i < n && bytes[i] != b'[' {
                i += 1;
            }
        }
    }
    true
}
