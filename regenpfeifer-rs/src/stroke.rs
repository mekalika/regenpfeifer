//! Port of util/stroke_util.py + stroke_validator.py.
//! Strokes are strings mixing bracketed key-parts like `[S]`, `[e|A]`, `[-R]`
//! with leftover lowercase letters. A "stroke part" is either a bracketed token
//! `[...]` or (in split) lowercase runs are split per-character-run between
//! brackets.

/// Like `split` (each `[...]` is one part; outside-bracket runs accumulate into one
/// part, matching the Python char loop), but writes parts into `out`, reusing the
/// `String` slots already in
/// `out` (and its outer `Vec` capacity) across calls to avoid per-part allocation.
/// `out` is logically reset (length set to the number of parts found); any extra
/// pre-existing slots are dropped.
pub fn split_into(stroke: &str, out: &mut Vec<String>) {
    let mut n = 0usize; // number of parts written so far

    // Helper closure-free push: reuse out[n] if present, else push a fresh String.
    macro_rules! begin_part {
        ($first:expr) => {{
            if n < out.len() {
                out[n].clear();
                out[n].push($first);
            } else {
                let mut s = String::new();
                s.push($first);
                out.push(s);
            }
        }};
    }

    let mut in_part = false;
    for ch in stroke.chars() {
        if ch == '[' {
            if in_part {
                n += 1;
            }
            begin_part!('[');
            in_part = true;
        } else if ch == ']' {
            if in_part {
                out[n].push(']');
                n += 1;
                in_part = false;
            }
            // stray ']' on empty part: dropped (no current part)
        } else {
            if !in_part {
                begin_part!(ch);
                in_part = true;
            } else {
                out[n].push(ch);
            }
        }
    }
    if in_part {
        n += 1;
    }
    out.truncate(n);
}

/// Zero-allocation variant of `split`: invokes `f` with each part as a `&str`
/// slice of `stroke`. Every part is a contiguous substring (the only divergence
/// from `split` — a stray `]` on an empty part — produces no part, so no slice
/// gap arises). Faithful to the Python char loop for well-formed strokes.
pub fn split_each<F: FnMut(&str)>(stroke: &str, mut f: F) {
    let bytes = stroke.as_bytes();
    let mut start: Option<usize> = None; // start byte of current part
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'[' {
            if let Some(s) = start {
                f(&stroke[s..i]);
            }
            start = Some(i);
        } else if c == b']' {
            if let Some(s) = start {
                f(&stroke[s..=i]);
                start = None;
            }
            // stray ']' on empty part: dropped
        } else {
            if start.is_none() {
                start = Some(i);
            }
        }
        i += 1;
    }
    if let Some(s) = start {
        f(&stroke[s..]);
    }
}

pub fn remove_markup(strokes: &str) -> String {
    let mut out = String::with_capacity(strokes.len());
    remove_markup_into(strokes, &mut out);
    out
}

/// Fused `remove_excess_hyphens` + bracket/marker stripping, written directly into
/// `out` (cleared first). Equivalent to the original per-segment
/// `remove_excess_hyphens(seg)` followed by `.replace("[e|","").replace('[',"")
/// .replace(']',"")`, but in a single allocation-free pass over each segment.
pub fn remove_markup_into(strokes: &str, out: &mut String) {
    out.clear();
    let mut first_segment = true;
    for stroke in strokes.split('/') {
        if !first_segment {
            out.push('/');
        }
        first_segment = false;

        // Path A: segments containing `[e|` or `[*]` simply drop all '-' (the
        // original `remove_excess_hyphens` early return), then strip markers.
        if stroke.contains("[e|") || stroke.contains("[*]") {
            // Strip markers `[e|`, `[`, `]` and drop ALL '-' (original early-return
            // did `stroke.replace('-', "")` over the whole segment): keep only inner
            // key letters (and '*' from `[*]`), with every hyphen removed.
            split_each(stroke, |part| {
                push_keys_strip_all_hyphens(part, out);
            });
        } else {
            // Path B: keep the FIRST '[-' hyphen, drop subsequent ones, then strip
            // brackets. After stripping `[`/`]`, a leading-hyphen part contributes
            // its inner letters; only the first such part keeps a '-' prefix.
            let mut first_hyphen_seen = false;
            split_each(stroke, |part| {
                if part.starts_with("[-") {
                    if !first_hyphen_seen {
                        first_hyphen_seen = true;
                        out.push('-');
                    }
                    // inner letters after "[-" up to "]"
                    let inner = &part[2..part.len() - 1];
                    out.push_str(inner);
                } else {
                    push_keys_no_hyphen(part, out);
                }
            });
        }
    }
}

/// Push a part's contents with markers `[e|`, `[`, `]` removed and a single leading
/// `-` (right-bank marker) dropped. Non-bracketed runs are copied verbatim (the
/// original `.replace` only strips bracket/marker chars in Path B).
#[inline]
fn push_keys_no_hyphen(part: &str, out: &mut String) {
    if let Some(inner) = part.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        // strip a leading "e|" or "-" marker, keep the rest verbatim (incl '*')
        let inner = inner.strip_prefix("e|").unwrap_or(inner);
        let inner = inner.strip_prefix('-').unwrap_or(inner);
        out.push_str(inner);
    } else {
        out.push_str(part);
    }
}

/// Like `push_keys_no_hyphen`, but drops EVERY '-' (Path A, where the original did a
/// whole-segment `replace('-', "")`), including hyphens in non-bracketed runs.
#[inline]
fn push_keys_strip_all_hyphens(part: &str, out: &mut String) {
    let body = if let Some(inner) = part.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        inner.strip_prefix("e|").unwrap_or(inner)
    } else {
        part
    };
    for c in body.chars() {
        if c != '-' {
            out.push(c);
        }
    }
}

// --- asterisk repositioning ---

const BEFORE_ASTERISK: &str = "ZSTKPWHRAO";

pub fn reposition_asterisks(stripped_strokes: &str) -> String {
    let mut fixed = Vec::new();
    for stroke in stripped_strokes.split('/') {
        if stroke.contains('*') {
            if stroke.contains('-') {
                fixed.push(stroke.replace('-', "*"));
                // Python `break`s out of the loop here, dropping any remaining
                // strokes from the output.
                break;
            }
            let stroke_nostar = stroke.replace('*', "");
            let chars: Vec<char> = stroke_nostar.chars().collect();
            let mut index_for_asterisk: Option<usize> = None;
            let mut current_before: String = BEFORE_ASTERISK.to_string();
            for (i, &key) in chars.iter().enumerate() {
                if current_before.contains(key) {
                    current_before = get_all_after_letter(&current_before, key);
                    continue;
                }
                index_for_asterisk = Some(i);
                break;
            }
            fixed.push(insert_asterisk(&stroke_nostar, index_for_asterisk));
        } else {
            fixed.push(stroke.to_string());
        }
    }
    fixed.join("/")
}

fn get_all_after_letter(letters: &str, letter: char) -> String {
    let mut out = String::new();
    let mut reached = false;
    for c in letters.chars() {
        if c == letter {
            reached = true;
            continue;
        }
        if reached {
            out.push(c);
        }
    }
    out
}

fn insert_asterisk(stroke: &str, index: Option<usize>) -> String {
    let chars: Vec<char> = stroke.chars().collect();
    match index {
        Some(idx) => {
            let mut out = String::new();
            out.extend(&chars[..idx]);
            out.push('*');
            out.extend(&chars[idx..]);
            out
        }
        // Python: stroke[:None] + "*" + stroke[None:] == stroke + "*" + stroke ??
        // Actually stroke[:None] is the whole string and stroke[None:] is whole
        // string too -> "stroke*stroke". But this only happens if no key was
        // outside before_asterisk, i.e. all keys precede '*'; then index stays
        // None and Python does stroke[:None]+"*"+stroke[None:] = stroke+"*"+stroke.
        // Reproduce faithfully.
        None => format!("{stroke}*{stroke}"),
    }
}

// ---------------------------------------------------------------------------
// Validator (stroke_validator.py)
// ---------------------------------------------------------------------------

const LEFT_KEYS: &[char] = &['Z', 'S', 'T', 'K', 'P', 'W', 'H', 'R'];
const VOWEL_KEYS: &[char] = &['A', 'O', '*', 'E', 'U'];
const RIGHT_KEYS: &[char] = &['-', 'F', 'R', 'P', 'B', 'L', 'G', 'T', 'S', 'D', 'Z'];

/// Validate a candidate held as a list of already-split `parts` (no '/' present —
/// within `match_word` a candidate is a single stroke), as if
/// `validate(strip_unmatched_letters(join(parts)))` were applied. `override_idx`,
/// when set, substitutes `override_val` for `parts[idx]` without materializing the
/// joined string — letting the matcher validate BEFORE allocating the result string.
pub fn validate_parts_stripped(
    parts: &[String],
    override_idx: usize,
    override_val: Option<&str>,
) -> bool {
    let mut st = SegState::new();
    for (k, p) in parts.iter().enumerate() {
        let part: &str = if override_idx == k {
            match override_val {
                Some(v) => v,
                None => p.as_str(),
            }
        } else {
            p.as_str()
        };
        // `split_into` guarantees each element starts a new part at a `[`, so an
        // element either (a) is a pure lowercase run — stripped, ignore — or (b)
        // begins with exactly one bracket token `[...]` optionally followed by
        // leftover lowercase (also stripped). So we only ever need to validate the
        // single leading `[...]` token, if present.
        if !part.starts_with('[') {
            continue; // lowercase run -> stripped
        }
        // Find the matching ']' (brackets don't nest in this data); the token is the
        // inclusive `[...]` slice. Anything after it is leftover lowercase (stripped).
        match part.as_bytes().iter().position(|&b| b == b']') {
            Some(end) => {
                if !st.push_token(&part[..=end]) {
                    return false;
                }
            }
            None => {
                // A stray '[' with no closing ']' — `split_each` would still surface
                // it as a (non-`]`-terminated) token; fall back to the faithful path.
                if !st.push_token(part) {
                    return false;
                }
            }
        }
    }
    st.finish()
}

/// Incremental state machine for validating one stroke segment under the "stripped"
/// rules (ignore non-bracketed parts; `*` removed before validation). Shared by the
/// string and the parts-based validators.
struct SegState {
    any_bracket: bool,
    passed_vowel: bool,
    right_before_vowel: bool,
    right_hyphen_seen: bool,
    left_cur: usize,
    vowel_cur: usize,
    right_cur: usize,
}

impl SegState {
    #[inline]
    fn new() -> Self {
        SegState {
            any_bracket: false,
            passed_vowel: false,
            right_before_vowel: false,
            right_hyphen_seen: false,
            left_cur: 0,
            vowel_cur: 0,
            right_cur: 0,
        }
    }

    /// Process one token (a `[...]` bracketed part or a lowercase run). Returns false
    /// on an order/placement violation. Non-bracketed tokens are ignored (stripped).
    #[inline]
    fn push_token(&mut self, part: &str) -> bool {
        if !part.starts_with('[') {
            return true; // strip_unmatched_letters drops non-bracketed runs
        }
        self.any_bracket = true;
        let is_right = part.starts_with("[-");
        let is_vowel = part.starts_with("[e|") || part == "[*]";
        if !self.passed_vowel && is_right {
            self.right_before_vowel = true;
        }
        if self.passed_vowel && !is_right {
            return false;
        }
        if part.starts_with("[e|") {
            self.passed_vowel = true;
        }
        if is_right {
            if !self.right_hyphen_seen {
                if !advance_key('-', RIGHT_KEYS, &mut self.right_cur) {
                    return false;
                }
                self.right_hyphen_seen = true;
            }
            for c in part[2..part.len() - 1].chars() {
                if c == '*' {
                    continue;
                }
                if !advance_key(c, RIGHT_KEYS, &mut self.right_cur) {
                    return false;
                }
            }
            true
        } else if is_vowel {
            consume_inner_keys_nostar(part, VOWEL_KEYS, &mut self.vowel_cur)
        } else {
            consume_inner_keys_nostar(part, LEFT_KEYS, &mut self.left_cur)
        }
    }

    #[inline]
    fn finish(&self) -> bool {
        if !self.any_bracket {
            return true; // empty stripped segment -> dropped -> valid
        }
        !(self.passed_vowel && self.right_before_vowel)
    }
}

pub fn validate(strokes: &str) -> bool {
    // Python first strips ALL '*' from the whole string, then splits on '/'.
    let owned;
    let strokes: &str = if strokes.contains('*') {
        owned = strokes.replace('*', "");
        &owned
    } else {
        strokes
    };
    for stroke in strokes.split('/') {
        if !validate_stroke(stroke) {
            return false;
        }
    }
    true
}

fn validate_stroke(stroke: &str) -> bool {
    let mut passed_vowel = false;
    let mut right_before_vowel = false;
    let mut right_hyphen_seen = false;
    // Incremental order-check cursors (index into the *_KEYS arrays). Each key in a
    // zone must appear at or after the zone's current cursor (the keys are a strict
    // ordering); we advance the cursor as keys are consumed. This replaces the three
    // per-call `Vec<char>` accumulators + `keys_in_order` sweeps with a single pass.
    let mut left_cur = 0usize;
    let mut vowel_cur = 0usize;
    let mut right_cur = 0usize;
    let mut ok = true;
    let mut early_false = false;

    split_each(stroke, |part| {
        if early_false {
            return;
        }
        let is_bracketed = part.starts_with('[') && part.ends_with(']');
        if !is_bracketed {
            ok = false;
            early_false = true;
            return;
        }
        let is_right = part.starts_with("[-");
        let is_vowel = part.starts_with("[e|") || part == "[*]";
        if !passed_vowel && is_right {
            right_before_vowel = true;
        }
        if passed_vowel && !is_right {
            ok = false;
            early_false = true;
            return;
        }
        if part.starts_with("[e|") {
            passed_vowel = true;
        }
        // consume order keys incrementally
        if is_right {
            if !right_hyphen_seen {
                if !advance_key('-', RIGHT_KEYS, &mut right_cur) {
                    ok = false;
                    early_false = true;
                    return;
                }
                right_hyphen_seen = true;
            }
            for c in part[2..part.len() - 1].chars() {
                if !advance_key(c, RIGHT_KEYS, &mut right_cur) {
                    ok = false;
                    early_false = true;
                    return;
                }
            }
        } else if is_vowel {
            if !consume_inner_keys(part, VOWEL_KEYS, &mut vowel_cur) {
                ok = false;
                early_false = true;
            }
        } else if !consume_inner_keys(part, LEFT_KEYS, &mut left_cur) {
            ok = false;
            early_false = true;
        }
    });

    if !ok {
        return false;
    }
    if passed_vowel && right_before_vowel {
        return false;
    }
    true
}

/// Advance `cur` past the first occurrence of `key` in `keys` at index >= `cur`.
/// Returns false if `key` does not occur at/after `cur` (order violation).
#[inline]
fn advance_key(key: char, keys: &[char], cur: &mut usize) -> bool {
    let mut i = *cur;
    while i < keys.len() {
        if keys[i] == key {
            *cur = i + 1;
            return true;
        }
        i += 1;
    }
    false
}

/// Consume a bracketed part's inner key letters against `keys`, advancing `cur`.
/// Mirrors `push_inner_keys` + `keys_in_order` but without allocating.
#[inline]
fn consume_inner_keys(part: &str, keys: &[char], cur: &mut usize) -> bool {
    let inner = part
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(part);
    let inner = inner.strip_prefix("e|").unwrap_or(inner);
    for c in inner.chars() {
        if !advance_key(c, keys, cur) {
            return false;
        }
    }
    true
}

/// Like `consume_inner_keys` but skips any '*' (the stripped-path validator removes
/// all '*' before validating, so `[*]` contributes no keys).
#[inline]
fn consume_inner_keys_nostar(part: &str, keys: &[char], cur: &mut usize) -> bool {
    let inner = part
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(part);
    let inner = inner.strip_prefix("e|").unwrap_or(inner);
    for c in inner.chars() {
        if c == '*' {
            continue;
        }
        if !advance_key(c, keys, cur) {
            return false;
        }
    }
    true
}

