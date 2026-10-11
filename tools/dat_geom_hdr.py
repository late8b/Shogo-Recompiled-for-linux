#!/usr/bin/env python3
"""Header walk of .DAT geometry section (loader FUN_00418d80).

VERIFIED layout (byte-exact, 37238 + 126 = 37364 on MCA_MARITROPA2):

    @geooff  : 9 x u32                       (36 bytes; u32[0] = section/lightmap dir offset)
    @geooff+36: short string                 (2 bytes; e.g. "m\\0")
    @geooff+38: 22 x u32                     (88 bytes; includes bbox_min/max floats)
    @geooff+126: u32 texture_table_bytes     (== 1637)
    @geooff+130: u32 nTextures               (== 37)
    @geooff+134: nTextures NUL-terminated .dtx paths (table_bytes total)

The 22-u32 block is laid out as 12 ints, then 3 x vec3 (f32), then 1 int.
Never OOMs: every count is sanity-checked before use.

Usage:
    dat_geom_header.py <world.dat> [...]
    dat_geom_header.py --scan <dir/glob>       # field/consistency table over many worlds
"""
import glob
import struct
import sys


def u32(d, o):
    return struct.unpack_from('<I', d, o)[0]


def f32(d, o):
    return struct.unpack_from('<f', d, o)[0]


VEC = lambda d, o: tuple(round(f32(d, o + i * 4), 2) for i in range(3))


def parse(path):
    d = open(path, 'rb').read()
    n = len(d)
    geo = u32(d, 8)
    if geo + 126 > n:
        return None
    head9 = [u32(d, geo + i * 4) for i in range(9)]
    s2 = d[geo + 36:geo + 38]
    base = geo + 38
    ints12 = [u32(d, base + i * 4) for i in range(12)]
    vb = base + 48
    bmin, bmax, vec3 = VEC(d, vb), VEC(d, vb + 12), VEC(d, vb + 24)
    a = u32(d, vb + 36)
    o = vb + 40
    table_bytes, ntex = u32(d, o), u32(d, o + 4)
    o += 8
    ok = 0 < ntex < 2000 and o + table_bytes <= n
    texs = []
    if ok:
        te = o
        for _ in range(ntex):
            z = d.find(b'\0', o)
            if z < 0:
                ok = False
                break
            texs.append(d[o:z].decode('latin-1', 'replace'))
            o = z + 1
        ok = ok and (o - te) == table_bytes
    return dict(path=path, size=n, geo=geo, head9=head9, s2=s2,
                ints12=ints12, bmin=bmin, bmax=bmax, vec3=vec3, a=a,
                table_bytes=table_bytes, ntex=ntex, table_ok=ok,
                table_at=(vb + 48 if ok else None),
                table_end=o, secdir=u32(d, geo), body_end=None,
                texs=texs, d=d)


def show(r):
    print("=" * 74)
    print("%s  size=%d  geooff=%d  secdir(u32[geo])=%d" %
          (r['path'].split('/')[-1], r['size'], r['geo'], r['secdir']))
    print("  head9 :", r['head9'])
    print("  str2  :", r['s2'])
    print("  int12 :", r['ints12'])
    print("  bbox  : min=%s max=%s  vec3=%s  a=%d" %
          (r['bmin'], r['bmax'], r['vec3'], r['a']))
    print("  table : bytes=%d ntex=%d  @%d..%d  ok=%s" %
          (r['table_bytes'], r['ntex'], r['table_at'], r['table_end'], r['table_ok']))
    if r['texs']:
        print("  tex[0]:", r['texs'][0])
    # count candidates: the ints that look like plausible element counts
    cand = [(i, v) for i, v in enumerate(r['ints12']) if 0 < v < 200000]
    print("  candidate ints (value, as element count):", cand)
    print("  region: table_end=%d -> secdir=%d  (=%d bytes body+dir)" %
          (r['table_end'], r['secdir'], r['secdir'] - r['table_end']))


def scan(paths):
    fields = {}
    for p in paths:
        r = parse(p)
        if not r:
            continue
        fields.setdefault('names', []).append(p.split('/')[-1])
        for i, v in enumerate(r['ints12']):
            fields.setdefault(i, []).append(v)
    names = fields.pop('names')
    print("scanned %d worlds; int12 fields across worlds:" % len(names))
    for i in sorted(fields):
        vals = fields[i]
        nonzero = sum(1 for v in vals if v)
        print("  int12[%2d]: nonzero=%2d  min=%d  max=%d  sample=%s" %
              (i, nonzero, min(vals), max(vals), vals[:6]))


def main(argv):
    if len(argv) >= 2 and argv[0] == '--scan':
        paths = sorted(glob.glob(argv[1]))
        if not paths:
            paths = argv[1:]
        scan(paths)
        return
    for p in argv:
        r = parse(p)
        if r:
            show(r)
        else:
            print("SKIP (too small / bad header):", p)


if __name__ == '__main__':
    main(sys.argv[1:])
