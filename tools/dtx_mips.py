#!/usr/bin/env python3
"""Brute-force DTX pixel-data mip layout. header=44, palette=1024 @44."""
import struct, glob, os
from collections import Counter

EX = "/home/a/projects/shogo-mad/extracted/SHOGO/"
HDR, PAL = 44, 1024

def dims_mips(w, h, n):
    out = []
    cw, ch = w, h
    for i in range(n):
        out.append((max(1, cw), max(1, ch)))
        cw = max(1, cw // 2); ch = max(1, ch // 2)
    return out

files = sorted(glob.glob(EX+"TEXTURES/**/*.DTX", recursive=True))
print("Total DTX:", len(files))

# For each file, find which mip-count (1..16) makes pixel bytes fit exactly.
fit = Counter()
mip_by_file = {}
no_fit = 0
for f in files:
    d = open(f,'rb').read()
    w, h = struct.unpack_from('<HH', d, 8)
    if w == 0 or h == 0:
        no_fit += 1; continue
    px = len(d) - HDR - PAL
    matched = None
    for n in range(1, 17):
        tot = sum(a*b for a,b in dims_mips(w,h,n))
        if tot == px:
            matched = n; break
    if matched is None:
        no_fit += 1
    else:
        fit[matched] += 1
        mip_by_file[f] = matched

print("Mip-count distribution (exact fits):")
for n,c in sorted(fit.items()):
    print("  %2d mips : %d files"%(n,c))
print("No exact fit (8bpp):", no_fit)

# For files that didn't fit 8bpp, test 16bpp (2 bytes/px, no palette) and DXT1
rest = [f for f in files if f not in mip_by_file]
print("\nTesting %d non-8bpp files for 16bpp / DXT1 / DXT3 / raw-rgba..."%len(rest))
alt = Counter()
for f in rest:
    d = open(f,'rb').read()
    w,h = struct.unpack_from('<HH', d, 8)
    if w==0 or h==0: alt['bad-dims']+=1; continue
    px16 = len(d) - HDR           # 16bpp, no palette
    pxrgb = len(d) - HDR          # 
    found=None
    for n in range(1,17):
        m8 = sum(a*b for a,b in dims_mips(w,h,n))
        if m8*2 == px16: found='16bpp-%d'%n; break
        if m8*4 == px16: found='rgba-%d'%n; break
    if not found:
        # DXT1 (8 bytes/4x4 block)
        for n in range(1,17):
            blk = sum(max(1,(a+3)//4)*max(1,(b+3)//4)*8 for a,b in dims_mips(w,h,n))
            if blk == len(d)-HDR: found='dxt1-%d'%n; break
    alt[found or 'none'] += 1
for k,c in alt.most_common():
    print("  %-12s %d"%(k,c))
