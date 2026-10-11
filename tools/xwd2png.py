#!/usr/bin/env python3
"""xwd2png.py -- Convert an XWD screen dump to PNG (no external deps but PIL).

xwd dumps ZPixmap data. For a 32-bit TrueColor visual (typical on modern X11)
the pixel data is little/big-endian BGRA/RGBX rows, bytes_per_line apart.
We honor the header's byte_order, bits_per_pixel, and masks so it works for
either byte order and 24/32-bpp.
"""
import struct, sys
from PIL import Image

def convert(src, dst):
    d = open(src, 'rb').read()
    (hdr_size, file_ver, pix_fmt, pix_depth, pix_w, pix_h, xoffset,
     byte_order, bitmap_unit, bitmap_bit_order, bitmap_pad, bpp, bpl,
     visual_class, red_mask, green_mask, blue_mask, bits_per_rgb,
     cmap_entries, ncolors) = struct.unpack_from('>20I', d, 0)
    # window name follows header (hdr_size - 104 bytes); pixel data after
    # header + window name + color map.
    # color map: ncolors * 12 bytes (each: 4*3 uint32: pixel,r,g,b)
    px_off = hdr_size + ncolors * 12
    w, h = pix_w, pix_h
    img = Image.new('RGB', (w, h))
    px = img.load()
    big = (byte_order == 1)  # 1 = MSB first (big endian)

    def shift_mask(v, mask):
        if mask == 0:
            return 0
        shift = (mask & -mask).bit_length() - 1
        maxv = mask >> shift
        return int(((v & mask) >> shift) * 255 / maxv)

    bytespp = (bpp + 7) // 8
    for y in range(h):
        row = px_off + y * bpl
        for x in range(w):
            o = row + x * bytespp
            chunk = d[o:o + bytespp]
            if len(chunk) < bytespp:
                break
            if big:
                val = int.from_bytes(chunk, 'big')
            else:
                val = int.from_bytes(chunk, 'little')
            r = shift_mask(val, red_mask)
            g = shift_mask(val, green_mask)
            b = shift_mask(val, blue_mask)
            px[x, y] = (r, g, b)
    img.save(dst)
    print("wrote %s (%dx%d, bpp=%d, bpl=%d, endian=%s)" %
          (dst, w, h, bpp, bpl, 'BE' if big else 'LE'))

if __name__ == '__main__':
    convert(sys.argv[1], sys.argv[2])
