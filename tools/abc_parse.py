#!/usr/bin/env python3
"""
ABC (LithTech "MonolithExport Model File v6") property-tree parser.

Grammar (confirmed against decompiled loader FUN_004343e0 / FUN_00435270):

    property := u16 nameLen, name[nameLen], u32 endOffset, data[endOffset - here]
                ... next property begins exactly at endOffset

Header data := u16 strLen, chars, NUL, (pad to even)

Geometry data := sequence of reads per FUN_00434ad0; we brute-force the
exact layout and require it to consume EXACTLY endOffset - dataStart bytes.
"""
import struct, sys, os

def rd_name(d, p):
    n = struct.unpack_from('<H', d, p)[0]
    name = d[p+2:p+2+n].decode('latin-1', 'replace')
    return name, p + 2 + n

def parse_properties(d, start, end, depth=0):
    """Walk properties in [start, end). Returns list of (name, dataStart, dataEnd)."""
    props = []
    p = start
    while p + 6 <= end:
        name, p2 = rd_name(d, p)
        if not name or len(name) > 255 or not all(32 <= ord(c) < 127 for c in name):
            break
        endoff = struct.unpack_from('<I', d, p2)[0]
        data_start = p2 + 4
        if endoff <= data_start - 4 or endoff > end:
            break
        props.append((name, data_start, endoff))
        p = endoff
    return props


def try_layouts(data):
    """Try candidate Geometry read-sequences; return those consuming len(data) exactly."""
    L = len(data)
    results = []

    def u32(o): return struct.unpack_from('<I', data, o)[0]

    # Candidate: [bbox?][u32 A][A*2+2 bytes][u32 nF][nF*(24+7)][u32 nV][u32 nN][nV*stride]
    for bbox in (0, 24, 32):
        o = bbox
        if o + 4 > L: continue
        A = u32(o); o += 4
        if A > 100000: continue
        o += A * 2 + 2
        if o + 4 > L: continue
        nF = u32(o); o += 4
        if nF == 0 or nF > 100000: continue
        # per-face: 24 bytes floats + 7 bytes misc (2+2+1+1+1)
        for face_extra in (6, 7, 8, 10):
            o2 = o + nF * (24 + face_extra)
            if o2 + 8 > L: continue
            nV, nN = u32(o2), u32(o2 + 4)
            if nV == 0 or nV > 200000: continue
            for stride in (19, 20, 21, 24, 28, 32):
                end = o2 + 8 + nV * stride
                if end == L:
                    results.append(dict(bbox=bbox, A=A, nF=nF, face_extra=face_extra,
                                        nV=nV, nN=nN, stride=stride))
    return results


def main(path):
    d = open(path, 'rb').read()
    print("=== %s (%d bytes) ===" % (os.path.basename(path), len(d)))
    # top level: walk properties from offset 0
    props = parse_properties(d, 0, len(d))
    if not props:
        print("  no top-level properties"); return
    for name, ds, de in props:
        print("  prop %-12s data[%d..%d) len=%d" % (name, ds, de, de - ds))
        if name.startswith('Geometry'):
            lays = try_layouts(d[ds:de])
            if not lays:
                print("      -> NO LAYOUT MATCH")
            for lay in lays:
                print("      -> MATCH %s" % lay)


if __name__ == '__main__':
    for f in sys.argv[1:]:
        main(f)
        print()
