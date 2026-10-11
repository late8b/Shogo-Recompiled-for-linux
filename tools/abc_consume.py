#!/usr/bin/env python3
"""Rigorous ABC validation: exact byte consumption + index-range, per model."""
import glob, os, sys, struct
sys.path.insert(0, os.path.dirname(__file__))
import ltabc

files = sorted(glob.glob("/home/a/projects/shogo-mad/extracted/SHOGO/**/*.ABC", recursive=True))
ok = 0
problems = {}
consume_fail = []
for f in files:
    d = open(f, 'rb').read()
    props = {n: (ds, de) for n, ds, de in ltabc._walk(d, 0, len(d))}
    if 'Geometry' not in props:
        problems.setdefault('no-Geometry', []).append(f); continue
    ds, de = props['Geometry']
    o = ds
    o += 24
    nNames = struct.unpack_from('<I', d, o)[0]; o += 4
    o += nNames * 2 + 2
    nFaces = struct.unpack_from('<I', d, o)[0]; o += 4
    o += nFaces * 33
    nVerts = struct.unpack_from('<I', d, o)[0]; o += 4
    o += 4
    o += nVerts * 20
    if o != de:
        consume_fail.append((f, "consumed %d expected %d (nF=%d nV=%d)" % (o - ds, de - ds, nFaces, nVerts)))
        continue
    # index range
    m = ltabc.load(f)
    if m.indices and max(m.indices) >= len(m.positions):
        problems.setdefault('index-OOB', []).append(f); continue
    ok += 1

print("Total: %d   exact-consumption+OK: %d" % (len(files), ok))
print("\nConsumption mismatches: %d" % len(consume_fail))
for f, msg in consume_fail[:12]:
    print("  %-48s %s" % (os.path.relpath(f, "/home/a/projects/shogo-mad/extracted/SHOGO"), msg))
for k, v in problems.items():
    print("\n%s: %d" % (k, len(v)))
    for f in v[:6]:
        print("  ", os.path.relpath(f, "/home/a/projects/shogo-mad/extracted/SHOGO"))
