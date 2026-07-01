//! Stroke generator — orchestrates split -> aggregate -> emphasize -> match ->
//! validate -> strip. Faithful port of stroke_generator.py (order-independent
//! variant: fresh per-word, like the reference's cleared valid_strokes_dict).

use crate::cache::MatchCache;
use crate::emphasize::Emphasizer;
use crate::matcher::Matcher;
use crate::split::WordSplitter;
use crate::stroke;
use rustc_hash::FxHashSet;
use std::sync::Arc;

pub struct Generator {
    splitter: WordSplitter,
    emphasizer: Emphasizer,
    matcher: Matcher,
    cache: MatchCache,
}

impl Generator {
    pub fn new(splitter: WordSplitter, emphasizer: Emphasizer, matcher: Matcher) -> Self {
        Generator {
            splitter,
            emphasizer,
            matcher,
            cache: MatchCache::new(),
        }
    }

    /// Valid single-stroke outlines (stripped + repositioned) for one contiguous
    /// syllable group whose concatenation is `chunk`. `is_whole_word` selects the
    /// emphasis word_type (matches the brute path where syllable == word). Cached
    /// globally on the emphasized chunk.
    fn group_strokes(&self, chunk: &str, is_whole_word: bool, word_type: &str) -> Arc<[String]> {
        let emphasized = if is_whole_word {
            self.emphasizer.emphasize(chunk, word_type)
        } else {
            self.emphasizer.emphasize(chunk, "other")
        };
        self.cache.get_or_compute(&emphasized, || {
            let mut out: Vec<String> = Vec::new();
            let mut seen: FxHashSet<String> = FxHashSet::default();
            for m in self.matcher.match_word(&emphasized) {
                if stroke::validate(&m) {
                    let f = stroke::remove_markup(&m);
                    let f = stroke::reposition_asterisks(&f);
                    if seen.insert(f.clone()) {
                        out.push(f);
                    }
                }
            }
            out
        })
    }

    pub fn generate(&self, word: &str, word_type: &str) -> Vec<String> {
        let word = word.to_lowercase();
        let syllables = self.splitter.split(&word);
        let n = syllables.len();
        if n == 0 {
            return Vec::new();
        }

        // Smart aggregation: instead of enumerating all 2^(n-1) join/slash combos
        // and validating each, partition the syllables into contiguous groups.
        // A group is only usable if its concatenation transduces to >=1 VALID
        // single stroke (the brute force discards every other combination during
        // validation anyway). The outline set is then every slash-join over a
        // valid partition. Equivalent to the brute force because validation is
        // per-slash-segment and stripping is per-segment.

        // Precompute group strokes for each contiguous range [i, j).
        // group_valid[i][j-i-1] = valid strokes for syllables i..j.
        let mut prefix_chunks: Vec<String> = Vec::with_capacity(n + 1);
        prefix_chunks.push(String::new());
        {
            let mut acc = String::new();
            for s in &syllables {
                acc.push_str(s);
                prefix_chunks.push(acc.clone());
            }
        }
        let chunk = |i: usize, j: usize| -> String {
            prefix_chunks[j][prefix_chunks[i].len()..].to_string()
        };

        // A contiguous group can only transduce to ONE valid stroke if its input
        // chunk is short enough (one vowel nucleus + bounded onset/coda). The
        // longest valid chunk observed empirically is 10 chars; 16 is a safe,
        // sound cap (longer chunks always yield an empty match, so skipping them
        // changes nothing) that prunes the O(n^2) matcher calls on long compounds.
        const MAX_CHUNK_CHARS: usize = 16;

        // groups[i] = Vec of (j, Arc<[String]>) for valid groups starting at i.
        let mut groups: Vec<Vec<(usize, Arc<[String]>)>> = vec![Vec::new(); n];
        // `i` indexes groups[i], feeds chunk(i, j), and gates is_whole (i == 0), so an
        // explicit range loop reads clearer here than enumerate().
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            for j in (i + 1)..=n {
                let c = chunk(i, j);
                if c.chars().count() > MAX_CHUNK_CHARS {
                    break; // longer j only makes the chunk longer
                }
                let is_whole = i == 0 && j == n; // chunk == word (joined whole)
                let strokes = self.group_strokes(&c, is_whole, word_type);
                if !strokes.is_empty() {
                    groups[i].push((j, strokes));
                }
            }
        }

        // DP from the right: outlines starting at position i.
        // Cap the number of outlines per word as a backstop against pathological
        // blowup. This never bites for the shipped word list + asset set (max ~12
        // outlines/word observed; the output is unchanged with the cap raised to 1M),
        // so if it ever truncates, that's new behavior worth knowing about — warn
        // loudly rather than silently dropping output.
        const MAX_OUTLINES: usize = 4096;
        let mut dp: Vec<Vec<String>> = vec![Vec::new(); n + 1];
        dp[n].push(String::new());
        for i in (0..n).rev() {
            let mut out: Vec<String> = Vec::new();
            let mut seen: FxHashSet<String> = FxHashSet::default();
            'outer: for (j, strokes) in &groups[i] {
                for s in strokes.iter() {
                    for tail in &dp[*j] {
                        let combined = if tail.is_empty() {
                            s.clone()
                        } else {
                            format!("{s}/{tail}")
                        };
                        if seen.insert(combined.clone()) {
                            out.push(combined);
                        }
                        if out.len() >= MAX_OUTLINES {
                            eprintln!(
                                "warning: '{word}' hit MAX_OUTLINES ({MAX_OUTLINES}); \
                                 output truncated",
                            );
                            break 'outer;
                        }
                    }
                }
            }
            dp[i] = out;
        }

        let result = dp[0].clone();
        if !result.is_empty() {
            return result;
        }
        // Trailing-schwa fallback (mkrnr's unwired final_patterns.json): the syllabifier
        // mis-splits words ending in an unstressed -e/-en (unsere -> uns/ere), so they
        // don't generate. Peel the ending into its own stroke (E / EPB). Only fires when
        // normal generation produced nothing, so it can't change a working word. The -e/-en
        // are ASCII, so byte-slicing the tail is codepoint-safe. Each call recurses on a
        // strictly shorter stem, so the chain terminates.
        // These peels chain with the vowel-initial recovery below: a word can be both
        // -e/-en-ending AND vowel-initial (e.g. alarmlampe), so each returns only when
        // it actually produced an outline, else falls through to the next recovery.
        let nchars = word.chars().count();
        if word.ends_with("en") && nchars > 4 {
            let stem = &word[..word.len() - 2];
            let out: Vec<String> = self
                .generate(stem, word_type)
                .iter()
                .map(|s| format!("{s}/EPB"))
                .collect();
            if !out.is_empty() {
                return out;
            }
        }
        if word.ends_with('e') && nchars > 3 {
            let stem = &word[..word.len() - 1];
            let out: Vec<String> = self
                .generate(stem, word_type)
                .iter()
                .map(|s| format!("{s}/E"))
                .collect();
            if !out.is_empty() {
                return out;
            }
        }
        // Vowel-initial fallback: bare-vowel-initial words are left un-split
        // (egal -> ['egal']) so the second vowel can't reduce. Peel the leading vowel
        // into its own stroke and generate the rest. Guarded to a single leading vowel
        // (not au-/ei- diphthongs); additive, only fires when nothing else generated.
        // The remainder (word[1:]) begins with a consonant, so it can't re-enter this
        // branch, and it is strictly shorter -- so the recursion terminates.
        const VOWELS: &str = "aeiouäöü";
        let mut cs = word.chars();
        if let (Some(first), Some(second)) = (cs.next(), cs.next()) {
            if nchars > 2 && VOWELS.contains(first) && !VOWELS.contains(second) {
                let head = self.generate(&first.to_string(), word_type);
                let rest_str: String = word.chars().skip(1).collect();
                let rest = self.generate(&rest_str, word_type);
                if !head.is_empty() && !rest.is_empty() {
                    return rest.iter().map(|r| format!("{}/{}", head[0], r)).collect();
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::Generator;
    use crate::emphasize::Emphasizer;
    use crate::matcher::Matcher;
    use crate::patterns;
    use crate::split::WordSplitter;
    use std::path::PathBuf;

    fn test_generator() -> Generator {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/patterns");
        let vowel = patterns::load(&dir.join("vowel_patterns.json"));
        let left = patterns::load(&dir.join("left_patterns.json"));
        let right = patterns::load(&dir.join("right_patterns.json"));
        // Minimal word list: each test word is a single morpheme, not a compound of
        // another, so the (word-list-driven) part-splitter never splits them — output
        // is independent of the list and matches the full-build reference.
        let words: Vec<String> = ["kopf", "pferd", "der", "wolf", "elf"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        Generator::new(
            WordSplitter::new(&words),
            Emphasizer::new(),
            Matcher::new(vowel, left, right),
        )
    }

    #[test]
    fn generate_golden_outlines() {
        let g = test_generator();
        // Pinned against the byte-identical full-build reference (REGEN_DUMP).
        assert_eq!(g.generate("Kopf", "sg").join(","), "KO*FP"); // pf coda, *-distinguished from ch
        assert_eq!(g.generate("Pferd", "sg").join(","), "TKPERD"); // pf onset [TKP]
        assert_eq!(g.generate("Wolf", "sg").join(","), "WOFL"); // -lf coda [-FL]
        assert_eq!(g.generate("elf", "cd").join(","), "EFL");
        assert_eq!(g.generate("der", "dt").join(","), "TKER");
    }
}
