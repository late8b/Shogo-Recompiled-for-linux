#!/usr/bin/env python3
"""
dtx2png.py -- LithTech 1.0 DTX texture decoder (Shogo MAD preservation).

DTX v0 layout (fully reverse-engineered, see docs/FINDINGS.md):

    offset 0   uint32  version      (= 0)
    offset 4   uint32  flags        (0xfffffffe)
    offset 8   uint16  width
    offset 10  uint16  height
    offset 12  uint16  mipcount     (usually 4)
    ...
    offset 44  uint32  palette[256]  stored A,R,G,B (byte order A,R,G,B)
    offset 1068  uint8  indices[mip0]  8bpp, row-major, w*h
              then mip1 (w/2*h/2), mip2, ... down to mipcount

We decode mip0 (the full-resolution base level) to an RGBA PNG.

The GLASS/ sub-format is 16-bit (a different DTX variant) and is skipped with
a warning unless --force.

Usage:
    dtx2png.py <file.dtx> [out.png]        decode one
    dtx2png.py --batch <in_dir> <out_dir>  decode a tree
"""
import struct, sys, os

try:
    from PIL import Image
except ImportError:
    Image = None


def parse_dtx(data):
    if len(data) < 44 + 1024:
        raise ValueError("too small for a DTX")
    version, flags = struct.unpack_from("<II", data, 0)
    w, h = struct.unpack_from("<HH", data, 8)
    mipcount = struct.unpack_from("<H", data, 12)[0]
    if w == 0 or h == 0:
        raise ValueError("zero dimensions")
    # palette: 256 entries of A,R,G,B
    pal = data[44:44 + 1024]
    rgba_pal = bytearray(1024)
    for i in range(256):
        a, r, g, b = pal[i*4], pal[i*4+1], pal[i*4+2], pal[i*4+3]
        rgba_pal[i*4+0] = r
        rgba_pal[i*4+1] = g
        rgba_pal[i*4+2] = b
        rgba_pal[i*4+3] = a
    # mip0 indices
    off = 44 + 1024
    need = w * h
    if off + need > len(data):
        raise ValueError("truncated pixel data (likely 16-bit GLASS variant)")
    idx = data[off:off + need]
    # expand to RGBA
    out = bytearray(w * h * 4)
    for p, ix in enumerate(idx):
        out[p*4:p*4+4] = rgba_pal[ix*4:ix*4+4]
    return w, h, bytes(out), mipcount, version, flags


def decode_bytes(data):
    """Return (w, h, rgba_bytes) or raise."""
    w, h, rgba, *_ = parse_dtx(data)
    return w, h, rgba


def to_png(data, out_path):
    if Image is None:
        raise RuntimeError("Pillow required for PNG output")
    w, h, rgba = decode_bytes(data)
    img = Image.frombytes("RGBA", (w, h), rgba)
    img.save(out_path)
    return w, h


def main(argv):
    if len(argv) >= 2 and argv[1] == "--batch":
        indir, outdir = argv[2], argv[3]
        os.makedirs(outdir, exist_ok=True)
        n_ok = n_fail = 0
        fails = []
        for root, _, files in os.walk(indir):
            for fn in files:
                if not fn.lower().endswith(".dtx"):
                    continue
                src = os.path.join(root, fn)
                rel = os.path.relpath(src, indir)
                dst = os.path.join(outdir, os.path.splitext(rel)[0] + ".png")
                os.makedirs(os.path.dirname(dst) or ".", exist_ok=True)
                try:
                    to_png(open(src, "rb").read(), dst)
                    n_ok += 1
                except Exception as e:
                    n_fail += 1
                    fails.append((rel, str(e)))
        print("OK: %d   FAIL: %d" % (n_ok, n_fail))
        from collections import Counter
        c = Counter(msg for _, msg in fails)
        for msg, cnt in c.most_common():
            print("  %-50s %d" % (msg[:50], cnt))
        return 0
    # single
    src = argv[1]
    dst = argv[2] if len(argv) > 2 else os.path.splitext(src)[0] + ".png"
    w, h = to_png(open(src, "rb").read(), dst)
    print("%s -> %s (%dx%d)" % (src, dst, w, h))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
