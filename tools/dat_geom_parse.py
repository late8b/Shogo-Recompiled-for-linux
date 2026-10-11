#!/usr/bin/env python3
"""Parse LithTech 1.0 .DAT world geometry using the decompiled loader (FUN_00418d80).

Header read sequence (vtable +8 = ReadFixed(len), +0xc = ReadString(maxlen)):
  after object list (== geooff):
    9x u32
    string (world/level name, null-terminated, max 100)
    15x u32   (some are counts; some go to local stack)
    3x 12B vectors (bbox min, bbox max, extra)
    ... texture count u32, then texture table (NUL-terminated .dtx names)
  then array sections driven by counts.

We validate by checking the texture count == actual table length and the
texture table lands at the expected offset.
"""
import struct, sys, os

def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def u16(d,o): return struct.unpack_from('<H',d,o)[0]
def f32(d,o): return struct.unpack_from('<f',d,o)[0]

class R:
    def __init__(s,d,o): s.d=d; s.o=o
    def fixed(s,n):
        v=s.d[s.o:s.o+n]; s.o+=n; return v
    def u32(s): v=u32(s.d,s.o); s.o+=4; return v
    def u16(s): v=u16(s.d,s.o); s.o+=2; return v
    def string(s):
        e=s.d.find(b'\0',s.o)
        v=s.d[s.o:e].decode('latin-1'); s.o=e+1; return v

def main(path, verbose=True):
    d=open(path,'rb').read(); n=len(d)
    geooff=u32(d,8)
    r=R(d,geooff)
    if verbose: print("=== %s  geooff=%d ==="%(os.path.basename(path),geooff))
    # 9x u32
    hdr9=[r.u32() for _ in range(9)]
    name=r.string()
    # 15x u32
    hdr15=[r.u32() for _ in range(15)]
    # 3x 12B vectors
    def vec(): return (r.u32(),r.u32(),r.u32())  # read 12 raw bytes as 3 u32 (floats)
    bbox_min=vec(); bbox_max=vec(); extra=vec()
    # next: 2 more u32 (from loader 13077-13081) then texture count
    after1=r.u32()
    texoff=r.o
    texcount=r.u32()
    # read texture table
    texs=[]
    for i in range(texcount):
        texs.append(r.string())
    table_end=r.o
    if verbose:
        print("hdr9 =",hdr9)
        print("name = %r"%name)
        print("hdr15=",hdr15)
        print("bbox_min floats = %s"%(tuple(round(f32(d,geooff+36+len(name)+1+i*4),1) for i in range(3)),))
        print("texcount=%d  table @%d..%d"%(texcount,texoff,table_end))
        print("textures[0:5] =",texs[:5])
        print("after the 'after1' u32 = %d"%after1)
        print("next offset after table = %d, next u32s = %s"%(r.o,[u32(d,r.o+i*4) for i in range(6)]))
    return d,r,geooff,texcount,texs

if __name__=='__main__':
    main(sys.argv[1])
