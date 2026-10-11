#!/usr/bin/env python3
"""Confirm DTX uses @12 as mip count; decode coverage; inspect failures."""
import struct, glob, os
from collections import Counter

EX = "/home/a/projects/shogo-mad/extracted/SHOGO/"
HDR, PAL = 44, 1024

def mip_dims(w, h, n):
    out=[]; cw,ch=w,h
    for _ in range(n):
        out.append((max(1,cw),max(1,ch)))
        cw=max(1,cw//2); ch=max(1,ch//2)
    return out

files = sorted(glob.glob(EX+"TEXTURES/**/*.DTX", recursive=True))
ok=0; fail=[]; mipcount_hist=Counter()
for f in files:
    d=open(f,'rb').read()
    w,h = struct.unpack_from('<HH', d, 8)
    n   = struct.unpack_from('<H', d, 12)[0]
    mipcount_hist[n]+=1
    px = len(d)-HDR-PAL
    if w==0 or h==0 or n==0 or n>16:
        fail.append((f,'baddims',w,h,n)); continue
    tot = sum(a*b for a,b in mip_dims(w,h,n))
    if tot==px: ok+=1
    else: fail.append((f,'sizemismatch',w,h,n,px,tot))

print("Total:",len(files))
print("Decoded OK (using @12 as mipcount):", ok)
print("@12 mipcount histogram:", dict(mipcount_hist.most_common(8)))
print("Failures:", len(fail))
for fr in fail[:12]:
    print("  ", os.path.relpath(fr[0],EX), fr[1:])
