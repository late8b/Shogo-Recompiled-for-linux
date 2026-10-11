#!/usr/bin/env python3
"""
Probe the world geometry section of a LithTech .DAT.

We know:
  - header text, objects, then a NUL-terminated texture-name table
  - a large unnamed region (the geometry)
  - named surface sections near the end (Gird1Beam, Billboard, Sky, ...)

This walks the texture table as NUL-terminated strings to find its true end,
then dumps the following bytes and tries to interpret them as vertex/polygon
counts and arrays. We look for a count field followed by ~count*stride bytes of
vertex-like floats.
"""
import struct, sys, os
def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def i32(d,o): return struct.unpack_from('<i',d,o)[0]
def f32(d,o): return struct.unpack_from('<f',d,o)[0]

def main(path, tex_start):
    d=open(path,'rb').read(); n=len(d)
    print("=== %s (%d bytes) ==="%(os.path.basename(path),n))
    # Walk NUL-terminated strings from tex_start.
    o=tex_start; names=[]
    while o<n:
        e=d.find(b'\0',o)
        if e<0: break
        s=d[o:e]
        if not (3<len(s)<120 and all(32<=b<127 for b in s)): break
        names.append(s.decode('latin-1')); o=e+1
        if len(names)>600: break
    print("texture names: %d, table ends @%d (u32 before table = %d)"%(len(names),o,u32(d,tex_start-4)))
    # Look back for the count: the u32 just before the first name.
    # Dump the region after the table.
    print("\nraw after table @%d:"%o)
    for j in range(o,o+64,16):
        c=d[j:j+16]
        print("  %06d %-47s %s"%(j,' '.join('%02x'%b for b in c),''.join(chr(b) if 32<=b<127 else '.' for b in c)))
    # Try: after the texture table there may be a counts block. Print u32s.
    print("\nas u32:", [u32(d,o+i*4) for i in range(16)])
    # Scan forward for a plausible vertex block: a u32 count C where C floats
    # (or C*3) of vertex-like values follow within [±20000], mostly nonzero.
    import math
    def looks_like_vertices(off, count, stride_floats):
        end=off+count*stride_floats*4
        if end>n or count<8 or count>200000: return False
        nz=0; inrange=0
        for i in range(0,count*stride_floats,max(1,count*stride_floats//200)):
            v=f32(d,off+i*4)
            if not math.isfinite(v): return False
            if abs(v)>1e-6: nz+=1
            if -20000<v<20000: inrange+=1
        total=len(range(0,count*stride_floats,max(1,count*stride_floats//200)))
        return nz>total*0.5 and inrange>total*0.95
    print("\nsearching for vertex blocks after table...")
    for cand in range(o, min(o+4000,n-8), 4):
        c=u32(d,cand)
        for sf in (3,):  # 3 floats per vertex
            if looks_like_vertices(cand+4,c,sf):
                vals=[f32(d,cand+4+i*4) for i in range(9)]
                print("  @%d count=%d (%d floats) first: %s"%(cand,c,c*sf,' '.join('%.1f'%v for v in vals)))
                break

if __name__=='__main__':
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv)>2 else 37372)
