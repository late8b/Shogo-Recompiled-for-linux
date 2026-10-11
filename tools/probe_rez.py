#!/usr/bin/env python3
"""Probe the SOUNDS child block + hunt the root dir pointer empirically."""
import struct

GAME = "/home/a/Downloads/shogo-mobile-armor-division/Shogo Mobile Armor Division/"

def dump(label, data, start, size, cap=400):
    block = data[start:start+size]
    print("=== %s @%d size=%d ===" % (label, start, size))
    for i in range(0, min(len(block), cap), 16):
        chunk = block[i:i+16]
        hexpart = " ".join("%02x" % b for b in chunk)
        txt = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        print("  +%04x %-47s %s" % (i, hexpart, txt))
    print()

# ---- 1. Follow the decoded SOUNDS dir pointer ----
snd = open(GAME + "SOUND.REZ", "rb").read()
# SOUNDS record at EOF-23: type,offset,size,time then "SOUNDS\0"
rec_at = len(snd) - 23
typ, off, size, tm = struct.unpack_from("<IIII", snd, rec_at)
print("SOUNDS dir record: type=%d offset=%d size=%d time=%08x\n" % (typ, off, size, tm))
dump("SOUNDS child block", snd, off, size)

# ---- 2. Anchor SHOGO.REZ: find its last record ----
sho = open(GAME + "SHOGO.REZ", "rb").read()
print("=== SHOGO.REZ last 160 bytes ===")
tail = sho[-160:]
for i in range(0, 160, 16):
    chunk = tail[i:i+16]
    hexpart = " ".join("%02x" % b for b in chunk)
    txt = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
    print("  %d: %-47s %s" % (len(sho)-160+i, hexpart, txt))
print()

# ---- 3. Scan both big files' 10 header BE fields, flag dirStart candidates ----
for name, blob in (("SOUND.REZ", snd), ("SHOGO.REZ", sho)):
    n = len(blob)
    print("=== %s header BE fields 128..168 ===" % name)
    for i in range(10):
        v = struct.unpack_from(">I", blob, 128 + i*4)[0]
        flag = ""
        if 168 < v < n:
            flag = "  (valid offset, %d from EOF)" % (n - v)
        print("  field[%d] = %-12d 0x%08x%s" % (i, v, v, flag))
    print()
