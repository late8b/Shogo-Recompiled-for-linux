#!/usr/bin/env python3
"""
rezextract.py - LithTech RezMgr v1 (.rez) extractor -- Shogo MAD preservation.

FORMAT (fully reverse-engineered, docs/FINDINGS.md):

  bytes 0..127    ASCII info string terminated "\\r\\n\\x1a\\x01"  => header = 128 bytes
  byte  168       first resource payload (confirmed for all retail archives)

  Directory = a contiguous block of little-endian records:

      uint32 type     ; 0 = file (payload), 1 = directory (child block)
      uint32 offset   ; file: payload offset. dir: offset of child record block
      uint32 size     ; file: stored size. dir: byte length of child block
      uint32 dostime  ; DOS packed timestamp
      -- type==0 (file):
          uint32 flags            ; usually 0x00989680
          char   ext[8]           ; extension, stored REVERSED, NUL-padded ("VAW"=="WAV")
          char   name[]           ; NUL-terminated base name
      -- type==1 (dir):
          char   name[]           ; NUL-terminated name (no flags/ext)

  A block may open with a sentinel record (type=0, offset=0, size=0, ext empty,
  name "DIRTYPE<dirname>"); it is skipped. Payloads are stored raw (no compression).

Usage:
    rezextract.py <file.rez> [outDir]           auto-detect root, extract all
    rezextract.py --walk <file.rez> <off> <size>   print a subtree
    rezextract.py --extract <file.rez> <off> <size> [outDir]
"""
import struct, sys, os


def _try_record(data, p, end, n):
    """Try to parse one directory record at p. Return (rec, next_p) or None.
    Handles both type==0 (file: flags+ext+name) and type==1 (dir: name)."""
    if p + 16 > end:
        return None
    rtype, off, rsize, tm = struct.unpack_from("<IIII", data, p)
    if rtype not in (0, 1):
        return None
    q = p + 16
    if rtype == 0:
        if q + 12 > end:
            return None
        ext_raw = data[q+4:q+12]
        q += 12
    else:
        ext_raw = b""
    z = data.find(b"\x00", q, min(q + 260, end))
    if z == -1:
        return None
    name = data[q:z].decode("latin-1")
    ext = ext_raw.split(b"\x00")[0][::-1].decode("latin-1")
    rec = {"type": rtype, "name": name, "ext": ext,
           "offset": off, "size": rsize, "time": tm}
    return rec, z + 1


def _plausible(rec, n):
    """A real resource: payload (or child block) at offset>=168, inside file."""
    return 168 <= rec["offset"] and rec["offset"] + rec["size"] <= n


def read_records(data, start, size):
    """Parse one directory block. Blocks may open with a sentinel record
    (type=0, offset=0, size=0, name \"DIRTYPE<dirname>\") followed by one pad
    byte; we skip sentinels and byte-resync past any malformed header."""
    n = len(data)
    end = min(start + size, n)
    p = start
    out = []
    while p + 16 <= end:
        res = _try_record(data, p, end, n)
        if res is None:
            p += 1            # resync: step a byte and retry
            continue
        rec, nxt = res
        if not _plausible(rec, n):
            # offset==0 -> sentinel/footer (skip past it); else misparse (step 1).
            p = nxt if rec["offset"] == 0 else p + 1
            continue
        if not rec["name"].strip():
            p = nxt
            continue
        out.append(rec)
        p = nxt
    return out


def walk(data, start, size, path="", _depth=0):
    for rec in read_records(data, start, size):
        if not rec["name"].strip():
            continue  # skip empty-named padding / sentinel records
        full = path + "/" + rec["name"] if path else rec["name"]
        if rec["ext"]:
            full += "." + rec["ext"]
        if rec["type"] == 1:
            yield (full + "/", rec["offset"], rec["size"], True)
            if _depth < 32:
                yield from walk(data, rec["offset"], rec["size"], full, _depth + 1)
        else:
            yield (full, rec["offset"], rec["size"], False)


def autodetect_root(data):
    """Find the block that yields the most top-level entries (the real root),
    scanning a window near EOF where RezMgr stores the directory."""
    n = len(data)
    best = (None, 0, [])
    lo = max(168, n - 8 * 1024 * 1024)
    for cand in range(lo, n - 16):
        recs = read_records(data, cand, n - cand)
        if len(recs) > best[1]:
            best = (cand, len(recs), recs)
    return best


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    if argv[1] == "--walk":
        rez, off, size = argv[2], int(argv[3]), int(argv[4])
        data = open(rez, "rb").read()
        for name, o, s, isdir in walk(data, off, size):
            print("%-72s %10d%s" % (name, s, "  [DIR]" if isdir else ""))
        return 0
    if argv[1] == "--extract":
        rez, off, size = argv[2], int(argv[3]), int(argv[4])
        out = argv[5] if len(argv) > 5 else "extracted"
        data = open(rez, "rb").read()
        extract(data, off, size, out)
        return 0
    # default: auto-detect + extract
    rez = argv[1]
    out = argv[2] if len(argv) > 2 else "extracted"
    data = open(rez, "rb").read()
    root_off, cnt, recs = autodetect_root(data)
    if root_off is None:
        print("No root found in %s" % rez)
        return 1
    names = [r["name"] + ("." + r["ext"] if r["ext"] else "") for r in recs]
    print("Root @%d with %d entries: %s" % (root_off, cnt, names[:12]))
    extract(data, root_off, n_size(data, root_off), out)
    return 0


def n_size(data, root_off):
    return len(data) - root_off


def extract(data, off, size, out_dir):
    nfile = 0
    total = 0
    for name, o, s, isdir in walk(data, off, size):
        if isdir:
            continue
        name = name.rstrip("/")
        if not name:
            continue
        dest = os.path.join(out_dir, name.replace("/", os.sep))
        os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)
        with open(dest, "wb") as fh:
            fh.write(data[o:o+s])
        nfile += 1
        total += s
    print("Extracted %d files (%.1f MB) -> %s" % (nfile, total/1e6, out_dir))


if __name__ == "__main__":
    sys.exit(main(sys.argv))
