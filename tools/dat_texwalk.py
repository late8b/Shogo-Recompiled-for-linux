#!/usr/bin/env python3
"""Walk the .DAT texture-name table and probe the geometry that follows."""
import struct, sys, os
def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def u16(d,o): return struct.unpack_from('<H',d,o)[0]
def f32(d,o): return struct.unpack_from('<f',d,o)[0]

def main(path, tex_start):
    d=open(path,'rb').read(); n=len(d)
    print("=== %s ==="%os.path.basename(path))
    # walk texture table as u32-length-prefixed strings
    o=tex_start; names=[]
    # the u32 before the first entry might be a count
    print("u32@%d=%d u32@%d=%d u32@%d=%d"%(tex_start-8,u32(d,tex_start-8),tex_start-4,u32(d,tex_start-4),tex_start,u32(d,tex_start)))
    while o+4<=n and len(names)<500:
        ln=u32(d,o)
        if ln==0 or ln>200: break
        s=d[o+4:o+4+ln]
        if not all(32<=b<127 or b==0 for b in s): break
        names.append(s.rstrip(b'\0').decode('latin-1'))
        o+=4+ln
    print("walked %d texture names, table ends @%d"%(len(names),o))
    for nm in names[:8]: print("   ",nm)
    # dump what follows the table
    print("\nbytes after texture table @%d:"%o)
    for j in range(o,o+96,16):
        c=d[j:j+16]
        print("  %06d %-47s %s"%(j,' '.join('%02x'%b for b in c),''.join(chr(b) if 32<=b<127 else '.' for b in c)))
    # interpret following as u32 counts then floats
    print("\nas u32 after table:", [u32(d,o+i*4) for i in range(12)])
    print("as f32 after table:", ['%.2f'%f32(d,o+i*4) for i in range(12)])

if __name__=='__main__':
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv)>2 else 37368)
