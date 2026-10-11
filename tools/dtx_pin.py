#!/usr/bin/env python3
"""Pin DTX header size + confirm 8bpp-palettized full-mip-chain layout."""
import struct, glob, os

EX = "/home/a/projects/shogo-mad/extracted/SHOGO/"

def mip_sum8(w, h):
    s = 0
    while True:
        s += max(1, w) * max(1, h)
        if w == 1 and h == 1: break
        w = max(1, w // 2); h = max(1, h // 2)
    return s

def hdr(d):
    u32=lambda o:struct.unpack_from('<I',d,o)[0]
    u16=lambda o:struct.unpack_from('<H',d,o)[0]
    return u16(8), u16(10), u32(4), u32(16)

files = sorted(glob.glob(EX+"TEXTURES/**/*.DTX", recursive=True))
print("Total:", len(files))

# Hypothesis: size == H + 1024 (palette) + mip_sum8(w,h). Solve H per file; check constant.
from collections import Counter
Hcount = Counter()
for f in files:
    d = open(f,'rb').read()
    w,h,flags,f16 = hdr(d)
    if w==0 or h==0: 
        Hcount[('bad',w,h)] += 1
        continue
    H = len(d) - 1024 - mip_sum8(w,h)
    Hcount[H] += 1
print("Derived header size H (= filesize - 1024 - fullmip8):")
for H,c in Hcount.most_common(8):
    print("  H=%-6s : %d files"%(H,c))

# Also test WITHOUT mipmaps (single level) and WITH palette-header combined
H2 = Counter()
for f in files:
    d=open(f,'rb').read(); w,h,_,_=hdr(d)
    if w==0 or h==0: continue
    H2[len(d)-1024-w*h] += 1
print("If single-level (no mips):")
for H,c in H2.most_common(5):
    print("  H=%-6s : %d files"%(H,c))
