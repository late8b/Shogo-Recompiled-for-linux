#!/usr/bin/env python3
"""Map the named sections in a LithTech .DAT world after the object list.

Strategy: the region from ~37200 appears to be a series of named sub-sections
("LightObjects", a texture-name table, then geometry). We scan for u16-length-
prefixed ASCII names and u32 counts to reconstruct the section layout, without
assuming a specific value grammar yet.
"""
import struct, sys, os

def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def u16(d,o): return struct.unpack_from('<H',d,o)[0]

def main(path, region_start):
    d=open(path,'rb').read()
    n=len(d)
    print("=== %s (%d bytes), scanning from %d ==="%(os.path.basename(path),n,region_start))
    # Find all u16-length-prefixed printable names in the region.
    o=region_start
    hits=[]
    while o+2<n:
        ln=u16(d,o)
        if 3<=ln<=80 and o+2+ln<=n:
            s=d[o+2:o+2+ln]
            if all(32<=b<127 for b in s):
                name=s.decode('latin-1')
                # candidate: check it looks like an identifier/path
                if name.replace('_','').replace('\\','').replace('/','').replace('.','').replace('-','').isalnum():
                    hits.append((o,ln,name))
                    o+=2+ln
                    continue
        o+=1
    # Print hits with the u32 before each (often a count or type)
    print("named strings (u16-prefixed) found: %d"%len(hits))
    for o,ln,name in hits[:60]:
        pre=u32(d,o-4) if o>=4 else -1
        print("  @%-7d len=%-3d %-40s (u32 before=%d)"%(o,ln,name[:40],pre))

if __name__=='__main__':
    path=sys.argv[1]
    start=int(sys.argv[2]) if len(sys.argv)>2 else 37000
    main(path,start)
