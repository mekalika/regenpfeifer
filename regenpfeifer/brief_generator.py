"""Theory-aware brief generator for Regenpfeifer.

Mints collision-free, single-stroke briefs for the most frequent words that
Regenpfeifer still writes in two or more strokes. It treats briefing as a global
assignment problem instead of per-word truncation, which is what makes the output
usable. Two ideas:

  1. FOLD, don't truncate. A brief keeps the word's first stroke *and* its trailing
     consonant(s), so it stays recognizable front-and-back
     (zurueck ZU/ROUBG -> ZUBG, etwas ET/WAS -> ETS) instead of collapsing to a
     prefix that a dozen words share.

  2. RESOLVE collisions globally, in frequency order. The most frequent word that
     wants a chord gets the leanest one free; any later word that would collide
     escalates to a more disambiguated fold, or is skipped. No two briefs ever
     clash, and none clash with an existing dictionary entry. This is the human
     "who gets the clean chord" judgement, done as a greedy assignment over the
     whole dictionary.

The result is CANDIDATES: collision-free and valid steno by construction, but a
human should still eyeball them for taste (a fold can drop a medial consonant, e.g.
Leben HRE/PWEPB -> HREPB). Briefs are opt-in -- Regenpfeifer is a brief-light,
write-what-you-hear theory by design; this just produces a clean starting set for
anyone who wants one, in whatever quantity they choose.

Usage:
    python -m regenpfeifer.brief_generator <dictionary.json> <frequency.txt> <out.json> [--max N] [--reserve d.json ...]

  dictionary.json  a generated Regenpfeifer dictionary (outline -> word)
  frequency.txt    "word<whitespace>count" per line (e.g. a hermitdave de_50k list);
                   determines priority -- common words win the lean chords
  out.json         output briefs (stroke -> word)
  --max N          optional cap on how many briefs to emit (order-independent; a
                   bare positional N before --reserve still works too)
  --reserve        extra dictionaries (suffixes, fingerspelling, commands, ...) whose
                   strokes are off-limits, so briefs never shadow the grammar layer
"""
import json
import sys

from plover_stroke import BaseStroke

# Regenpfeifer key layout -- mirrors plover_regenpfeifer/system.py.
KEYS = (
    "#",
    "Z-", "S-", "T-", "K-", "P-", "W-", "H-", "R-",
    "A-", "O-",
    "*",
    "-E", "-U",
    "-F", "-R", "-P", "-B", "-L", "-G", "-T", "-S", "-D", "-Z",
)
IMPLICIT_HYPHEN_KEYS = ("A-", "O-", "-E", "-U", "*")
NUMBER_KEY = "#"
NUMBERS = {
    "S-": "1-", "T-": "2-", "P-": "3-", "H-": "4-", "A-": "5-",
    "O-": "0-", "-F": "-6", "-P": "-7", "-L": "-8", "-T": "-9",
}
RIGHT_BANK = frozenset({"-F", "-R", "-P", "-B", "-L", "-G", "-T", "-S", "-D", "-Z"})

# Brief length policy. A brief must stay comfortable. Lean briefs (<= LEAN_BRIEF_KEYS)
# are always allowed; a fat fold (up to MAX_BRIEF_KEYS) only earns its keep when it
# collapses three or more strokes into one (e.g. Geschichte TKPWE/SHEUFP/TE ->
# TKPWEFP). A 7-key chord that saves a single stroke is not worth it; that word keeps
# its normal outline.
LEAN_BRIEF_KEYS = 6
MAX_BRIEF_KEYS = 8

# Vulgar / offensive words are kept out of the easy-brief layer. They stay fully
# writable in the main dictionary -- they just shouldn't be a two-key accident.
DEFAULT_STOPLIST = frozenset({
    "scheiße", "scheisse", "verdammt", "verdammte", "verdammten", "verdammter",
    "verdammtes", "arsch", "arschloch", "arschlöcher", "ficken", "fickt", "fick",
    "fotze", "hure", "huren", "schlampe", "wichser", "nutte", "kacke", "pisse",
    "töten", "tötet", "töte", "getötet", "tötete", "teufel", "teufels",
})


class _Stroke(BaseStroke):
    pass


_Stroke.setup(KEYS, IMPLICIT_HYPHEN_KEYS, NUMBER_KEY, NUMBERS)


def _keys(steno):
    return set(_Stroke.from_steno(steno).keys())


def _steno(key_set):
    """Canonical single-stroke steno for a set of keys, or None if not reachable."""
    try:
        return str(_Stroke.from_keys(tuple(key_set)))
    except ValueError:
        # plover_stroke raises ValueError for an unreachable key set
        return None


def candidates(outline):
    """Single-stroke brief candidates for a multi-stroke outline, leanest first.

    head            = the first stroke (shortest; the frequency winner takes it)
    head+last_right = first stroke + the final stroke's trailing consonants
    head+tail_right = first stroke + every trailing consonant (most disambiguating)
    """
    strokes = outline.split("/")
    head = _keys(strokes[0])
    last_right = _keys(strokes[-1]) & RIGHT_BANK
    tail_right = set().union(*(_keys(s) & RIGHT_BANK for s in strokes[1:]))

    out = []
    seen = set()
    for key_set in (head, head | last_right, head | tail_right):
        if key_set <= {"#", "*"}:
            continue  # never emit the undo (*) or number (#) stroke as a brief
        steno = _steno(key_set)
        if steno and steno not in seen:
            seen.add(steno)
            out.append((len(key_set), steno))
    out.sort(key=lambda pair: pair[0])  # leanest (most ergonomic) first
    return out  # (key_count, steno) pairs; caller applies the length policy


def generate(dictionary_path, frequency_path, output_path, max_briefs=None, reserve_paths=()):
    with open(dictionary_path, encoding="utf8") as f:
        dictionary = json.load(f)

    # shortest current outline per word (match the lowercased frequency list,
    # but keep the dictionary's cased form so noun capitals survive)
    by_word = {}
    for outline, word in dictionary.items():
        strokes = outline.count("/") + 1
        key = word.lower()
        if key not in by_word or strokes < by_word[key][1]:
            by_word[key] = (outline, strokes, word)

    occupied = set(dictionary)  # every existing outline is taken
    for path in reserve_paths:  # ...and every stroke in the reserved dicts
        with open(path, encoding="utf8") as f:
            occupied |= set(json.load(f))
    briefs = {}
    folds = 0
    skipped = 0  # words dropped because their outline wouldn't parse
    with open(frequency_path, encoding="utf8") as f:
        for line in f:
            parts = line.split()
            if len(parts) < 2:
                continue
            word = parts[0].lower()
            if word in DEFAULT_STOPLIST:
                continue
            entry = by_word.get(word)
            if not entry:
                continue
            outline, strokes, cased = entry
            if strokes < 2:
                continue  # already one stroke -- nothing to brief
            try:
                options = candidates(outline)
            except ValueError:
                # plover_stroke couldn't parse a stroke in this outline; skip it
                skipped += 1
                continue
            for nkeys, brief in options:
                # lean briefs always; a fat fold only when it collapses 3+ strokes
                if nkeys > MAX_BRIEF_KEYS or (nkeys > LEAN_BRIEF_KEYS and strokes < 3):
                    continue
                if brief not in occupied:
                    briefs[brief] = cased
                    occupied.add(brief)
                    if brief != outline.split("/")[0]:
                        folds += 1
                    break
            if max_briefs and len(briefs) >= max_briefs:
                break

    with open(output_path, "w", encoding="utf8") as f:
        json.dump(dict(sorted(briefs.items())), f, ensure_ascii=False, indent=0)
    print(
        f"{len(briefs)} briefs -> {output_path} "
        f"({folds} folds, {len(briefs) - folds} bare first-strokes)"
    )
    if skipped:
        print(f"skipped {skipped} words (outline would not parse)")


if __name__ == "__main__":
    args = sys.argv[1:]
    cap = None
    if "--max" in args:  # order-independent; preferred over the positional form
        i = args.index("--max")
        cap = int(args[i + 1])
        del args[i:i + 2]
    reserve = []
    if "--reserve" in args:
        i = args.index("--reserve")
        reserve = args[i + 1:]
        args = args[:i]
    if len(args) < 3:
        sys.exit(__doc__)
    if cap is None and len(args) > 3:  # backward-compatible positional max_briefs
        cap = int(args[3])
    generate(args[0], args[1], args[2], cap, reserve)
