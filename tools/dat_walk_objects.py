#!/usr/bin/env python3
"""Walk the .DAT object list to find its exact end (= geometry start).

Object grammar (merged from server.dll loader + empirical):
  per object:
    u16 objDataSize      # total bytes of object data AFTER this field
    u16 nameLen, name    # class name
    u32 nProps
    per prop:
      u16 nameLen, name
      u8  type
      [4 bytes]
      u16 valueLen
      value[valueLen]
"""
import struct, sys, os

def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def u16(d,o): return struct.unpack_from('<H',d,o)[0]

def main(path):
    d=open(path,'rb').read(); n=len(d)
    print("=== %s (%d bytes) ==="%(os.path.basename(path),n))
    magic, objoff, geooff, txtlen = [u32(d,i*4) for i in range(4)]
    print("header: magic=%d objects@%d field2@%d textlen=%d"%(magic,objoff,geooff,txtlen))
    o=objoff
    nobj=u32(d,o); o+=4
    print("nObjects=%d, object list starts @%d"%(nobj,o))
    for i in range(nobj):
        start=o
        objsz=u16(d,o); o+=2
        datastart=o                     # objsz counts bytes from here
        nlen=u16(d,o); o+=2
        cls=d[o:o+nlen].decode('latin-1','replace'); o+=nlen
        if o+4>n: print("  !! EOF in object %d"%i); break
        nprops=u32(d,o); o+=4
        # skip properties
        ok=True
        for p in range(nprops):
            if o+2>n: ok=False; break
            plen=u16(d,o); o+=2
            pname=d[o:o+plen]; o+=plen
            if o+1+4+2>n: ok=False; break
            ptype=d[o]; o+=1
            o+=4
            vlen=u16(d,o); o+=2
            o+=vlen
        consumed = o-datastart
        flag = "" if consumed==objsz else "  <-- SIZE MISMATCH (consumed %d vs %d)"%(consumed,objsz)
        if i<6 or not ok or (consumed!=objsz):
            print("  obj%-3d @%-7d sz=%-5d cls=%-22s props=%-3d consumed=%-5d%s"%(
                i,start,objsz,cls,nprops,consumed,flag))
        # enforce size (seek to datastart+objsz) — matches loader skip
        o = datastart+objsz
    end=o
    print("object list ends @%d  (geooff field said %d, delta %d)"%(end,geooff,end-geooff))
    print("next 48 bytes @%d:"%end)
    for j in range(end,end+48,16):
        c=d[j:j+16]
        print("  %06d %-47s %s"%(j,' '.join('%02x'%b for b in c),''.join(chr(b) if 32<=b<127 else '.' for b in c)))

if __name__=='__main__':
    main(sys.argv[1])
