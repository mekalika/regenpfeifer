//! Loads the pattern JSON files, preserving insertion order (longest-first as
//! authored). The Python code escapes `[`, `]`, `|` for regex use; we don't use
//! regex, we use plain substring replacement, so we keep the raw (unescaped)
//! pattern strings.

use serde_json::Value;
use std::fs;
use std::path::Path;

/// An ordered list of (pattern, replacement) pairs.
pub type OrderedPatterns = Vec<(String, String)>;

pub fn load(path: &Path) -> OrderedPatterns {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read pattern file {}: {e}", path.display()));
    // serde_json preserves object order only with the "preserve_order" feature;
    // to stay dependency-light we parse manually preserving order.
    let v: Value = serde_json::from_str(&text).expect("invalid pattern JSON");
    let obj = v.as_object().expect("pattern file must be a JSON object");
    // serde_json's Map is a BTreeMap by default (sorted), which would DESTROY the
    // authored longest-first order. So we re-extract order from the raw text.
    let _ = obj; // not used for ordering
    parse_ordered(&text)
}

/// Parse a flat JSON object `{ "k": "v", ... }` preserving key insertion order.
/// The pattern files are simple flat string->string maps with no nested braces.
fn parse_ordered(text: &str) -> OrderedPatterns {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    // skip to first '{'
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    i += 1;
    loop {
        // find next '"'
        while i < bytes.len() && bytes[i] != b'"' && bytes[i] != b'}' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'}' {
            break;
        }
        let (key, ni) = read_json_string(text, i);
        i = ni;
        // find ':'
        while i < bytes.len() && bytes[i] != b':' {
            i += 1;
        }
        i += 1;
        // find value opening '"'
        while i < bytes.len() && bytes[i] != b'"' {
            i += 1;
        }
        let (val, ni) = read_json_string(text, i);
        i = ni;
        out.push((key, val));
    }
    out
}

/// Read a JSON string literal starting at the opening quote at `start`.
/// Returns (decoded_string, index_after_closing_quote).
fn read_json_string(text: &str, start: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    debug_assert_eq!(bytes[start], b'"');
    let mut i = start + 1;
    let mut s = String::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' {
            // escape
            let n = bytes[i + 1];
            match n {
                b'"' => s.push('"'),
                b'\\' => s.push('\\'),
                b'/' => s.push('/'),
                b'n' => s.push('\n'),
                b't' => s.push('\t'),
                b'u' => {
                    let hex = &text[i + 2..i + 6];
                    let cp = u32::from_str_radix(hex, 16).unwrap();
                    if let Some(ch) = char::from_u32(cp) {
                        s.push(ch);
                    }
                    i += 6;
                    continue;
                }
                other => s.push(other as char),
            }
            i += 2;
            continue;
        }
        if c == b'"' {
            return (s, i + 1);
        }
        // copy a full UTF-8 char
        let ch_len = utf8_len(c);
        s.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    (s, i)
}

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
