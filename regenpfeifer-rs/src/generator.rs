//! Stroke generator — orchestrates split -> aggregate -> emphasize -> match ->
//! validate -> strip. Faithful port of stroke_generator.py (order-independent
//! variant: fresh per-word, like the reference's cleared valid_strokes_dict).

use crate::cache::MatchCache;
use crate::emphasize::Emphasizer;
use crate::matcher::Matcher;
use crate::split::WordSplitter;
use crate::stroke;
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
            for m in self.matcher.match_word(&emphasized) {
                if stroke::validate(&m) {
                    let f = stroke::remove_markup(&m);
                    let f = stroke::reposition_asterisks(&f);
                    if !out.contains(&f) {
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
        let chunk = |i: usize, j: usize| -> String { prefix_chunks[j][prefix_chunks[i].len()..].to_string() };

        // A contiguous group can only transduce to ONE valid stroke if its input
        // chunk is short enough (one vowel nucleus + bounded onset/coda). The
        // longest valid chunk observed empirically is 10 chars; 16 is a safe,
        // sound cap (longer chunks always yield an empty match, so skipping them
        // changes nothing) that prunes the O(n^2) matcher calls on long compounds.
        const MAX_CHUNK_CHARS: usize = 16;

        // groups[i] = Vec of (j, Arc<[String]>) for valid groups starting at i.
        let mut groups: Vec<Vec<(usize, Arc<[String]>)>> = vec![Vec::new(); n];
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
        // To bound pathological blowup we cap the number of outlines per word.
        const MAX_OUTLINES: usize = 4096;
        let mut dp: Vec<Vec<String>> = vec![Vec::new(); n + 1];
        dp[n].push(String::new());
        for i in (0..n).rev() {
            let mut out: Vec<String> = Vec::new();
            'outer: for (j, strokes) in &groups[i] {
                for s in strokes.iter() {
                    for tail in &dp[*j] {
                        let combined = if tail.is_empty() {
                            s.clone()
                        } else {
                            format!("{s}/{tail}")
                        };
                        if !out.contains(&combined) {
                            out.push(combined);
                        }
                        if out.len() >= MAX_OUTLINES {
                            break 'outer;
                        }
                    }
                }
            }
            dp[i] = out;
        }

        dp[0].clone()
    }
}
