#!/usr/bin/env python3
"""LithTech 1.0 (DAT v56) world-geometry parser — Shogo / Blood II.

On-disk WorldBSP layout follows haekb's bspv56.bt, cross-checked against
Client.exe!FUN_00418d80 reads and every retail Shogo world.

World file:
    u32 version (56)
    u32 objects_offset
    u32 geometry_offset          # start of WorldModel root
    u32 info_len + info text
    objects...
    @geometry_offset:
        u32 next_section         # named-section directory (after all BSPs)
        u32 pad[8]
        WorldBSP                 # root model ("m")
        (optional extra WorldModels)

WorldBSP after the C-string name:
    12 x u32 counts, 1 x u32 unknown2, 3 x vec3 (min/max/translation),
    u32 texture_table_bytes, u32 nTextures, then NUL-terminated .dtx paths.
    Body: Vertex[poly] (count,extra), Leaf[], Plane[16], Surface[],
    Polygon[], Node[], Portal[], Point[12], PBlockTable, ...
"""
from __future__ import annotations

import struct
from dataclasses import dataclass, field


class EOF(Exception):
    pass


class Stream:
    def __init__(self, data: bytes, pos: int = 0):
        self.d = data
        self.p = pos

    def read(self, n: int) -> bytes:
        if n < 0 or self.p + n > len(self.d):
            raise EOF('read %d at %d (EOF %d)' % (n, self.p, len(self.d)))
        b = self.d[self.p:self.p + n]
        self.p += n
        return b

    def u8(self) -> int:
        return self.read(1)[0]

    def i16(self) -> int:
        return struct.unpack('<h', self.read(2))[0]

    def u16(self) -> int:
        return struct.unpack('<H', self.read(2))[0]

    def i32(self) -> int:
        return struct.unpack('<i', self.read(4))[0]

    def u32(self) -> int:
        return struct.unpack('<I', self.read(4))[0]

    def f32(self) -> float:
        return struct.unpack('<f', self.read(4))[0]

    def vec3(self) -> tuple[float, float, float]:
        return struct.unpack('<3f', self.read(12))

    def cstr(self, maxn: int = 256) -> str:
        out = bytearray()
        for _ in range(maxn):
            c = self.read(1)
            if c == b'\0':
                return out.decode('latin-1', 'replace')
            out += c
        return out.decode('latin-1', 'replace')

    def ltstr(self) -> str:
        n = self.i16()
        if n < 0 or n > 4096:
            raise ValueError('ltstr len %d at %d' % (n, self.p - 2))
        return self.read(n).decode('latin-1', 'replace')


@dataclass
class Surface:
    o: tuple
    p: tuple
    q: tuple
    texture: int
    plane: int
    flags: int
    texture_flags: int
    effect: str = ''
    effect_param: str = ''


@dataclass
class Polygon:
    surface: int
    lm_w: int
    lm_h: int
    indices: list  # point indices


@dataclass
class WorldBSP:
    name: str
    textures: list
    points: list
    planes: list
    surfaces: list
    polygons: list
    bmin: tuple
    bmax: tuple
    translation: tuple
    counts: dict
    body_end: int
    next_section: int


def _read_header_counts(s: Stream) -> dict:
    next_section = s.u32()
    for _ in range(8):
        s.u32()
    # Always 2 bytes (C-string "m\0" on small maps; non-NUL pair on large ones).
    raw_name = s.read(2)
    name = raw_name.split(b'\0', 1)[0].decode('latin-1', 'replace')
    ints = [s.u32() for _ in range(12)]
    unknown2 = s.u32()
    bmin, bmax, xlt = s.vec3(), s.vec3(), s.vec3()
    table_bytes = s.u32()
    ntex = s.u32()
    if not (0 < ntex < 4000 and table_bytes < 200000):
        raise ValueError('bad texture table ntex=%d bytes=%d' % (ntex, table_bytes))
    start = s.p
    textures = [s.cstr(200) for _ in range(ntex)]
    if s.p - start != table_bytes:
        raise ValueError('texture table %d != %d' % (s.p - start, table_bytes))
    return dict(
        next_section=next_section, name=name, ints=ints, unknown2=unknown2,
        bmin=bmin, bmax=bmax, xlt=xlt, textures=textures,
        point=ints[2], plane=ints[3], surface=ints[4], portal=ints[5],
        poly=ints[6], leaf=ints[7], vert=ints[8], vis=ints[9],
        leaflist=ints[10], node=ints[11],
    )


def _read_leaves(s: Stream, n: int) -> None:
    for i in range(n):
        llc = s.u16()
        if llc == 0xFFFF:
            s.u16()
        else:
            if llc > 4000:
                raise ValueError('leaf list %d at leaf %d' % (llc, i))
            for _ in range(llc):
                s.u16()
                sz = s.u16()
                if sz > 20000:
                    raise ValueError('leaf data %d' % sz)
                s.read(sz)
        pc = s.u16()
        if pc > 8000:
            raise ValueError('leaf polys %d' % pc)
        s.read(pc * 4)
        s.f32()


def _read_surfaces(s: Stream, n: int, ntex: int) -> list:
    out = []
    for i in range(n):
        o, p, q = s.vec3(), s.vec3(), s.vec3()
        s.vec3(); s.vec3(); s.vec3()
        tex = s.u16()
        plane = s.i32()
        flags = s.u32()
        s.read(4)
        use = s.u8()
        effect = effect_param = ''
        if use:
            effect = s.ltstr()
            effect_param = s.ltstr()
        tflags = s.u16()
        if tex >= ntex and ntex:
            # sky / missing slot still tolerated; keep the index
            pass
        out.append(Surface(o, p, q, tex, plane, flags, tflags, effect, effect_param))
    return out


def _read_polygons(s: Stream, verts: list, npoints: int) -> list:
    out = []
    for i, (c, e) in enumerate(verts):
        lm_w, lm_h = s.i16(), s.i16()
        s.f32(); s.f32()
        surf = s.i32()
        nv = c + e
        if nv > 64 or nv < 0:
            raise ValueError('poly %d nv=%d' % (i, nv))
        idx = []
        for _ in range(nv):
            vi = s.i16()
            s.read(3)
            idx.append(vi)
        out.append(Polygon(surf, lm_w, lm_h, idx))
    return out


def _read_nodes(s: Stream, n: int) -> None:
    for _ in range(n):
        s.i32(); s.i32(); s.i16(); s.i32(); s.i32()
        s.read(16)


def _read_portals(s: Stream, n: int) -> None:
    for _ in range(n):
        s.ltstr()
        s.i32(); s.u16()
        s.vec3(); s.vec3()


def _read_pblocks(s: Stream) -> None:
    a, b, c = s.u32(), s.u32(), s.u32()
    n = a * b * c
    if n < 0 or n > 200000:
        raise ValueError('pblock dim %s' % ((a, b, c),))
    s.vec3(); s.vec3()
    for _ in range(n):
        sz = s.i16()
        s.i16()
        if sz < 0 or sz > 4000:
            raise ValueError('pblock size %d' % sz)
        s.read(sz * 6)


def parse_world_bsp(s: Stream) -> WorldBSP:
    h = _read_header_counts(s)
    verts = [(s.u8(), s.u8()) for _ in range(h['poly'])]
    _read_leaves(s, h['leaf'])
    planes = [s.vec3() + (s.f32(),) for _ in range(h['plane'])]
    surfaces = _read_surfaces(s, h['surface'], len(h['textures']))
    polygons = _read_polygons(s, verts, h['point'])
    _read_nodes(s, h['node'])
    _read_portals(s, h['portal'])
    points = [s.vec3() for _ in range(h['point'])]
    try:
        _read_pblocks(s)
    except (EOF, ValueError):
        pass
    return WorldBSP(
        name=h['name'], textures=h['textures'], points=points, planes=planes,
        surfaces=surfaces, polygons=polygons, bmin=h['bmin'], bmax=h['bmax'],
        translation=h['xlt'], counts={k: h[k] for k in
            ('point', 'plane', 'surface', 'portal', 'poly', 'leaf',
             'vert', 'vis', 'leaflist', 'node')},
        body_end=s.p, next_section=h['next_section'],
    )


def parse_dat(data: bytes) -> WorldBSP:
    if len(data) < 16:
        raise ValueError('too small')
    ver, _obj, geo = struct.unpack_from('<III', data, 0)
    if ver != 56:
        raise ValueError('DAT version %d (want 56)' % ver)
    if geo + 40 > len(data):
        raise ValueError('bad geometry offset %d' % geo)
    return parse_world_bsp(Stream(data, geo))


def mesh_stats(w: WorldBSP) -> dict:
    nidx = 0
    bad_pt = bad_surf = 0
    for poly in w.polygons:
        nidx += len(poly.indices)
        if poly.surface < 0 or poly.surface >= len(w.surfaces):
            bad_surf += 1
        for i in poly.indices:
            if i < 0 or i >= len(w.points):
                bad_pt += 1
    xs = [p[0] for p in w.points] or [0]
    ys = [p[1] for p in w.points] or [0]
    zs = [p[2] for p in w.points] or [0]
    return dict(
        points=len(w.points), polys=len(w.polygons), surfs=len(w.surfaces),
        texs=len(w.textures), corners=nidx, bad_pt=bad_pt, bad_surf=bad_surf,
        pmin=(min(xs), min(ys), min(zs)), pmax=(max(xs), max(ys), max(zs)),
        body_end=w.body_end, next_section=w.next_section,
    )


def opq_uv(pos, surf: Surface) -> tuple[float, float]:
    """LithTech OPQ / world mapping: u = P·pos + O.x, v = Q·pos + O.y."""
    px, py, pz = pos
    u = surf.p[0] * px + surf.p[1] * py + surf.p[2] * pz + surf.o[0]
    v = surf.q[0] * px + surf.q[1] * py + surf.q[2] * pz + surf.o[1]
    return u, v


if __name__ == '__main__':
    import sys, glob, os
    paths = []
    for a in sys.argv[1:]:
        if any(c in a for c in '*?'):
            paths.extend(sorted(glob.glob(a)))
        else:
            paths.append(a)
    if not paths:
        sys.exit('usage: ltdat.py <world.dat> [...]')
    for path in paths:
        data = open(path, 'rb').read()
        try:
            w = parse_dat(data)
            st = mesh_stats(w)
            ok = st['bad_pt'] == 0 and st['bad_surf'] == 0 and st['points'] > 0
            print('%s  %s  pts=%d poly=%d surf=%d tex=%d  idx_ok=%s  body=%d secdir=%d'
                  % (os.path.basename(path), 'OK' if ok else 'BAD',
                     st['points'], st['polys'], st['surfs'], st['texs'],
                     ok, st['body_end'], st['next_section']))
            if not ok:
                print('   bad_pt=%d bad_surf=%d bbox=%s..%s'
                      % (st['bad_pt'], st['bad_surf'], st['pmin'], st['pmax']))
        except Exception as e:
            print('%s  FAIL  %s' % (os.path.basename(path), e))
