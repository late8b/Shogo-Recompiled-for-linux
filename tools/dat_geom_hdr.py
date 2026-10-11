#!/usr/bin/env python3
"""Defensive header walk of .DAT geometry (FUN_00418d80). Prints fields, never OOMs."""
import struct, sys
def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def f32(d,o): return struct.unpack_from('<f',d,o)[0]
def main(path):
    d=open(path,'rb').read(); n=len(d)
    geo=u32(d,8)
    print("geooff=%d filesize=%d"%(geo,n))
    o=geo
    def r4():
        nonlocal o; v=u32(d,o); o+=4; return v
    # reads 1-9 (each 4 bytes) per loader lines 12966-13001
    f=[r4() for _ in range(9)]
    print("reads1-9 (u32):", f)
    # string (world name) max 100
    e=d.find(b'\0',o); name=d[o:e].decode('latin-1'); o=e+1
    print("name=%r (len %d) o now %d"%(name,len(name),o))
    # reads 11-22 (12 more u32)
    g=[r4() for _ in range(12)]
    print("reads11-22 (u32):", g)
    # 3 vectors (12 bytes each)
    def vec():
        nonlocal o; v=tuple(round(f32(d,o+i*4),1) for i in range(3)); o+=12; return v
    print("bbox_min=",vec()); print("bbox_max=",vec()); print("vec3=",vec())
    # 2 more u32 (lines 13077-13081): first->local, second = texture count
    a=r4(); texcount=r4()
    print("after-vec u32=%d, texcount=%d (o=%d)"%(a,texcount,o))
    if texcount>10000:
        print("!! texcount implausible; header walk misaligned. Dumping raw u32s from geooff:")
        for i in range(40): print("  @%d=%d"%(geo+i*4,u32(d,geo+i*4)))
        return
    # read texture table
    texs=[]
    for _ in range(texcount):
        e=d.find(b'\0',o); texs.append(d[o:e].decode('latin-1')); o=e+1
    print("table @%d..%d (%d textures)"%(o-sum(len(t)+1 for t in texs),o,len(texs)))
    print("first tex:",texs[:3])
    print("position after table:",o)
    print("next u32s:",[u32(d,o+i*4) for i in range(10)])
if __name__=='__main__':
    main(sys.argv[1])
