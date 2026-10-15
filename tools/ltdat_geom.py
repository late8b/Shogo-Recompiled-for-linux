#!/usr/bin/env python3
"""Faithful reader for the LithTech (Shogo) .DAT world-geometry stream.

Structure derived from decompiled loader FUN_00418d80 (Client.exe) and
verified against its disassembly.  The stream is consumed strictly in order;
this tool models every read and validates by exact byte consumption.

Usage:
  ltdat_geom.py <world.dat>            # print header + fixed phases
  ltdat_geom.py <world.dat> --solve    # brute-force the header count fields
  ltdat_geom.py <world.dat> --counts k=v,...   # parse with explicit counts
"""
import struct
import sys


class EOF(Exception):
    pass


class Stream:
    def __init__(self, data, pos=0):
        self.d = data
        self.p = pos

    def read(self, n):
        if n < 0 or self.p + n > len(self.d):
            raise EOF('read %d at %d (EOF %d)' % (n, self.p, len(self.d)))
        b = self.d[self.p:self.p + n]
        self.p += n
        return b

    def u8(self):
        return self.read(1)[0]

    def u16(self):
        return struct.unpack_from('<H', self.read(2))[0]

    def i16(self):
        return struct.unpack_from('<h', self.read(2))[0]

    def u32(self):
        return struct.unpack_from('<I', self.read(4))[0]

    def f32(self):
        return struct.unpack_from('<f', self.read(4))[0]

    def rstr(self, maxn):
        out = bytearray()
        n = 0
        while n < maxn:
            c = self.read(1)
            n += 1
            if c == b'\0':
                return bytes(out)
            out += c
        return bytes(out)


def read_header(s):
    h = {}
    h['head'] = [s.u32() for _ in range(9)]
    h['name'] = s.rstr(100)
    h['ints'] = [s.u32() for _ in range(12)]
    h['bmin'] = [s.f32() for _ in range(3)]
    h['bmax'] = [s.f32() for _ in range(3)]
    h['vec3'] = [s.f32() for _ in range(3)]
    h['a'] = s.u32()
    h['table_bytes'] = s.u32()
    h['ntex'] = s.u32()
    return h


# ---- phases ------------------------------------------------------------

def ph_textures(s, c, h):
    for _ in range(h['ntex']):
        s.rstr(100)


def ph_pairs(s, c, h):
    return [(s.u8(), s.u8()) for _ in range(c['nDir'])]


def ph_surfaces(s, c, h):
    nv = c['nVert']
    for _ in range(c['nSurf']):
        pc = s.i16()
        if pc == -1:
            s.u16()
        else:
            for _ in range(pc):
                s.u16()
                blen = s.u16()
                s.read(blen)
        vc = s.u16()
        for _ in range(vc):
            vi = s.u32()
            if vi >= nv:
                raise ValueError('vidx %d >= %d' % (vi, nv))
        s.u32()


def ph_planeA(s, c, h):
    s.read(16 * c['nPlaneA'])


def ph_portals(s, c, h):
    for _ in range(c['nPort']):
        s.read(0x48)
        s.u16()
        pidx = s.u32()
        if pidx >= c['nPlaneA']:
            raise ValueError('pplane %d >= %d' % (pidx, c['nPlaneA']))
        flag = (s.u32() >> 24) & 0xff
        s.read(5)
        if flag == 1:
            s.rstr(99)
            s.rstr(255)
        s.u16()


def ph_dirbody(s, c, h, pairs):
    for i in range(c['nDir']):
        s.read(16)
        s.read(5 * (pairs[i][0] + pairs[i][1]))


def ph_vertices(s, c, h):
    s.read(34 * c['nVert'])


def ph_leaves(s, c, h):
    for _ in range(c['nLeaf']):
        s.rstr(99)
        s.read(30)


def ph_planeB(s, c, h):
    s.read(12 * c['nPlaneB'])


def ph_lightmap(s, c, h):
    w = s.u32(); hh = s.u32(); d = s.u32()
    n = w * hh * d
    s.read(0x18)
    for _ in range(n):
        v = s.u16(); s.u16(); s.read(v * 6)
    s.u32()


PHASES = [
    ('nDir', 'pairs'),
    ('nSurf', 'surfaces'),
    ('nPlaneA', 'planeA'),
    ('nPort', 'portals'),
    ('nDir', 'dirbody'),
    ('nVert', 'vertices'),
    ('nLeaf', 'leaves'),
    ('nPlaneB', 'planeB'),
    (None, 'lightmap'),
]


def run_phase(kind, s, c, h, pairs):
    if kind == 'pairs':
        return [(s.u8(), s.u8()) for _ in range(c['nDir'])]
    if kind == 'surfaces':
        ph_surfaces(s, c, h)
    elif kind == 'planeA':
        ph_planeA(s, c, h)
    elif kind == 'portals':
        ph_portals(s, c, h)
    elif kind == 'dirbody':
        ph_dirbody(s, c, h, pairs)
    elif kind == 'vertices':
        ph_vertices(s, c, h)
    elif kind == 'leaves':
        ph_leaves(s, c, h)
    elif kind == 'planeB':
        ph_planeB(s, c, h)
    elif kind == 'lightmap':
        ph_lightmap(s, c, h)
    return None



def parse_full(data, geo, c):
    s = Stream(data, geo)
    h = read_header(s)
    ph_textures(s, c, h)
    body = s.p
    pairs = ph_pairs(s, c, h)
    ph_surfaces(s, c, h)
    ph_planeA(s, c, h)
    ph_portals(s, c, h)
    ph_dirbody(s, c, h, pairs)
    ph_vertices(s, c, h)
    ph_leaves(s, c, h)
    ph_planeB(s, c, h)
    ph_lightmap(s, c, h)
    return s.p, h, body


def main(argv):
    path = argv[0]
    data = open(path, 'rb').read()
    geo = struct.unpack_from('<I', data, 8)[0]
    ss = Stream(data, geo)
    h = read_header(ss)
    ph_textures(ss, dict(), h)
    body = ss.p
    secdir = h['head'][0]
    print('file=%s size=%d geo=%d secdir=%d body=%d ntex=%d table_bytes=%d'
          % (path, len(data), geo, secdir, body, h['ntex'], h['table_bytes']))
    print('name=%r ints=%s' % (h['name'], h['ints']))
    print('bmin=%s bmax=%s vec3=%s a=%d' % (h['bmin'], h['bmax'], h['vec3'], h['a']))

    if '--counts' in argv:
        kv = argv[argv.index('--counts') + 1]
        c = {}
        for part in kv.split(','):
            k, v = part.split('=')
            c[k] = int(v)
        end, hh, bo = parse_full(data, geo, c)
        print('counts=%s -> end=%d (secdir=%d gap=%d, EOF gap=%d)'
              % (c, end, secdir, end - secdir, end - len(data)))
        return

    if '--solve' not in argv:
        return

    cand = sorted(set(x for x in h['ints'] if 0 < x < 100000))
    print('candidate counts:', cand)
    best = []

    def rec(level, pos, c, pairs):
        if pos > len(data):
            return
        if level == len(PHASES):
            d = min(abs(pos - secdir), abs(pos - len(data)))
            if d < 6000:
                best.append((d, pos, dict(c)))
            return
        role, kind = PHASES[level]
        if kind == 'dirbody':
            opts = [c['nDir']]
        else:
            opts = cand if role else [None]
        for v in opts:
            if role:
                c[role] = v
            s = Stream(data, pos)
            try:
                npairs = run_phase(kind, s, c, h, pairs)
            except (EOF, ValueError):
                continue
            rec(level + 1, s.p, c, npairs if npairs is not None else pairs)

    c0 = {'nVert': 682, 'nLeaf': 506, 'nSurf': 682, 'nPlaneA': 682,
          'nPort': 682, 'nDir': 682, 'nPlaneB': 682}
    rec(0, body, dict(c0), None)
    best.sort(key=lambda t: t[0])
    print('hits:', len(best))
    seen = set()
    for gap, end, c in best[:40]:
        key = tuple(sorted(c.items()))
        if key in seen:
            continue
        seen.add(key)
        print('gap=%6d end=%7d  %s' % (gap, end, c))


if __name__ == '__main__':
    main(sys.argv[1:])
