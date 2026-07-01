//! Loads the pattern JSON files, preserving insertion order (longest-first as
//! authored). The Python code escapes `[`, `]`, `|` for regex use; this matcher uses
//! plain substring replacement instead, so the raw (unescaped) pattern strings are
//! kept.

use serde_json::Value;
use std::fs;
use std::path::Path;

/// An ordered list of (pattern, replacement) pairs.
pub type OrderedPatterns = Vec<(String, String)>;

pub fn load(path: &Path) -> OrderedPatterns {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read pattern file {}: {e}", path.display()));
    // serde_json's `preserve_order` feature backs its Map with an IndexMap, so object
    // iteration follows the authored (longest-first) key order — which the matcher relies
    // on (applying a shorter pattern before a longer one that contains it changes output).
    let v: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid pattern JSON {}: {e}", path.display()));
    let obj = v
        .as_object()
        .unwrap_or_else(|| panic!("pattern file {} must be a JSON object", path.display()));
    obj.iter()
        .map(|(k, val)| {
            let repl = val
                .as_str()
                .unwrap_or_else(|| panic!("pattern value for key {k:?} must be a string"));
            (k.clone(), repl.to_string())
        })
        .collect()
}
