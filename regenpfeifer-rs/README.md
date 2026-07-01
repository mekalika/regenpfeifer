# regen-rs — a Rust port of the Regenpfeifer outline generator

A Rust reimplementation of [Regenpfeifer's](https://github.com/mkrnr/regenpfeifer) word→outline generator (the *generate* direction): the same output as the Python
generator, much faster.

## Performance

Full 823k-word build:

| version | cores | time |
|---|---|---|
| Python | 1 | ~7h |
| Python | 12 | ~36 min |
| Rust, faithful port | 12 | ~60s |
| Rust, optimized fixpoint | 12 | ~24s |
| **Rust, transducer (default)** | 12 | **~6.1s** |

~350× over the parallel Python on the same 12 cores. Output matches this repo's
Python generator (`main`, with the #1 coverage fixes), identical except for a
handful of pathological slash-bearing entries (`der/die/dasjenige`) where the
Python emits redundant slash-boundary variants.

## Algorithm

Faithful ports of the splitter (compound + Kirsch syllabifier), the emphasizer, and
the asterisk/markup utilities (including the `before_asterisk = "ZSTKPWHRAO"` Z-fix).
Two wins over the Python's generate-and-validate:

1. **`generator.rs` — partition DP.** Instead of enumerating all 2^(n-1) join/slash
   combinations and validating each, match each contiguous syllable group once.
2. **`matcher.rs` — transducer.** Onset and coda reduce independently (left patterns
   only touch the onset, right patterns only the coda), so a syllable transduces
   directly to `<onset-keys>[e|V]<coda-keys>` — no whole-word fixpoint and no separate
   validation pass, since steno order is structural. Reproduces the fixpoint's
   strict-valid output set exactly.

## Build & run

```sh
cargo build --release
./target/release/regen-rs data/wortformliste.csv out.json assets
# -> "<N> words -> <M> entries in <T>s", writes an outline->word JSON
```

The word list (`data/wortformliste.csv`) is the external input — the same
[mkrnr/wortformliste](https://github.com/mkrnr/wortformliste) the Python generator takes.

## Validate

```sh
REGEN_DUMP=test/regen_reference_words.txt \
  ./target/release/regen-rs data/wortformliste.csv /tmp/_d.json assets > /tmp/dump.tsv
python3 test/validate.py /tmp/dump.tsv
```

`test/regen_reference.json` is the fixed Python generator's output for the same 3000
words, regenerated in-repo with `python3 test/make_reference.py data/wortformliste.csv`.
A 100% shortest-outline match is expected (validate.py exits non-zero otherwise).