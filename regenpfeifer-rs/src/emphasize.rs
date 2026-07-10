//! Word emphasizer — marks the stressed vowel as `[e|X]` via prefix heuristics.
//! Faithful port of word_emphasizer.py.

const NEVER_EMP_PREFIXES: &[&str] = &["be", "ent", "er", "ver", "zer"];
const DIPHTONGS: &[&str] = &["au", "äu", "eu", "ei", "ey", "ai", "ay"];
const USUALLY_EMP_PREFIXES: &[&str] = &["dar", "da", "her", "hin", "vor", "zu"];
const EMP_PREFIXES: &[&str] = &[
    "ab", "an", "auf", "aus", "bei", "ein", "empor", "fort", "los", "mit", "nach", "nieder", "weg",
    "weiter", "wieder",
];
const VOWELS: &[char] = &['a', 'e', 'i', 'o', 'u', 'ä', 'ö', 'ü'];

pub struct Emphasizer;

impl Emphasizer {
    pub fn new() -> Self {
        Emphasizer
    }

    fn emp_vowel(word: &str) -> String {
        let chars: Vec<char> = word.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if VOWELS.contains(&c) {
                let mut out = String::new();
                out.extend(&chars[..i]);
                out.push_str("[e|");
                out.push(c);
                out.push(']');
                out.extend(&chars[i + 1..]);
                return out;
            }
        }
        word.to_string()
    }

    fn never_emp_prefixes(word_type: &str) -> Vec<&'static str> {
        // Python: returns [] for word_type=="in"; ppart adds "ge"; verb forms
        // (start with 1/2/3/inf/part) add "miss","wider"; else [].
        if word_type == "in" {
            return Vec::new();
        }
        if word_type == "ppart" {
            let mut v: Vec<&'static str> = NEVER_EMP_PREFIXES.to_vec();
            v.push("ge");
            return v;
        }
        // verb forms
        for vf in ["1", "2", "3", "inf", "part"] {
            if word_type.starts_with(vf) {
                let mut v: Vec<&'static str> = NEVER_EMP_PREFIXES.to_vec();
                v.push("miss");
                v.push("wider");
                return v;
            }
        }
        Vec::new()
    }

    fn get_usually_emp_prefix(word: &str) -> &'static str {
        // Longest match wins ("dar" must beat "da" for daran).
        let mut matched = "";
        for p in USUALLY_EMP_PREFIXES {
            if word.starts_with(p) && p.len() > matched.len() {
                matched = p;
            }
        }
        matched
    }

    /// Strip a leading `prefix` once (anchored), like re.sub("^prefix", "", word).
    fn strip_prefix_once(word: &str, prefix: &str) -> String {
        if !prefix.is_empty() {
            if let Some(rest) = word.strip_prefix(prefix) {
                return rest.to_string();
            }
        }
        word.to_string()
    }

    pub fn emphasize(&self, word: &str, word_type: &str) -> String {
        let mut word = word.to_string();
        let mut matched_never_emp_prefix = String::new();
        for never in Self::never_emp_prefixes(word_type) {
            if word == never {
                return Self::emp_vowel(&word);
            }
            if word.starts_with(never) {
                // Strip only the first matching prefix (beerdigen keeps its be).
                matched_never_emp_prefix = never.to_string();
                word = Self::strip_prefix_once(&word, never);
                break;
            }
        }

        for diph in DIPHTONGS {
            if word.contains(diph) {
                // Mark only the first occurrence (aufbauen gets ONE stress).
                let replaced = word.replacen(diph, &format!("[e|{diph}]"), 1);
                return format!("{matched_never_emp_prefix}{replaced}");
            }
        }

        let matched_usually_emp_prefix = Self::get_usually_emp_prefix(&word);
        word = Self::strip_prefix_once(&word, matched_usually_emp_prefix);

        for emp in EMP_PREFIXES {
            if word.starts_with(emp) {
                word = Self::strip_prefix_once(&word, emp);
                return format!(
                    "{}{}{}{}",
                    matched_never_emp_prefix,
                    matched_usually_emp_prefix,
                    Self::emp_vowel(emp),
                    word
                );
            }
        }

        if !matched_usually_emp_prefix.is_empty() {
            return format!(
                "{}{}{}",
                matched_never_emp_prefix,
                Self::emp_vowel(matched_usually_emp_prefix),
                word
            );
        }

        format!("{}{}", matched_never_emp_prefix, Self::emp_vowel(&word))
    }
}
