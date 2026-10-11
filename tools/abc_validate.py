#!/usr/bin/env python3
"""Validate the ABC parser across every model in SHOGO.REZ extraction."""
import glob, os, sys, traceback
sys.path.insert(0, os.path.dirname(__file__))
import ltabc as abc

files = sorted(glob.glob("/home/a/projects/shogo-mad/extracted/SHOGO/**/*.ABC", recursive=True))
ok = fail = empty = 0
fails = []
stats = []
for f in files:
    try:
        m = abc.load(f)
        if m.tri_count == 0:
            empty += 1
            fails.append((f, "0 tris"))
            continue
        # sanity: all positions inside (padded) bbox; UVs finite
        bad = False
        if m.bbox:
            mn = m.bbox[:3]; mx = m.bbox[3:]
            for (x, y, z) in m.positions:
                for v, lo, hi in ((x, mn[0], mx[0]), (y, mn[1], mx[1]), (z, mn[2], mx[2])):
                    if v < lo - 0.51 or v > hi + 0.51:
                        bad = True; break
                if bad: break
        for (u, v) in m.uvs:
            if not (-1e6 < u < 1e6) or not (-1e6 < v < 1e6):
                bad = True; break
        if bad:
            fail += 1
            fails.append((f, "out-of-bbox/NaN"))
        else:
            ok += 1
            stats.append((m.tri_count, os.path.basename(f)))
    except Exception as e:
        fail += 1
        fails.append((f, "%s: %s" % (type(e).__name__, e)))

print("Total ABC: %d   OK: %d   FAIL: %d   EMPTY: %d" % (len(files), ok, fail, empty))
stats.sort()
print("\nSmallest 5:", [(n, t) for t, n in stats[:5]])
print("Largest 5 :", [(n, t) for t, n in stats[-5:]])
if fails:
    print("\nFailures (first 15):")
    for f, msg in fails[:15]:
        print("  %-50s %s" % (os.path.relpath(f, "/home/a/projects/shogo-mad/extracted/SHOGO"), msg))
