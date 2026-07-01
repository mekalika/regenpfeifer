import json, sys, subprocess

ref = json.load(open('/tmp/regen_reference.json'))
# dump file: word\t[outlines-json] per line
dumped = {}
for line in open(sys.argv[1]):
    line = line.rstrip('\n')
    if '\t' not in line: continue
    w, j = line.split('\t', 1)
    try:
        dumped[w] = json.loads(j)
    except Exception:
        dumped[w] = []

total = 0
shortest_match = 0
missing = []
for w, ref_outs in ref.items():
    total += 1
    d = dumped.get(w, [])
    # shortest-outline match: shortest outline (by len, then lexicographic) must agree
    def shortest(outs):
        if not outs: return None
        return min(outs, key=lambda o: (len(o), o))
    rs = shortest(ref_outs)
    ds = shortest(d)
    if rs == ds:
        shortest_match += 1
    else:
        if len(missing) < 15:
            missing.append((w, rs, ds))

rate = 100.0 * shortest_match / max(total,1)
print(f"shortest-outline match rate: {shortest_match}/{total} = {rate:.3f}%")
if rate < 100.0:
    print("First mismatches (word, ref_shortest, got_shortest):")
    for w, rs, ds in missing:
        print(f"  {w!r}: ref={rs!r} got={ds!r}")
