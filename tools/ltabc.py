#!/usr/bin/env python3
"""
abc.py -- LithTech 1.0 "MonolithExport Model File v6" (.ABC) parser.

Reverse-engineered from the decompiled Client.exe model loader
(FUN_004343e0 top-level, FUN_00434ad0 geometry, FUN_00435270 property walk)
and validated against retail SHOGO.REZ models.

Property-tree grammar
---------------------
A file is a flat sequence of properties; each is:

    u16  nameLen
    char name[nameLen]        (ASCII, no NUL)
    u32  endOffset            (absolute file offset of the NEXT property)
    u8   data[endOffset - here]

The top-level properties observed: Header, Geometry, Nodes, Animation.

Header data:
    u16 strLen, char[strLen]  -> "MonolithExport Model File v6", NUL, pad even

Geometry data (FUN_00434ad0):
    24 bytes  bounding box: f32 min.x,min.y,min.z, max.x,max.y,max.z
    u32  nNames;  u8 nameTable[nNames*2 + 2]     (material name index table)
    u32  nFaces
      per face (33 bytes):
        6 x f32  (24 B)  3 UV pairs, one per triangle corner (clamped on load)
        u16 x 3           vertex indices  (a triangle)
        u8  x 3           per-corner flags
    u32  nVerts
    u32  nNorms           (normals are computed/loaded elsewhere; not in file here)
    per vert (20 bytes):
        3 x f32 position  (12 B)
        u8 x 4            (flags / light)
        u16, u16          (remaining fields)

So each face is a triangle carrying its own 3 UVs. To preserve UV seams we
expand to per-corner vertices (position + uv) for glTF/OBJ.

This module returns a Mesh: positions[], uvs[], normals[](optional), and
triangle corner indices into the (position,uv) pairs.
"""
import struct, os


class Mesh:
    def __init__(self):
        self.name = ""
        self.positions = []   # list of (x,y,z)
        self.uvs = []         # list of (u,v)  -- per corner, aligned with positions
        self.normals = []     # list of (x,y,z) or empty
        self.indices = []     # triangle corner indices
        self.material = ""
        self.bbox = None

    @property
    def vert_count(self):
        return len(self.positions)

    @property
    def tri_count(self):
        return len(self.indices) // 3


def _walk(d, start, end):
    """Yield (name, dataStart, dataEnd) for each property in [start,end)."""
    p = start
    while p + 6 <= end:
        n = struct.unpack_from('<H', d, p)[0]
        if n == 0 or n > 255 or p + 2 + n + 4 > end:
            break
        name = d[p+2:p+2+n]
        if not all(32 <= b < 127 for b in name):
            break
        endoff = struct.unpack_from('<I', d, p+2+n)[0]
        ds = p + 2 + n + 4
        if endoff <= ds or endoff > end:
            break
        yield name.decode('latin-1'), ds, endoff
        p = endoff


def _parse_geometry(d, ds, de):
    o = ds
    bbox = struct.unpack_from('<6f', d, o); o += 24
    nNames = struct.unpack_from('<I', d, o)[0]; o += 4
    o += nNames * 2 + 2
    nFaces = struct.unpack_from('<I', d, o)[0]; o += 4
    faces = []
    for _ in range(nFaces):
        uvs = struct.unpack_from('<6f', d, o); o += 24
        i0, i1, i2 = struct.unpack_from('<3H', d, o); o += 6
        f0, f1, f2 = struct.unpack_from('<3B', d, o); o += 3
        faces.append(((uvs[0], uvs[1]), (uvs[2], uvs[3]), (uvs[4], uvs[5]),
                      i0, i1, i2))
    nVerts = struct.unpack_from('<I', d, o)[0]; o += 4
    nNorms = struct.unpack_from('<I', d, o)[0]; o += 4
    verts = []
    for _ in range(nVerts):
        x, y, z = struct.unpack_from('<3f', d, o); o += 12
        o += 4        # 4 flag bytes
        o += 4        # 2 x u16
        verts.append((x, y, z))
    return bbox, faces, verts, nNorms


def load(path):
    d = open(path, 'rb').read()
    props = {name: (ds, de) for name, ds, de in _walk(d, 0, len(d))}
    m = Mesh()
    m.name = os.path.splitext(os.path.basename(path))[0]
    if 'Geometry' in props:
        ds, de = props['Geometry']
        bbox, faces, verts, nNorms = _parse_geometry(d, ds, de)
        m.bbox = bbox
        # Expand per-corner so UV seams are preserved.
        for (uv0, uv1, uv2, i0, i1, i2) in faces:
            for ci in (i0, i1, i2):
                if ci >= len(verts):
                    continue
                m.positions.append(verts[ci])
                # UVs in LithTech are typically (u, 1-v) for top-left origin;
                # we keep raw and flip in the glTF exporter.
            m.uvs.extend([uv0, uv1, uv2])
            base = len(m.positions) - 3
            m.indices.extend([base, base + 1, base + 2])
    return m


if __name__ == '__main__':
    for f in sys.argv[1:] if False else []:
        pass
    import sys
    for f in sys.argv[1:]:
        m = load(f)
        print("%-28s verts=%-6d tris=%-6d bbox=%s" % (
            os.path.basename(f), m.vert_count, m.tri_count,
            tuple(round(v, 2) for v in m.bbox) if m.bbox else None))
