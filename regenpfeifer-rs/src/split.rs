//! Compound splitting (WordPartSplitter) + German syllabification
//! (WordSyllableSplitter, Kirsch algorithm). Faithful port.

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Compound part splitter: greedy — split at the FIRST position where the
// accumulated prefix is a known word; reset and continue. If a trailing
// remainder is left over (and at least one split was made), drop the last split.
// ---------------------------------------------------------------------------

pub struct PartSplitter {
    words: HashSet<String>,
}

impl PartSplitter {
    pub fn new(words: &[String]) -> Self {
        let set: HashSet<String> = words.iter().cloned().collect();
        PartSplitter { words: set }
    }

    fn get_split_positions(&self, chars: &[char]) -> Vec<usize> {
        let mut split_positions = Vec::new();
        let mut current = String::new();
        for (i, &c) in chars.iter().enumerate() {
            // word is already lowercased upstream; .lower() in Python is a no-op here
            current.push(c);
            if self.words.contains(&current) {
                split_positions.push(i + 1);
                current.clear();
            }
        }
        if !current.is_empty() && !split_positions.is_empty() {
            split_positions.pop();
        }
        split_positions
    }

    pub fn split(&self, word: &str) -> Vec<String> {
        let chars: Vec<char> = word.chars().collect();
        let positions: HashSet<usize> = self.get_split_positions(&chars).into_iter().collect();
        let mut syllables = Vec::new();
        let mut current = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if positions.contains(&i) {
                syllables.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        if !current.is_empty() {
            syllables.push(current);
        }
        syllables
    }
}

// ---------------------------------------------------------------------------
// Syllable splitter (Kirsch). Faithful port of word_syllable_splitter.py.
// ---------------------------------------------------------------------------

const VOWELS: &[&str] = &["a", "e", "i", "o", "u", "ä", "ö", "ü"];
const SPLIT_VOWEL_PAIRS: &[&str] = &["io", "eie", "eue"];
const PREVENTING_VOWEL_SPLIT_RIGHT: &[&str] = &["nen"];
const SPLITTERS: &[&str] = &["sst", "ier"];
const NON_CONNECTORS: &[&str] = &["chl"];
const CERTAIN_CONNECTORS: &[&str] = &["sch", "ch", "ck", "schl", "chl"];
const LEFT_CONNECTORS: &[&str] = &["er", "an"];
const LEFT_NON_CONNECTORS: &[&str] = &["ana"];
const POSSIBLE_CONNECTORS: &[&str] = &[
    "ph", "pf", "br", "pl", "tr", "gr", "sp", "kl", "zw", "spr", "fr", "gl", "bl", "ren",
];
const SEPARATORS: &[&str] = &[
    "-", "*", ";", ".", "+", "=", ")", "(", "&", "!", "?", "", ":", " ", "_", "~",
];

#[inline]
fn is_vowel(s: &str) -> bool {
    VOWELS.contains(&s)
}
#[inline]
fn is_separator(s: &str) -> bool {
    SEPARATORS.contains(&s)
}

pub struct SyllableSplitter;

impl SyllableSplitter {
    pub fn new() -> Self {
        SyllableSplitter
    }

    /// chars: lowercased word as char vec. Returns a string slice helper.
    fn ch(chars: &[char], i: usize) -> String {
        chars[i].to_string()
    }

    fn handle_v_extended(v: &str, i: usize, chars: &[char]) -> String {
        let mut v_new = v.to_string();
        let z_minus_2 = if i > 1 { Self::ch(chars, i - 2) } else { String::new() };
        let z_minus_3 = if i > 2 { Self::ch(chars, i - 3) } else { String::new() };

        let v_ext2 = format!("{z_minus_2}{v}");
        if Self::in_any(&v_ext2) {
            v_new = v_ext2.clone();
        }
        let v_ext3 = format!("{z_minus_3}{v_ext2}");
        if Self::in_any(&v_ext3) {
            v_new = v_ext3;
        }
        v_new
    }

    #[inline]
    fn in_any(s: &str) -> bool {
        CERTAIN_CONNECTORS.contains(&s)
            || POSSIBLE_CONNECTORS.contains(&s)
            || SPLITTERS.contains(&s)
            || SPLIT_VOWEL_PAIRS.contains(&s)
            || NON_CONNECTORS.contains(&s)
    }

    fn is_split_candidate(z: &str, z1: &str, z_minus_1: &str) -> bool {
        is_vowel(z1) && !is_vowel(z) && !is_separator(z) && !is_separator(z_minus_1)
    }

    fn add_split_position(pos: i64, chars: &[char], out: &mut Vec<usize>) {
        let len = chars.len() as i64;
        if 1 < pos && pos < len - 1 {
            out.push(pos as usize);
        }
    }

    fn validate_and_add_position(
        v: &str,
        i: usize,
        z1: &str,
        chars: &[char],
        out: &mut Vec<usize>,
    ) {
        let v_len = v.chars().count() as i64;
        if NON_CONNECTORS.contains(&v) {
            return;
        }
        if CERTAIN_CONNECTORS.contains(&v) {
            Self::add_split_position(i as i64 - v_len + 1, chars, out);
        } else if SPLITTERS.contains(&v) {
            Self::add_split_position(i as i64, chars, out);
        } else if LEFT_NON_CONNECTORS.contains(&format!("{v}{z1}").as_str()) {
            Self::add_split_position(i as i64 + 2, chars, out);
        } else if LEFT_CONNECTORS.contains(&v) {
            Self::add_split_position(i as i64 + 1, chars, out);
        } else if POSSIBLE_CONNECTORS.contains(&v) {
            Self::add_split_position(i as i64 - v_len + 1, chars, out);
        } else {
            Self::add_split_position(i as i64, chars, out);
        }
    }

    fn compute_for_position(v: &str, i: usize, chars: &[char], out: &mut Vec<usize>) {
        let z_minus_1 = Self::ch(chars, i - 1);
        let z = Self::ch(chars, i);
        let word_length = chars.len();
        let z1 = if word_length > i + 1 {
            Self::ch(chars, i + 1)
        } else {
            String::new()
        };
        if SPLIT_VOWEL_PAIRS.contains(&v) {
            let z_i_plus: String = chars[i + 1..].iter().collect();
            if !PREVENTING_VOWEL_SPLIT_RIGHT.contains(&z_i_plus.as_str()) {
                out.push(i);
            }
        } else if Self::is_split_candidate(&z, &z1, &z_minus_1) {
            Self::validate_and_add_position(v, i, &z1, chars, out);
        }
    }

    fn compute_for_word(word: &str) -> Vec<usize> {
        let chars: Vec<char> = word.to_lowercase().chars().collect();
        let mut out = Vec::new();
        let word_length = chars.len();
        if word_length > 2 {
            let mut split_allowed = false;
            for i in 1..word_length {
                let z_minus_1 = Self::ch(&chars, i - 1);
                if !split_allowed && is_vowel(&z_minus_1) {
                    split_allowed = true;
                }
                if split_allowed {
                    let z = Self::ch(&chars, i);
                    let v = format!("{z_minus_1}{z}");
                    let v = Self::handle_v_extended(&v, i, &chars);
                    Self::compute_for_position(&v, i, &chars, &mut out);
                }
            }
        }
        out
    }

    pub fn split(&self, word: &str) -> Vec<String> {
        let positions: HashSet<usize> = Self::compute_for_word(word).into_iter().collect();
        let chars: Vec<char> = word.chars().collect();
        let mut syllables = Vec::new();
        let mut current = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if positions.contains(&i) {
                syllables.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        if !current.is_empty() {
            syllables.push(current);
        }
        syllables
    }
}

pub struct WordSplitter {
    part: PartSplitter,
    syl: SyllableSplitter,
}

impl WordSplitter {
    pub fn new(words: &[String]) -> Self {
        WordSplitter {
            part: PartSplitter::new(words),
            syl: SyllableSplitter::new(),
        }
    }

    pub fn split(&self, word: &str) -> Vec<String> {
        let parts = self.part.split(word);
        let mut syllables = Vec::new();
        for part in &parts {
            syllables.extend(self.syl.split(part));
        }
        syllables
    }
}
