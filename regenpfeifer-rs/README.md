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

~200× over the parallel Python (same-day A/B on the same 12 cores: 27.6 min vs
7.8 s best-of-3; times vary with machine state, treat the table as ballpark).
Output, on the full 823,381-word wortformliste (upstream main after #4/#5): the
Python generator emits 897,130 entries; regen-rs reproduces 897,124 of them exactly.
The 6 missing are
redundant double-slash variants of the two slash-bearing word-list entries
(`der/die/dasjenige`, `der/die/dasselbige`) — each contains an empty stroke,
which Plover cannot write. 0 entries differ in value; 0 are Rust-only.

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

## Experiment on patterns (`--diff`)

The point of a seconds-fast build: edit a pattern file, rebuild, and see exactly what
changed — entry level, full corpus.

```sh
./target/release/regen-rs data/wortformliste.csv baseline.json assets            # once
# ... edit assets/patterns/right_patterns.json ...
./target/release/regen-rs data/wortformliste.csv new.json assets --diff baseline.json
# -> +N entries, -N entries, N reassigned | words newly writable: N, no longer writable: N
#    (with samples of each)
```

Point `assets` at the Python repo's `regenpfeifer/assets/` to evaluate a pattern PR
against the real theory in one command.

## Validate

```sh
REGEN_DUMP=test/regen_reference_words.txt \
  ./target/release/regen-rs data/wortformliste.csv /tmp/_d.json assets > /tmp/dump.tsv
python3 test/validate.py /tmp/dump.tsv
```

`test/regen_reference.json` is the fixed Python generator's output for the same 3000
words, regenerated in-repo with `python3 test/make_reference.py data/wortformliste.csv`.
A 100% outline-set match is expected (validate.py exits non-zero otherwise; sets, not
lists, because the Python's alternate ordering varies with the hash seed and intra-word
order cannot change the assembled dictionary).