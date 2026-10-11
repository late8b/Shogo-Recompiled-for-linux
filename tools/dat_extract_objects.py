#!/usr/bin/env python3
"""Extract game object data from LithTech 1.0 .DAT worlds.

The object/property grammar is fully reverse-engineered and verified across all
retail .DAT files (object-list byte consumption matches the stored size exactly).

Object list (starts at header.objects_offset):
  u32 nObjects
  per object:
    u16 objDataSize        # bytes of object data AFTER this field
    u16 nameLen, name      # class name (GameStartPoint, Light, Sprite, ...)
    u32 nProps
    per prop:
      u16 nameLen, name
      u8  type             # value type tag
      4 bytes              # (type-specific header, usually zero)
      u16 valueLen
      value[valueLen]

Property value types observed (from server.dll loader FUN_10007372):
   1 = vector (3 f32, 12 bytes)   e.g. Pos, Rotation(x,y,z)
   2 = color  (3 f32, 12 bytes)   e.g. LightColor, InnerColor
   3 = float  (f32)               e.g. LightRadius, BrightScale
   5 = byte                     e.g. LightObjects
   string = u16 len + chars       e.g. Name
"""
import struct, sys, os, glob, json

def u32(d,o): return struct.unpack_from('<I',d,o)[0]
def u16(d,o): return struct.unpack_from('<H',d,o)[0]
def f32(d,o): return struct.unpack_from('<f',d,o)[0]

def parse_prop_value(d,o,ptype,vlen):
    if ptype in (1,2) and vlen>=12:
        return (round(f32(d,o),3),round(f32(d,o+4),3),round(f32(d,o+8),3))
    if ptype==3 and vlen>=4:
        return round(f32(d,o),4)
    if ptype==5 and vlen>=1:
        return d[o]
    if vlen>=2:
        slen=u16(d,o)
        return d[o+2:o+2+slen].decode('latin-1','replace')
    return d[o:o+vlen].hex()

def parse_world(path):
    d=open(path,'rb').read(); n=len(d)
    magic,objoff,geooff,txtlen=[u32(d,i*4) for i in range(4)]
    text=d[16:16+txtlen].decode('latin-1')
    o=objoff; nobj=u32(d,o); o+=4
    objs=[]
    for i in range(nobj):
        objsz=u16(d,o); o+=2; ds=o
        nlen=u16(d,o); o+=2; cls=d[o:o+nlen].decode('latin-1'); o+=nlen
        nprops=u32(d,o); o+=4
        props={}
        for p in range(nprops):
            pl=u16(d,o); o+=2; pname=d[o:o+pl].decode('latin-1'); o+=pl
            ptype=d[o]; o+=1; o+=4
            vlen=u16(d,o); o+=2
            props[pname]=parse_prop_value(d,o,ptype,vlen)
            o+=vlen
        objs.append({'class':cls,'props':props})
        o=ds+objsz   # enforce stored size (verified exact)
    return {'text':text,'objects':objs,'geooff':geooff,'filesize':n}

def main(pattern):
    files=sorted(glob.glob(pattern))
    summary={}
    for path in files:
        try: w=parse_world(path)
        except Exception as e:
            print("FAIL %s: %s"%(os.path.basename(path),e)); continue
        from collections import Counter
        c=Counter(o['class'] for o in w['objects'])
        summary[os.path.basename(path)]=dict(c)
        # print spawn points and light info for the first file
        if path==files[0]:
            print("=== %s ==="%os.path.basename(path))
            print("header text:",w['text'])
            for o in w['objects']:
                if o['class']=='GameStartPoint':
                    print("  Spawn %r pos=%s rot=%s"%(o['props'].get('Name'),
                        o['props'].get('Pos'),o['props'].get('Rotation')))
            lights=[o for o in w['objects'] if 'Light' in o['class']]
            print("  %d lights, e.g.:"%len(lights))
            for L in lights[:3]:
                print("    %s pos=%s radius=%s color=%s"%(L['class'],
                    L['props'].get('Pos'),L['props'].get('LightRadius'),
                    L['props'].get('LightColor')))
    print("\n=== object class totals across %d worlds ==="%len(files))
    from collections import Counter
    tot=Counter()
    for c in summary.values():
        for k,v in c.items(): tot[k]+=v
    for k,v in tot.most_common():
        print("  %-24s %d"%(k,v))

if __name__=='__main__':
    main(sys.argv[1] if len(sys.argv)>1 else 'extracted/SHOGO/WORLDS/MULTI/*.DAT')
