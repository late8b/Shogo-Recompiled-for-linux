#!/usr/bin/env python3
"""
dat_world.py -- Map the structure of a LithTech 1.0 DEdit world (.DAT).

We know (from the header of MCA_MARITROPA2.DAT):
    +0  u32  ? (56)
    +4  u32  objects_offset (121)
    +8  u32  geometry_hint (37238)
    +12 u32  text_len (73)
    +16 text blob (MasterPalette ...;WorldName "...")

This walks the object/entity list to find its end, then probes what follows
(vertices / polygons / surfaces), which is what we need to export worlds to glTF.
"""
import struct, sys, os

def u32(d, o): return struct.unpack_from('<I', d, o)[0]
def u16(d, o): return struct.unpack_from('<H', d, o)[0]
def i32(d, o): return struct.unpack_from('<i', d, o)[0]
def f32(d, o): return struct.unpack_from('<f', d, o)[0]

def read_string(d, o):
    """DEdit string: u16 length, then bytes (no NUL)."""
    n = u16(d, o)
    s = d[o+2:o+2+n].decode('latin-1', 'replace')
    return s, o + 2 + n

def walk_objects(d, start, max_objects=100000):
    """Walk the object list; return (n_objects, end_offset, sample names)."""
    n = u32(d, start)
    o = start + 4
    names = []
    for k in range(n):
        if o + 4 > len(d): break
        # u16 ? u16 nameLen name
        a = u16(d, o); b = u16(d, o+2)
        name, o2 = read_string(d, o+2)
        # then u32 nProps, then property list (u16 len, name, type, value...)
        if o2 + 4 > len(d): break
        nprops = u32(d, o2)
        o = o2 + 4
        # We need to skip each property generically. Properties appear to be:
        #   u16 nameLen, name, [type byte?], value
        # This is heuristic; we walk until we've consumed nprops by scanning
        # for the next object's recognizable shape. To keep it robust we just
        # record the name and do a bounded scan.
        if k < 5:
            names.append((name, nprops))
        # crude: skip forward by scanning for next plausible object header later
        # (full property walk below)
        o = skip_props(d, o, nprops)
    return n, o, names

def skip_props(d, o, nprops):
    """Skip nprops DEdit properties. Each: u16 nameLen, name, then typed value.
    We try to parse the value by type tag. Returns new offset."""
    for _ in range(nprops):
        if o + 2 > len(d): return o
        name, o = read_string(d, o)
        if o >= len(d): return o
        # After the name there is a type indicator. Empirically the value
        # encoding varies; we detect by inspecting following bytes.
        # Common: a u16/u32 then payload. We'll use a small parser.
        o = skip_one_value(d, o)
    return o

def skip_one_value(d, o):
    """Heuristically skip a single property value. DEdit world props store
    the value type as a small tag then the payload. We sniff the payload."""
    # Try: many DEdit values are prefixed by a u16 type code.
    # We'll read a u16 and branch.
    if o + 2 > len(d): return o
    tag = u16(d, o)
    # This is guesswork; fall back to returning o+2 so the outer loop can
    # resync. A more precise parser is added once we observe real data.
    return o + 2

def main(path):
    d = open(path, 'rb').read()
    print("=== %s (%d bytes) ===" % (os.path.basename(path), len(d)))
    print("header u32s:", [u32(d, i) for i in (0, 4, 8, 12)])
    tl = u32(d, 12)
    print("text: %r" % d[16:16+tl])
    obj_off = u32(d, 4)
    n = u32(d, obj_off)
    print("objects @%d, n=%d" % (obj_off, n))

    # Dump the first object raw so we can refine the property grammar.
    o = obj_off + 4
    print("\nfirst object raw bytes @%d:" % o)
    for j in range(o, o + 96, 16):
        c = d[j:j+16]
        print("  %04d %-47s %s" % (j, ' '.join('%02x'%b for b in c),
                                    ''.join(chr(b) if 32 <= b < 127 else '.' for b in c)))

if __name__ == '__main__':
    main(sys.argv[1])
