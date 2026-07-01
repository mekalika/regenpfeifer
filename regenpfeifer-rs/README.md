# regen-rs — a Rust port of the Regenpfeifer outline generator

A Rust reimplementation of [Regenpfeifer's](https://github.com/mkrnr/regenpfeifer) word→outline
generator (the *generate* direction): the same output as the Python generator, much faster.

**This commit — the faithful port.** The pipeline — splitter, emphasizer, fixpoint matcher,
validator, markup stripping — is a straight translation of the Python. The one liberty, taken
from the start, is in the generator: it matches each contiguous syllable group once (a
partition DP) instead of enumerating the Python's 2^(n-1) join/slash combinations. The output
is the same because the brute force validates per slash segment anyway — only groups that form
a valid single stroke survive, and every outline is a slash-join over such groups. This commit
predates the coverage fixes folded into the final commit, so it reproduces the *original*
Python's output; the later commits keep that output and make the matcher faster (and the final
one adds the coverage fixes).

## Performance

Full 823k-word build:

| version | cores | time |
|---|---|---|
| Python | 1 | ~7h |
| Python | 12 | ~36 min |
| **Rust, faithful port** | 12 | **~60s** |

~36× over the parallel Python on the same 12 cores. The next two commits cut this to ~24s
(optimizing the fixpoint matcher's internals) then ~6.1s (replacing the fixpoint with a
transducer).

## Algorithm

Faithful ports of the splitter (compound + Kirsch syllabifier), the emphasizer, the fixpoint
matcher, and the asterisk/markup utilities (including the `before_asterisk = "ZSTKPWHRAO"`
Z-fix). Generation partitions each word's syllables into contiguous groups, matches each group
once (memoized on the emphasized group), and takes every slash-join over groups that yield a
valid single stroke — equivalent to the Python's 2^(n-1) enumeration, minus the redundant
matching. The matcher itself is the Python's set-fixpoint, translated directly; the next two
commits speed it up and then replace it:

1. `matcher.rs` internals (next commit) — hash sets, a first-byte pattern index, single-pass
   replace, per-thread scratch. Same fixpoint, much less work per call.
2. `matcher.rs` → **transducer** (final commit) — onset and coda reduce independently, with no
   whole-word fixpoint.

Build, run, and validation instructions are in the final commit's README.
