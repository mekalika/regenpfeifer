//! Original baseline matcher (reconstructed for A/B timing).
use crate::patterns::OrderedPatterns;
use crate::stroke;
use std::collections::BTreeSet;

pub struct Matcher {
    vowel: OrderedPatterns,
    left: OrderedPatterns,
    right: OrderedPatterns,
}

fn replace_if_changed(s: &str, pat: &str, repl: &str) -> Option<String> {
    if pat.is_empty() || !s.contains(pat) {
        return None;
    }
    let new = s.replace(pat, repl);
    if new.chars().count() != s.chars().count() {
        Some(new)
    } else {
        None
    }
}

fn replace_all(s: &str, pat: &str, repl: &str) -> String {
    if pat.is_empty() {
        return s.to_string();
    }
    s.replace(pat, repl)
}

impl Matcher {
    pub fn new(vowel: OrderedPatterns, left: OrderedPatterns, right: OrderedPatterns) -> Self {
        Matcher { vowel, left, right }
    }

    pub fn match_word(&self, emphasized_word: &str) -> Vec<String> {
        let mut word = emphasized_word.to_string();
        for (pat, repl) in &self.vowel {
            word = replace_all(&word, pat, repl);
        }
        let mut matches: BTreeSet<String> = BTreeSet::new();
        matches.insert(word);
        let mut final_matches: BTreeSet<String> = BTreeSet::new();
        loop {
            let mut new_matches: BTreeSet<String> = BTreeSet::new();
            for m in &matches {
                let generated = self.generate_matches(m);
                if generated.is_empty() {
                    final_matches.insert(m.clone());
                } else {
                    new_matches.extend(generated);
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

    fn generate_matches(&self, m: &str) -> BTreeSet<String> {
        let word_parts = stroke::split(m);
        let mut generated: BTreeSet<String> = BTreeSet::new();
        generated.extend(self.generate_left_consonants(&word_parts));
        generated.extend(self.generate_right_consonants(&word_parts));
        let mut validated: BTreeSet<String> = BTreeSet::new();
        for g in generated {
            let stripped = stroke::strip_unmatched_letters(&g);
            if stroke::validate(&stripped) {
                validated.insert(g);
            }
        }
        validated
    }

    fn generate_left_consonants(&self, word_parts: &[String]) -> BTreeSet<String> {
        let mut parts: Vec<String> = word_parts.to_vec();
        let mut generated: BTreeSet<String> = BTreeSet::new();
        for i in 0..parts.len() {
            if parts[i].starts_with("[e|") {
                break;
            }
            for (pat, repl) in &self.left {
                if let Some(matched) = replace_if_changed(&parts[i], pat, repl) {
                    parts[i] = matched;
                    generated.insert(stroke::join(&parts));
                }
            }
        }
        generated
    }

    fn generate_right_consonants(&self, word_parts: &[String]) -> BTreeSet<String> {
        let mut generated: BTreeSet<String> = BTreeSet::new();
        let mut after_vowel = false;
        for i in 0..word_parts.len() {
            if after_vowel {
                for (pat, repl) in &self.right {
                    if let Some(matched) = replace_if_changed(&word_parts[i], pat, repl) {
                        let mut copy = word_parts.to_vec();
                        copy[i] = matched;
                        generated.insert(stroke::join(&copy));
                    }
                }
            } else if word_parts[i].starts_with("[e|") {
                after_vowel = true;
            }
        }
        generated
    }
}
