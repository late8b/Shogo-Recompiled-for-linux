#!/usr/bin/env python3
"""
rezscan.py - Discover the RezMgr v1 header/directory layout empirically.

We know from manual decoding:
  * bytes 0..127   : ASCII info string, terminated by "\r\n\x1a\x01"
  * byte 168       : always the first resource payload (MZ / RIFF / "Shogo...")
  * the directory  : a block of records near EOF; each record carries a payload
                     offset (>=168), a size, and a printable ASCII path name.

Different RezMgr builds order the header words differently, so instead of
assuming, we try every plausible (dirStart, record-header-size) combination and
score it by how cleanly the resulting names parse as printable ASCII paths.
"""
import struct
import sys
import string

PRINTABLE = set(string.printable) - set("\x0b\x0c")


def try_parse_dir(data, dir_start, rec_hdr_len, max_rec=200000):
    """Walk records from dir_start. Return (entries, reason_ok)."""
    n = len(data)
    p = dir_start
    entries = []
    for _ in range(max_rec):
        if p + rec_hdr_len + 2 > n:
            break
        # payload offset & size are the first two big-endian u32 in every variant
        off, size = struct.unpack_from(">II", data, p)
        # name: try NUL-terminated string starting a few bytes after the fixed hdr
        for name_at in (p + rec_hdr_len, p + rec_hdr_len - 2, p + rec_hdr_len - 4):
            if name_at < p or name_at >= n:
                continue
            z = data.find(b"\x00", name_at, name_at + 128)
            if z == -1:
                continue
            raw = data[name_at:z]
            if not raw:
                continue
            try:
                name = raw.decode("ascii")
            except UnicodeDecodeError:
                continue
            if name and all(c in PRINTABLE for c in name) and (off >= 168):
                rec_len = rec_hdr_len + (z - name_at) + 1
                if rec_len % 2:
                    rec_len += 1
                entries.append((name, off, size))
                p += rec_len
                break
        else:
            return entries, False   # could not read a clean name -> reject layout
    return entries, len(entries) > 0


def score(data, dir_start, rec_hdr_len):
    entries, ok = try_parse_dir(data, dir_start, rec_hdr_len)
    if not ok or not entries:
        return -1, []
    # good layout: most names contain a path separator or a known extension,
    # and payload offsets are inside the file
    n = len(data)
    good = sum(1 for name, off, sz in entries
               if ("." in name or "\\" in name or "/" in name) and off < n)
    return good, entries


def discover(path, topn=6):
    with open(path, "rb") as f:
        data = f.read()
    n = len(data)
    hdr = data[:128].rstrip(b"\x00 \r\n\x1a\x01").decode("latin-1", "replace")
    words = struct.unpack_from(">20I", data, 128)
    candidates = set()
    for w in words:
        if 168 < w < n:
            candidates.add(w)
    # also try "directory is the last N bytes" for a range of N
    best = []
    for ds in sorted(candidates):
        for rh in (16, 20, 24, 28, 32):
            g, entries = score(data, ds, rh)
            if g > 0:
                best.append((g, ds, rh, entries))
    best.sort(key=lambda t: -t[0])
    print("== %s (size=%d)" % (path.split("/")[-1], n))
    print("   info: %s" % hdr)
    if not best:
        print("   no layout discovered")
        return
    seen = set()
    shown = 0
    for g, ds, rh, entries in best:
        key = (ds, rh)
        if key in seen:
            continue
        seen.add(key)
        names = [e[0] for e in entries]
        print("   dirStart=%d recHdr=%d  score=%d  count=%d" % (ds, rh, g, len(entries)))
        print("       sample: %s" % (names[:6],))
        shown += 1
        if shown >= topn:
            break


if __name__ == "__main__":
    for p in sys.argv[1:]:
        discover(p)
