#!/usr/bin/env python3
"""
abc2glb.py -- Export a LithTech .ABC model to glTF 2.0 binary (.glb).

Produces a single self-contained .glb with POSITION, NORMAL, TEXCOORD_0
and indices, using the parser in ltabc.py. Loads directly in Blender 2.8+
and in Bevy (via gltf crate).

Coordinate conversion (LithTech Z-up right-handed -> glTF Y-up right-handed):
    (x, y, z)_LT  ->  (x, z, -y)_glTF      [rotate -90 deg about X]
This is a proper rotation (det=+1), so triangle winding is preserved.
UV: LithTech uses top-left-origin V; glTF uses bottom-left. So v' = 1 - v.

Normals: computed smooth normals (area-weighted) by averaging over shared
positions, since the ABC format stores per-corner UVs (no vertex sharing).
"""
import struct, sys, os, json, base64, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ltabc


def lt_to_gltf(x, y, z):
    return (x, z, -y)


def compute_smooth_normals(positions, indices):
    """Area-weighted smooth normals averaged over identical positions."""
    normals = [[0.0, 0.0, 0.0] for _ in positions]
    for t in range(0, len(indices), 3):
        i0, i1, i2 = indices[t], indices[t+1], indices[t+2]
        ax, ay, az = positions[i0]
        bx, by, bz = positions[i1]
        cx, cy, cz = positions[i2]
        # edge vectors
        e1 = (bx-ax, by-ay, bz-az)
        e2 = (cx-ax, cy-ay, cz-az)
        # cross product (area-weighted: magnitude prop. to area)
        nx = e1[1]*e2[2] - e1[2]*e2[1]
        ny = e1[2]*e2[0] - e1[0]*e2[2]
        nz = e1[0]*e2[1] - e1[1]*e2[0]
        for i in (i0, i1, i2):
            normals[i][0] += nx; normals[i][1] += ny; normals[i][2] += nz
    # normalize
    out = []
    for (x, y, z) in normals:
        l = math.sqrt(x*x + y*y + z*z)
        if l > 1e-12:
            out.append((x/l, y/l, z/l))
        else:
            out.append((0.0, 0.0, 1.0))
    return out


def export_glb(mesh, out_path):
    # Build per-corner arrays. ltabc already expands per-corner (positions/uvs
    # aligned, indices reference corners). Apply axis + UV conversion.
    n = len(mesh.positions)
    positions = [lt_to_gltf(x, y, z) for (x, y, z) in mesh.positions]
    uvs = [(u, 1.0 - v) for (u, v) in mesh.uvs]
    normals = compute_smooth_normals(positions, mesh.indices)

    # bounds
    xs = [p[0] for p in positions]; ys = [p[1] for p in positions]; zs = [p[2] for p in positions]
    mn = (min(xs), min(ys), min(zs)); mx = (max(xs), max(ys), max(zs))

    # pack binary: POSITION (vec3 float), NORMAL (vec3 float), TEXCOORD_0 (vec2 float), indices (u32)
    pos_bytes = b''.join(struct.pack('<3f', *p) for p in positions)
    nrm_bytes = b''.join(struct.pack('<3f', *v) for v in normals)
    uv_bytes  = b''.join(struct.pack('<2f', *t) for t in uvs)
    idx_bytes = b''.join(struct.pack('<I', i) for i in mesh.indices)

    # align each view to 4 bytes
    def pad4(b):
        return b + b'\x00' * ((4 - len(b) % 4) % 4)
    pos_b = pad4(pos_bytes); nrm_b = pad4(nrm_bytes); uv_b = pad4(uv_bytes); idx_b = pad4(idx_bytes)

    bin_chunk = pos_b + nrm_b + uv_b + idx_b
    off = 0
    views = []
    for b, target in ((pos_b, 34962), (nrm_b, 34962), (uv_b, 34962), (idx_b, 34963)):
        views.append({"buffer": 0, "byteOffset": off, "byteLength": len(b),
                      "target": target})
        off += len(b)

    gltf = {
        "asset": {"version": "2.0", "generator": "shogo-mad abc2glb"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": mesh.name}],
        "meshes": [{"name": mesh.name, "primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
            "indices": 3, "material": 0, "mode": 4}]}],
        "materials": [{
            "name": mesh.material or (mesh.name + "_mat"),
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0, "roughnessFactor": 1.0}}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": n, "type": "VEC3",
             "min": list(mn), "max": list(mx)},
            {"bufferView": 1, "componentType": 5126, "count": n, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": n, "type": "VEC2"},
            {"bufferView": 3, "componentType": 5125, "count": len(mesh.indices),
             "type": "SCALAR"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": len(bin_chunk)}],
    }

    json_bytes = json.dumps(gltf, separators=(',', ':')).encode('utf-8')
    json_bytes += b' ' * ((4 - len(json_bytes) % 4) % 4)
    bin_bytes = pad4(bin_chunk)

    total = 12 + 8 + len(json_bytes) + 8 + len(bin_bytes)
    with open(out_path, 'wb') as f:
        f.write(struct.pack('<4sII', b'glTF', 2, total))
        f.write(struct.pack('<I', len(json_bytes)))
        f.write(b'JSON')
        f.write(json_bytes)
        f.write(struct.pack('<I', len(bin_bytes)))
        f.write(b'BIN\x00')
        f.write(bin_bytes)
    return n, len(mesh.indices) // 3


def convert_file(src, dst):
    mesh = ltabc.load(src)
    if mesh.tri_count == 0:
        raise ValueError("0 triangles")
    nv, nt = export_glb(mesh, dst)
    return nv, nt


if __name__ == '__main__':
    if len(sys.argv) >= 4 and sys.argv[1] == '--batch':
        indir, outdir = sys.argv[2], sys.argv[3]
        os.makedirs(outdir, exist_ok=True)
        ok = fail = 0
        fails = []
        for root, _, files in os.walk(indir):
            for fn in files:
                if not fn.lower().endswith('.abc'):
                    continue
                src = os.path.join(root, fn)
                rel = os.path.relpath(src, indir)
                dst = os.path.join(outdir, os.path.splitext(rel)[0] + '.glb')
                os.makedirs(os.path.dirname(dst) or '.', exist_ok=True)
                try:
                    convert_file(src, dst); ok += 1
                except Exception as e:
                    fail += 1; fails.append((rel, str(e)))
        print("OK: %d   FAIL: %d" % (ok, fail))
        for rel, msg in fails[:15]:
            print("  %-50s %s" % (rel, msg))
    else:
        src = sys.argv[1]
        dst = sys.argv[2] if len(sys.argv) > 2 else os.path.splitext(src)[0] + '.glb'
        nv, nt = convert_file(src, dst)
        print("%s -> %s  (%d verts, %d tris)" % (src, dst, nv, nt))
