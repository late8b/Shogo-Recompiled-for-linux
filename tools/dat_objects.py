#!/usr/bin/env python3
"""Decode the DEdit world object/property grammar precisely.

Object @objects_offset+4:
    u16 type_id
    u16 nameLen, char name[nameLen]
    u32 nProps
    per prop:
        u16 nameLen, char name[nameLen]
        <typed value>

We dump the raw bytes of the first 2 objects and try to identify the value
type tag so we can walk the list to its end and locate the geometry section.
"""
import struct, sys, os

def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def u16(d,o): return struct.unpack_from('<H',d,o)[0]
def i32(d,o): return struct.unpack_from('<i',d,o)[0]
def f32(d,o): return struct.unpack_from('<f',d,o)[0]

def hexdump(d,o,n,label):
    print("  %s @%d:" % (label,o))
    for j in range(o,o+n,16):
        c=d[j:j+16]
        print("    %04d %-47s %s"%(j,' '.join('%02x'%b for b in c),
              ''.join(chr(b) if 32<=b<127 else '.' for b in c)))

def main(path):
    d=open(path,'rb').read()
    obj_off=u32(d,4)
    n=u32(d,obj_off)
    print("=== %s: objects @%d, n=%d ==="%(os.path.basename(path),obj_off,n))
    o=obj_off+4
    # walk first 3 objects
    for k in range(3):
        if o+6>len(d): break
        tid=u16(d,o)
        nlen=u16(d,o+2)
        name=d[o+4:o+4+nlen].decode('latin-1')
        p=o+4+nlen
        nprops=u32(d,p); p+=4
        print("\nobj%d @%d: type=%d name=%r nprops=%d"%(k,o,tid,name,nprops))
        # walk props
        for pi in range(nprops):
            if p+2>len(d): break
            plen=u16(d,p)
            pname=d[p+2:p+2+plen].decode('latin-1','replace')
            vp=p+2+plen
            print("    prop%d @%d: %r  next bytes: %s"%(pi,p,pname,
                  ' '.join('%02x'%b for b in d[vp:vp+20])))
            # heuristic value walk: after name there seems to be a type/flags region
            # then the value. We just stop after first prop to inspect raw.
            if pi==0:
                hexdump(d,p,40,"first prop raw")
            break  # only show first prop detail; stop prop walk
        # advance to next object by scanning: we can't yet; break
        break

if __name__=='__main__':
    main(sys.argv[1])
