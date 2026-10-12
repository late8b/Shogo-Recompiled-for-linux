#!/usr/bin/env python3
"""Export a LithTech 1.0 .DAT world (root WorldBSP) to glTF 2.0 binary.

Mirrors tools/abc2glb.py: LithTech Z-up -> glTF Y-up (x, z, -y), V flipped.
Polygons are fan-triangulated. One glTF primitive per world texture.
"""
import json
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ltdat


def lt_to_gltf(x, y, z):
    return (x, z, -y)


def compute_smooth_normals(positions, indices):
    normals = [[0.0, 0.0, 0.0] for _ in positions]
    for t in range(0, len(indices), 3):
        i0, i1, i2 = indices[t], indices[t + 1], indices[t + 2]
        ax, ay, az = positions[i0]
        bx, by, bz = positions[i1]
        cx, cy, cz = positions[i2]
        e1 = (bx - ax, by - ay, bz - az)
        e2 = (cx - ax, cy - ay, cz - az)
        nx = e1[1] * e2[2] - e1[2] * e2[1]
        ny = e1[2] * e2[0] - e1[0] * e2[2]
        nz = e1[0] * e2[1] - e1[1] * e2[0]
        for i in (i0, i1, i2):
            normals[i][0] += nx
            normals[i][1] += ny
            normals[i][2] += nz
    out = []
    for x, y, z in normals:
        l = math.sqrt(x * x + y * y + z * z)
        if l > 1e-12:
            out.append((x / l, y / l, z / l))
        else:
            out.append((0.0, 0.0, 1.0))
    return out


def build_mesh(w: ltdat.WorldBSP):
    """Per-corner arrays grouped by texture index.

    Returns (groups, n_tris) where groups is a list of
    (tex_name, positions, uvs, indices).
    """
    buckets = {}
    for poly in w.polygons:
        if poly.surface < 0 or poly.surface >= len(w.surfaces):
            continue
        surf = w.surfaces[poly.surface]
        idx = [i for i in poly.indices if 0 <= i < len(w.points)]
        if len(idx) < 3:
            continue
        key = surf.texture
        b = buckets.setdefault(key, {'pos': [], 'uv': [], 'idx': []})
        base = len(b['pos'])
        for vi in idx:
            p = w.points[vi]
            b['pos'].append(lt_to_gltf(*p))
            u, v = ltdat.opq_uv(p, surf)
            b['uv'].append((u, 1.0 - v))
        for k in range(1, len(idx) - 1):
            b['idx'].extend((base, base + k, base + k + 1))
    groups = []
    n_tris = 0
    for tex_i in sorted(buckets):
        b = buckets[tex_i]
        if not b['idx']:
            continue
        name = w.textures[tex_i] if 0 <= tex_i < len(w.textures) else 'tex_%d' % tex_i
        groups.append((name, b['pos'], b['uv'], b['idx']))
        n_tris += len(b['idx']) // 3
    return groups, n_tris


def pad4(b):
    return b + b'\x00' * ((4 - len(b) % 4) % 4)


def export_glb(w: ltdat.WorldBSP, out_path: str):
    groups, n_tris = build_mesh(w)
    if not groups:
        raise ValueError('0 triangles')

    bin_parts = []
    views = []
    accessors = []
    primitives = []
    materials = []
    off = 0

    def add_bytes(raw, target, acc):
        nonlocal off
        b = pad4(raw)
        views.append({'buffer': 0, 'byteOffset': off, 'byteLength': len(raw),
                      'target': target})
        accessors.append(acc)
        bin_parts.append(b)
        off += len(b)
        return len(accessors) - 1

    for name, positions, uvs, indices in groups:
        normals = compute_smooth_normals(positions, indices)
        xs = [p[0] for p in positions]
        ys = [p[1] for p in positions]
        zs = [p[2] for p in positions]
        mn = [min(xs), min(ys), min(zs)]
        mx = [max(xs), max(ys), max(zs)]
        n = len(positions)
        pos_i = add_bytes(
            b''.join(struct.pack('<3f', *p) for p in positions), 34962,
            {'bufferView': len(views), 'componentType': 5126, 'count': n,
             'type': 'VEC3', 'min': mn, 'max': mx})
        nrm_i = add_bytes(
            b''.join(struct.pack('<3f', *v) for v in normals), 34962,
            {'bufferView': len(views), 'componentType': 5126, 'count': n,
             'type': 'VEC3'})
        uv_i = add_bytes(
            b''.join(struct.pack('<2f', *t) for t in uvs), 34962,
            {'bufferView': len(views), 'componentType': 5126, 'count': n,
             'type': 'VEC2'})
        idx_i = add_bytes(
            b''.join(struct.pack('<I', i) for i in indices), 34963,
            {'bufferView': len(views), 'componentType': 5125, 'count': len(indices),
             'type': 'SCALAR'})
        mat_i = len(materials)
        materials.append({
            'name': name,
            'pbrMetallicRoughness': {
                'baseColorFactor': [1, 1, 1, 1],
                'metallicFactor': 0.0, 'roughnessFactor': 1.0},
        })
        primitives.append({
            'attributes': {'POSITION': pos_i, 'NORMAL': nrm_i, 'TEXCOORD_0': uv_i},
            'indices': idx_i, 'material': mat_i, 'mode': 4,
        })

    # fix accessor bufferView indices (they were captured before append)
    for i, acc in enumerate(accessors):
        acc['bufferView'] = i

    bin_chunk = b''.join(bin_parts)
    gltf = {
        'asset': {'version': '2.0', 'generator': 'shogo-mad dat2glb'},
        'scene': 0,
        'scenes': [{'nodes': [0]}],
        'nodes': [{'mesh': 0, 'name': w.name or 'world'}],
        'meshes': [{'name': w.name or 'world', 'primitives': primitives}],
        'materials': materials,
        'accessors': accessors,
        'bufferViews': views,
        'buffers': [{'byteLength': len(bin_chunk)}],
    }
    json_bytes = json.dumps(gltf, separators=(',', ':')).encode('utf-8')
    json_bytes += b' ' * ((4 - len(json_bytes) % 4) % 4)
    bin_bytes = pad4(bin_chunk)
    total = 12 + 8 + len(json_bytes) + 8 + len(bin_bytes)
    os.makedirs(os.path.dirname(out_path) or '.', exist_ok=True)
    with open(out_path, 'wb') as f:
        f.write(struct.pack('<4sII', b'glTF', 2, total))
        f.write(struct.pack('<I', len(json_bytes)))
        f.write(b'JSON')
        f.write(json_bytes)
        f.write(struct.pack('<I', len(bin_bytes)))
        f.write(b'BIN\x00')
        f.write(bin_bytes)
    return sum(len(g[1]) for g in groups), n_tris


def convert_file(src, dst):
    w = ltdat.parse_dat(open(src, 'rb').read())
    return export_glb(w, dst)


if __name__ == '__main__':
    if len(sys.argv) >= 4 and sys.argv[1] == '--batch':
        indir, outdir = sys.argv[2], sys.argv[3]
        ok = fail = 0
        fails = []
        for root, _, files in os.walk(indir):
            for fn in files:
                if not fn.lower().endswith('.dat'):
                    continue
                src = os.path.join(root, fn)
                rel = os.path.relpath(src, indir)
                dst = os.path.join(outdir, os.path.splitext(rel)[0] + '.glb')
                try:
                    nv, nt = convert_file(src, dst)
                    ok += 1
                    print('OK  %s  verts=%d tris=%d' % (rel, nv, nt))
                except Exception as e:
                    fail += 1
                    fails.append((rel, str(e)))
        print('OK: %d   FAIL: %d' % (ok, fail))
        for rel, msg in fails:
            print('  FAIL %s  %s' % (rel, msg))
    else:
        src = sys.argv[1]
        dst = sys.argv[2] if len(sys.argv) > 2 else os.path.splitext(src)[0] + '.glb'
        nv, nt = convert_file(src, dst)
        print('%s -> %s  (%d verts, %d tris)' % (src, dst, nv, nt))
