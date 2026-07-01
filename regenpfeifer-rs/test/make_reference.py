"""Regenerate regen_reference.json from the Python generator in this repo.

Runs the repo's own (fixed) Python generator over the words in
regen_reference_words.txt, taking each word's type from the wortformliste,
and writes word -> [outlines]. Words are generated in list order with a
populated trie, matching the real build.

Usage: python3 test/make_reference.py <wortformliste.csv>
"""
import json
import os
import sys
from collections import OrderedDict

here = os.path.dirname(os.path.abspath(__file__))
repo_root = os.path.dirname(os.path.dirname(here))
sys.path.insert(0, repo_root)

from regenpfeifer.stroke_generator import StrokeGenerator  # noqa: E402

with open(os.path.join(here, "regen_reference_words.txt"), encoding="utf8") as f:
    reference_words = [line.strip() for line in f if line.strip()]

word_types = OrderedDict()
with open(sys.argv[1], encoding="utf8") as f:
    for line in f:
        parts = line.rstrip("\n").split(",")
        if len(parts) >= 2:
            word_types[parts[0]] = parts[1].split(" ")[0]

generator = StrokeGenerator([w for w in word_types if len(w) > 3])

reference = {}
for word in reference_words:
    reference[word] = generator.generate(word, word_types.get(word, "other"))

out_path = os.path.join(here, "regen_reference.json")
with open(out_path, "w", encoding="utf8") as f:
    json.dump(reference, f, ensure_ascii=False, indent=0, sort_keys=True)
print(f"wrote {len(reference)} words to {out_path}")
