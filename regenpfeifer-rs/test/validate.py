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
exact = 0
shortest_match = 0
mismatches = []
for word, ref_outlines in ref.items():
    total += 1
    got = dumped.get(word, [])
    if got == ref_outlines:
        exact += 1
    if shortest(got) == shortest(ref_outlines):
        shortest_match += 1
    elif len(mismatches) < 15:
        mismatches.append((word, shortest(ref_outlines), shortest(got)))

print(f"exact outline-list match:    {exact}/{total} = {100.0 * exact / total:.3f}%")
print(
    f"shortest-outline match:      {shortest_match}/{total}"
    f" = {100.0 * shortest_match / total:.3f}%"
)
if mismatches:
    print("First mismatches (word, ref_shortest, got_shortest):")
    for word, ref_short, got_short in mismatches:
        print(f"  {word!r}: ref={ref_short!r} got={got_short!r}")
sys.exit(0 if shortest_match == total else 1)
