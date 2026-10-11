#!/usr/bin/env python3
"""Empirically map the ABC property-tree grammar by walking length-prefixed tags."""
import struct, sys, os

def rdstr(d, p):
    n = struct.unpack_from('<H', d, p)[0]
    return d[p+2:p+2+n].decode('latin-1'), p+2+n

def walk(d, p, end, depth, maxrec=60):
    rec = [0]
    while p + 2 <= end and rec[0] < maxrec:
        rec[0] += 1
        start = p
        try:
            name, p2 = rdstr(d, p)
        except Exception:
            break
        # is it printable?
        if not name or not all(32 <= ord(c) < 127 for c in name):
            print("  "*depth + "@%04d NONPRINT %r -- stop" % (start, name[:20]))
            break
        # peek what follows
        rest = d[p2:p2+16]
        print("  "*depth + "@%04d %-28s | %s" % (start, repr(name), ' '.join('%02x'%b for b in rest)))
        # Heuristic: recurse if next u16 is a small printable-string length
        if p2+2 <= end:
            nxt = struct.unpack_from('<H', d, p2)[0]
            if 1 <= nxt <= 40 and p2+2+nxt <= end:
                nxts = d[p2+2:p2+2+nxt]
                if all(32 <= b < 127 for b in nxts):
                    p = p2
                    depth += 1
                    continue
        p = p2 + 16  # skip a blob
    return p

f = sys.argv[1]
d = open(f, 'rb').read()
print("=== %s (%d bytes) ===" % (os.path.basename(f), len(d)))
walk(d, 0, len(d), 0)
