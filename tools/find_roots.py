#!/usr/bin/env python3
"""Verify correct leaf offsets + locate SHOGO.REZ root block."""
import struct
GAME = "/home/a/Downloads/shogo-mobile-armor-division/Shogo Mobile Armor Division/"
snd = open(GAME + "SOUND.REZ", "rb").read()
sho = open(GAME + "SHOGO.REZ", "rb").read()

def records(d, start, size):
    end = min(start + size, len(d)); p = start; out = []
    while p + 16 <= end:
        t, o, s, tm = struct.unpack_from("<IIII", d, p); p += 16
        if t == 0:
            if p + 12 > end: break
            ext = d[p+4:p+12].split(b"\0")[0][::-1].decode("latin-1"); p += 12
            z = d.find(b"\0", p, min(p+260, end))
            if z == -1: break
            nm = d[p:z].decode("latin-1"); p = z + 1
            out.append((0, nm, ext, o, s))
        elif t == 1:
            z = d.find(b"\0", p, min(p+260, end))
            if z == -1: break
            nm = d[p:z].decode("latin-1"); p = z + 1
            out.append((1, nm, "", o, s))
        else: break
        if not (168 <= out[-1][3] and out[-1][3] + out[-1][4] <= len(d)):
            out.pop(); break
    return out

def block(d, off, n=96):
    return d[off:off+n]

# 1. Correct VOICE / PROPS offsets
print("VOICE child @140802059 first 96:")
b = block(snd, 140802059)
for i in range(0, len(b), 16):
    c = b[i:i+16]
    print("  +%03d %-47s %s" % (i, " ".join("%02x" % x for x in c),
          "".join(chr(x) if 32 <= x < 127 else "." for x in c)))
print("  parse ->", records(snd, 140802059, 29523)[:6])
print()

# 2. Find SHOGO.REZ root: slide over last 2MB, count multi-record clean blocks
print("== SHOGO.REZ root hunt (last 2MB) ==")
n = len(sho)
cands = []
for cand in range(n - 2*1024*1024, n - 16, 1):
    recs = records(sho, cand, n - cand)
    if len(recs) >= 3:
        # prefer blocks where entries are dirs (type 1) -> real directory
        ndir = sum(1 for r in recs if r[0] == 1)
        cands.append((cand, len(recs), ndir, [r[1] + ("." + r[2] if r[2] else "") for r in recs]))
# cluster: keep only starts where record count is maximal locally (true block starts)
best = []
seen_start = set()
for cand, cnt, ndir, names in cands:
    if cnt >= 4 and ndir >= 1:
        best.append((cand, cnt, ndir, names))
# dedupe overlapping (keep first/largest)
best.sort(key=lambda t: (-t[1], t[0]))
shown = []
for cand, cnt, ndir, names in best:
    if any(abs(cand - s) < 64 for s in shown): continue
    shown.append(cand)
    print("  @%d  %d recs (%d dirs): %s" % (cand, cnt, ndir, names[:8]))
    if len(shown) >= 8: break
