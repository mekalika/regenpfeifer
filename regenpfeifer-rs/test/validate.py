"""Diff a regen-rs dump against the Python-generated reference.

The reference (regen_reference.json, word -> [outlines]) is the fixed Python
generator's output for the 3000 words in regen_reference_words.txt; regenerate
it with test/make_reference.py whenever the patterns change. A 100% match is
expected.

Usage: python3 test/validate.py <dump.tsv>   (dump: word\\t[outlines-json] lines,
produced by running regen-rs with REGEN_DUMP=test/regen_reference_words.txt)
"""
import json
import os
import sys


def shortest(outlines):
    if not outlines:
        return None
    return min(outlines, key=lambda o: (len(o), o))


here = os.path.dirname(os.path.abspath(__file__))
with open(os.path.join(here, "regen_reference.json"), encoding="utf8") as f:
    ref = json.load(f)

dumped = {}
with open(sys.argv[1], encoding="utf8") as f:
    for line in f:
        line = line.rstrip("\n")
        if "\t" not in line:
            continue
        word, outlines_json = line.split("\t", 1)
        try:
            dumped[word] = json.loads(outlines_json)
        except ValueError:
            dumped[word] = []

total = 0
set_match = 0
shortest_match = 0
mismatches = []
for word, ref_outlines in ref.items():
    total += 1
    got = dumped.get(word, [])
    # Order-insensitive: the Python's alternate ordering varies with hash seed,
    # and a word's own outline order cannot change the assembled dictionary.
    if set(got) == set(ref_outlines):
        set_match += 1
    elif len(mismatches) < 15:
        only_ref = sorted(set(ref_outlines) - set(got))
        only_got = sorted(set(got) - set(ref_outlines))
        mismatches.append((word, only_ref, only_got))
    if shortest(got) == shortest(ref_outlines):
        shortest_match += 1

print(f"outline-set match:      {set_match}/{total} = {100.0 * set_match / total:.3f}%")
print(
    f"shortest-outline match: {shortest_match}/{total}"
    f" = {100.0 * shortest_match / total:.3f}%"
)
if mismatches:
    print("First mismatches (word, only-in-reference, only-in-dump):")
    for word, only_ref, only_got in mismatches:
        print(f"  {word!r}: ref-only={only_ref[:4]} dump-only={only_got[:4]}")
sys.exit(0 if set_match == total else 1)
