#!/usr/bin/env python3
"""Determine DTX pixel format by correlating file size vs dimensions."""
import struct, glob, os, sys

EX = "/home/a/projects/shogo-mad/extracted/SHOGO/"

def hdr(d):
    u32=lambda o:struct.unpack_from('<I',d,o)[0]
    u16=lambda o:struct.unpack_from('<H',d,o)[0]
    return dict(ver=u32(0), flags=u32(4), w=u16(8), h=u16(10),
                f12=u16(12), f14=u16(14), f16=u32(16), f20=u32(20))

def mip_sum8(w, h):
    s=0
    while True:
        s += max(1,w)*max(1,h)
        if w==1 and h==1: break
        w=max(1,w//2); h=max(1,h//2)
    return s

def mip_sum_dxt1(w,h):
    s=0
    while True:
        s += max(1,(w+3)//4)*max(1,(h+3)//4)*8
        if w==1 and h==1: break
        w=max(1,w//2); h=max(1,h//2)
    return s

def mip_sum16(w,h):
    s=0
    while True:
        s += max(1,w)*max(1,h)*2
        if w==1 and h==1: break
        w=max(1,w//2); h=max(1,h//2)
    return s

files = sorted(glob.glob(EX+"TEXTURES/**/*.DTX", recursive=True))
print("Total DTX:", len(files))
# For each hypothesis (header + palette + mipmap-rule), count exact size matches
hyps = {
    "8bpp+pal, hdr32, fullmip":   lambda w,h: 32 + 1024 + mip_sum8(w,h),
    "8bpp+pal, hdr36, fullmip":   lambda w,h: 36 + 1024 + mip_sum8(w,h),
    "8bpp+pal, hdr32, nomip":     lambda w,h: 32 + 1024 + w*h,
    "8bpp nopal, hdr32, fullmip": lambda w,h: 32 + mip_sum8(w,h),
    "16bpp, hdr32, fullmip":      lambda w,h: 32 + mip_sum16(w,h),
    "16bpp, hdr32, nomip":        lambda w,h: 32 + w*h*2,
    "DXT1, hdr32, fullmip":       lambda w,h: 32 + mip_sum_dxt1(w,h),
    "DXT1, hdr32, nomip":         lambda w,h: 32 + max(1,(w+3)//4)*max(1,(h+3)//4)*8,
}
# test on a sample
sample = files[:: max(1,len(files)//400)]
counts = {k:0 for k in hyps}
for f in sample:
    d=open(f,'rb').read(); H=hdr(d); w,h=H['w'],H['h']
    if w==0 or h==0: continue
    sz=len(d)
    for k,fn in hyps.items():
        if fn(w,h)==sz: counts[k]+=1
print("Tested %d files. Exact size matches per hypothesis:"%len(sample))
for k,v in sorted(counts.items(), key=lambda x:-x[1]):
    print("  %-30s %d"%(k,v))
