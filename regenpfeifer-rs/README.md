# regen-rs — a Rust port of the Regenpfeifer outline generator

A Rust reimplementation of [Regenpfeifer's](https://github.com/mkrnr/regenpfeifer) word→outline
generator (the *generate* direction): the same output as the Python generator, much faster.

**This commit — the optimized fixpoint.** Win 1 of 2 over the faithful port: rework the
fixpoint matcher's internals — hash sets instead of ordered sets, a first-byte presence index
that skips patterns which can't match, single-pass replacement with double buffering, and
per-thread scratch buffers. Same fixpoint algorithm, same output, much less work per call. The
final commit replaces the fixpoint with the transducer. Like the faithful port, this predates
the coverage fixes in the final commit, so it reproduces the *original* Python's output.

## Performance

Full 823k-word build:

| version | cores | time |
|---|---|---|
| Python | 1 | ~7h |
| Python | 12 | ~36 min |
| Rust, faithful port | 12 | ~60s |
| **Rust, optimized fixpoint** | 12 | **~24s** |

~90× over the parallel Python on the same 12 cores. The final commit's transducer reaches ~6.1s.

## Algorithm

The generator's partition DP is unchanged from the faithful port: contiguous syllable groups
are matched once each instead of enumerating all 2^(n-1) join/slash combinations. What this
commit changes is inside `matcher.rs`:

1. **Hash sets** (`FxHashSet`) replace the ordered sets in the fixpoint loop.
2. **First-byte presence index** — a 256-bit set of the bytes a candidate contains, so
   patterns whose first byte is absent are skipped without a substring search.
3. **Single-pass replace** (`replace_into`) — Python `str.replace` semantics with double
   buffering, no allocation per substitution.
4. **Per-thread scratch** — candidate sets and buffers are reused across calls.

The matcher is still the Python's fixpoint, so the output is unchanged. The final commit
replaces it with a transducer (onset and coda reduce independently, no whole-word fixpoint)
for another ~4×.

Build, run, and validation instructions are in the final commit's README.
