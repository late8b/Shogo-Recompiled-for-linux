#!/usr/bin/env python3
"""Decode ABC Geometry section layout from raw bytes (find float/int arrays)."""
import struct, sys, os

def analyze(path):
    d = open(path,'rb').read()
    print("=== %s (%d bytes) ===" % (os.path.basename(path), len(d)))
    # locate 'Geometry'
    gi = d.find(b'Geometry')
    if gi < 0:
        print("  no Geometry"); return
    # name len prefix is 2 bytes before
    p = gi + 8
    marker = struct.unpack_from('<I', d, p)[0]; p += 4
    print("  Geometry @%d, u32 marker=%d (0x%x)" % (gi, marker, marker))
    # dump next 40 u32 as mixed float/int interpretation
    print("  %-6s %-12s %-12s %-14s %s" % ("off","u32","as_float","as_u32","as_i16[2]"))
    for k in range(0, 60):
        o = p + k*4
        if o+4 > len(d): break
        u = struct.unpack_from('<I', d, o)[0]
        f = struct.unpack_from('<f', d, o)[0]
        i = struct.unpack_from('<i', d, o)[0]
        h = struct.unpack_from('<hh', d, o)
        fs = ("%.5f"%f) if abs(f) < 1e6 and f!=0 else ""
        print("  @%-4d %-12d %-12s %-14d %s" % (o, u, fs, i, h))
    print()

for f in sys.argv[1:]:
    analyze(f)
