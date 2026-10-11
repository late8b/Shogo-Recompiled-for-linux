#!/usr/bin/env python3
"""Dump candidate directory blocks to settle the file-record layout."""
import struct
GAME = "/home/a/Downloads/shogo-mobile-armor-division/Shogo Mobile Armor Division/"
d = open(GAME + "SOUND.REZ", "rb").read()

def dump(label, off, size):
    print("=== %s @%d size=%d ===" % (label, off, size))
    blk = d[off:off+min(size,192)]
    for i in range(0, len(blk), 16):
        c = blk[i:i+16]
        print("  +%03d %-47s %s" % (i, " ".join("%02x" % b for b in c),
              "".join(chr(b) if 32 <= b < 127 else "." for b in c)))
    print()

# From the SOUNDS listing: PROPS (small, likely a clean record block)
# PROPS record was: type=1 offset=140836529 size=294
dump("PROPS child", 140836529, 294)
# The character block the probe found
dump("CHARACTER block", 140964085, 300)
# WEAPONS/TANTO (a leaf-ish dir, size 149) -- need its offset; re-parse SOUNDS block
# SOUNDS block @140965951 size 357
p = 140965951
end = p + 357
print("=== SOUNDS records (raw parse) ===")
while p + 16 <= end:
    t, o, s, tm = struct.unpack_from("<IIII", d, p)
    p += 16
    z = d.find(b"\x00", p, p+64)
    name = d[p:z].decode("latin-1")
    p = z + 1
    print("  type=%d off=%d size=%d name=%s" % (t, o, s, name))
    if name == "WEAPONS":
        print("  --> WEAPONS child block dump:")
        dump("WEAPONS child", o, s)
        break
