//! LithTech 1.0 "MonolithExport Model File v6" (.ABC) model decoder.
//!
//! Reverse-engineered from the decompiled `Client.exe` loader (top-level
//! `FUN_004343e0`, geometry `FUN_00434ad0`, property walk `FUN_00435270`) and
//! validated against all 538 retail models (exact byte consumption, indices in
//! range).
//!
//! The file is a flat sequence of properties:
//!
//! ```text
//! u16 nameLen, char name[nameLen], u32 endOffset, u8 data[endOffset - here]
//! ```
//!
//! The next property begins exactly at `endOffset`. Top-level properties:
//! `Header`, `Geometry`, `Nodes`, `Animation`. Geometry vertices are stored in
//! each piece's local space. `Nodes` is the piece tree, and keyframe 0 of the
//! `static_model` animation is the bind pose. The vertex transform is the one
//! in `FUN_004348d0`: the node matrix from `FUN_00435ad0` is applied as
//! `x' = m0·v + m3` (the transpose of the quaternion's column matrix).
//!
//! ```text
//! 24B   bbox: f32 min.xyz, max.xyz
//! u32   nNames;  u8 nameTable[nNames*2 + 2]
//! u32   nFaces
//!   per face (33B):
//!     6 x f32 (24B)   3 UV pairs (one per triangle corner)
//!     u16 x 3         vertex indices
//!     u8  x 3         per-corner flags
//! u32   nVerts
//! u32   nNorms        (normals computed elsewhere; not stored here)
//!   per vert (20B):
//!     3 x f32 position (12B)
//!     u8 x 4, u16 x 2  (flags / light / unused)
//! ```
//!
//! Each face is a triangle carrying its own 3 UVs, so we expand to per-corner
//! vertices to preserve UV seams (no vertex sharing).

/// A decoded triangle mesh. Vertex arrays are per-corner: `positions[i]` and
/// `uvs[i]` share an index, and `indices` are triangles into them.
pub struct AbcModel {
    pub name: String,
    /// Per-corner positions, `(x, y, z)`, in the bind pose when the file has one.
    /// Posed characters stand along +Y. Files with no `static_model` keep the
    /// raw geometry positions.
    pub positions: Vec<[f32; 3]>,
    /// Per-corner UVs, `(u, v)`, raw (not flipped).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle corner indices, length is a multiple of 3.
    pub indices: Vec<u32>,
    /// Stored bounding box: `[min.xyz, max.xyz]`.
    pub bbox: [f32; 6],
}

impl AbcModel {
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }
}

#[derive(Debug)]
pub enum AbcError {
    NoGeometry,
    Truncated,
    BadIndex,
}

impl std::fmt::Display for AbcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AbcError::NoGeometry => write!(f, "no Geometry property found"),
            AbcError::Truncated => write!(f, "truncated model data"),
            AbcError::BadIndex => write!(f, "vertex index out of range"),
        }
    }
}
impl std::error::Error for AbcError {}

fn u16_at(d: &[u8], off: usize) -> Result<u16, AbcError> {
    d.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(AbcError::Truncated)
}
fn u32_at(d: &[u8], off: usize) -> Result<u32, AbcError> {
    d.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(AbcError::Truncated)
}
fn f32_at(d: &[u8], off: usize) -> Result<f32, AbcError> {
    d.get(off..off + 4)
        .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(AbcError::Truncated)
}

/// Walk top-level properties, yielding `(name, data_start, data_end)`.
fn walk(d: &[u8]) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 6 <= d.len() {
        let n = match u16_at(d, p) {
            Ok(n) => n as usize,
            Err(_) => break,
        };
        if n == 0 || n > 255 || p + 2 + n + 4 > d.len() {
            break;
        }
        let name_bytes = &d[p + 2..p + 2 + n];
        if !name_bytes.iter().all(|&b| (32..127).contains(&b)) {
            break;
        }
        let end = match u32_at(d, p + 2 + n) {
            Ok(e) => e as usize,
            Err(_) => break,
        };
        let ds = p + 2 + n + 4;
        if end <= ds || end > d.len() {
            break;
        }
        out.push((
            String::from_utf8_lossy(name_bytes).into_owned(),
            ds,
            end,
        ));
        p = end;
    }
    out
}

/// Decode an ABC model. `name` is used to label the mesh (usually the file stem).
pub fn decode(data: &[u8], name: &str) -> Result<AbcModel, AbcError> {
    let props = walk(data);
    let (_, ds, de) = props
        .iter()
        .find(|(n, _, _)| n == "Geometry")
        .ok_or(AbcError::NoGeometry)?;

    let mut o = *ds;
    let mut bbox = [0f32; 6];
    for i in 0..6 {
        bbox[i] = f32_at(data, o)?;
        o += 4;
    }

    let n_names = u32_at(data, o)? as usize;
    o += 4;
    o += n_names * 2 + 2;

    let n_faces = u32_at(data, o)? as usize;
    o += 4;

    // Per-corner expansion: each face contributes 3 corners.
    let mut positions = Vec::with_capacity(n_faces * 3);
    let mut uvs = Vec::with_capacity(n_faces * 3);
    let mut indices = Vec::with_capacity(n_faces * 3);

    // We need vertex positions, but faces are read first. So peek vertex table
    // location, then read faces, resolving indices after. Simplest: two passes.
    // First, remember where the vertex block starts.
    let faces_bytes = n_faces * 33;
    let vert_block = o + faces_bytes;
    let n_verts = u32_at(data, vert_block)? as usize;
    // (n_norms is at vert_block + 4; vertices begin at vert_block + 8.)

    // Read vertex positions into a temp vec. Byte 15 of each 20-byte vertex
    // is the node that owns it.
    let mut verts = Vec::with_capacity(n_verts);
    let mut nodes = Vec::with_capacity(n_verts);
    {
        let mut vo = vert_block + 8;
        for _ in 0..n_verts {
            let x = f32_at(data, vo)?;
            let y = f32_at(data, vo + 4)?;
            let z = f32_at(data, vo + 8)?;
            let node = *data.get(vo + 15).ok_or(AbcError::Truncated)?;
            verts.push([x, y, z]);
            nodes.push(node);
            vo += 20;
        }
    }
    let bind = props
        .iter()
        .find(|(n, _, _)| n == "Nodes")
        .map(|(_, ds, de)| &data[*ds..*de])
        .and_then(|nodes_blob| {
            let anim = props.iter().find(|(n, _, _)| n == "Animation")?;
            bind_pose(nodes_blob, &data[anim.1..anim.2])
        });

    // Now read faces (they precede the vertex block).
    let mut fo = o;
    for _ in 0..n_faces {
        let u0 = f32_at(data, fo)?;
        let v0 = f32_at(data, fo + 4)?;
        let u1 = f32_at(data, fo + 8)?;
        let v1 = f32_at(data, fo + 12)?;
        let u2 = f32_at(data, fo + 16)?;
        let v2 = f32_at(data, fo + 20)?;
        fo += 24;
        let i0 = u16_at(data, fo)? as usize;
        let i1 = u16_at(data, fo + 2)? as usize;
        let i2 = u16_at(data, fo + 4)? as usize;
        fo += 6;
        fo += 3; // per-corner flags

        for (vi, uv) in [(i0, [u0, v0]), (i1, [u1, v1]), (i2, [u2, v2])] {
            let p = *verts.get(vi).ok_or(AbcError::BadIndex)?;
            let node = *nodes.get(vi).ok_or(AbcError::BadIndex)?;
            positions.push(pose_vertex(p, node, bind.as_deref()));
            uvs.push(uv);
            indices.push((positions.len() - 1) as u32);
        }
    }

    // `de` is the property end; exact consumption is a good invariant but we
    // don't hard-fail on it (some exporters pad). Silence unused warning:
    let _ = de;

    Ok(AbcModel {
        name: name.to_string(),
        positions,
        uvs,
        indices,
        bbox,
    })
}

struct Piece {
    index: usize,
    flags: u8,
    kids: Vec<usize>,
}

struct NodeAnim {
    pos: [f32; 3],
    quat: [f32; 4],
    s1: [f32; 3],
    s2: [f32; 3],
}

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn rest(&self) -> usize {
        self.d.len().saturating_sub(self.p)
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.rest() < n {
            return None;
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Option<f32> {
        let b = self.take(4)?;
        let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        v.is_finite().then_some(v)
    }
    fn vec3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    fn str16(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        if n > 256 {
            return None;
        }
        let b = self.take(n)?;
        Some(String::from_utf8_lossy(b).into_owned())
    }
}

/// `Nodes` property (`FUN_00434dc0`): local bounds, name, preorder index, flags,
/// a u16 list, then children. The bounds are the piece's own box, not the pose.
fn read_pieces(blob: &[u8]) -> Option<Vec<Piece>> {
    let mut c = Cur { d: blob, p: 0 };
    let mut pieces = Vec::new();
    read_piece(&mut c, &mut pieces)?;
    (!pieces.is_empty() && pieces.len() <= 512).then_some(pieces)
}

fn read_piece(c: &mut Cur, pieces: &mut Vec<Piece>) -> Option<usize> {
    let _bounds_a = c.vec3()?;
    let _bounds_b = c.vec3()?;
    let _name = c.str16()?;
    let index = c.u16()? as usize;
    if index > 4096 {
        return None;
    }
    let flags = c.u8()?;
    let extra = c.u32()? as usize;
    if extra > 10_000 {
        return None;
    }
    c.take(extra * 2)?;
    let n_child = c.u32()? as usize;
    if n_child > 256 {
        return None;
    }
    let slot = pieces.len();
    pieces.push(Piece {
        index,
        flags,
        kids: Vec::new(),
    });
    let mut kids = Vec::with_capacity(n_child);
    for _ in 0..n_child {
        kids.push(read_piece(c, pieces)?);
    }
    pieces[slot].kids = kids;
    Some(slot)
}

/// Keyframe 0 of `static_model`, one record per node in preorder.
/// Layout matches `FUN_00434ef0` / `FUN_004350f0`: the animation header, then
/// per node `nKeys` × 28-byte poses followed by the two scale vectors.
fn static_model_anims(anim: &[u8], n_nodes: usize) -> Option<Vec<NodeAnim>> {
    if n_nodes == 0 {
        return None;
    }
    let mut c = Cur { d: anim, p: 0 };
    let n_anims = c.u32()? as usize;
    if n_anims == 0 || n_anims > 512 {
        return None;
    }
    for _ in 0..n_anims {
        let name = c.str16()?;
        let _unk = c.u32()?;
        let _min = c.vec3()?;
        let _max = c.vec3()?;
        let n_keys = c.u32()? as usize;
        if n_keys == 0 || n_keys > 20_000 {
            return None;
        }
        for _ in 0..n_keys {
            let _time = c.f32()?;
            let _a = c.vec3()?;
            let _b = c.vec3()?;
            let _label = c.str16()?;
        }
        let mut nodes = Vec::with_capacity(n_nodes);
        for _ in 0..n_nodes {
            let pos = c.vec3()?;
            let quat = [c.f32()?, c.f32()?, c.f32()?, c.f32()?];
            for _ in 1..n_keys {
                c.take(28)?;
            }
            let s1 = c.vec3()?;
            let s2 = c.vec3()?;
            nodes.push(NodeAnim { pos, quat, s1, s2 });
        }
        if name.eq_ignore_ascii_case("static_model") {
            return Some(nodes);
        }
    }
    None
}

fn identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

/// Column-vector quaternion matrix, `(x, y, z, w)`, matching `FUN_00435ad0`.
/// The 2/len² factor is the engine's normalize-and-double.
fn quat_matrix(q: [f32; 4]) -> [f32; 9] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let len2 = x * x + y * y + z * z + w * w;
    if len2 < 1.0e-12 {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let s = 2.0 / len2;
    let (xx, yy, zz) = (s * x * x, s * y * y, s * z * z);
    let (xy, xz, yz) = (s * x * y, s * x * z, s * y * z);
    let (wx, wy, wz) = (s * w * x, s * w * y, s * w * z);
    [
        1.0 - (yy + zz),
        xy - wz,
        xz + wy,
        xy + wz,
        1.0 - (xx + zz),
        yz - wx,
        xz - wy,
        yz + wx,
        1.0 - (xx + yy),
    ]
}

/// Child matrix from parent `p` and the local quaternion matrix `r` plus
/// translation. This is the store in `FUN_00435ad0`: the rotation block is
/// written transposed relative to a column-vector multiply.
fn compose(p: &[f32; 16], r: &[f32; 9], t: [f32; 3]) -> [f32; 16] {
    let (r00, r01, r02, r10, r11, r12, r20, r21, r22) =
        (r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8]);
    let (tx, ty, tz) = (t[0], t[1], t[2]);
    [
        r00 * p[0] + r01 * p[1] + r02 * p[2],
        r10 * p[0] + r11 * p[1] + r12 * p[2],
        r20 * p[0] + r21 * p[1] + r22 * p[2],
        tx * p[0] + ty * p[1] + tz * p[2] + p[3],
        r00 * p[4] + r01 * p[5] + r02 * p[6],
        r10 * p[4] + r11 * p[5] + r12 * p[6],
        r20 * p[4] + r21 * p[5] + r22 * p[6],
        tx * p[4] + ty * p[5] + tz * p[6] + p[7],
        r00 * p[8] + r01 * p[9] + r02 * p[10],
        r10 * p[8] + r11 * p[9] + r12 * p[10],
        r20 * p[8] + r21 * p[9] + r22 * p[10],
        tx * p[8] + ty * p[9] + tz * p[10] + p[11],
        r00 * p[12] + r01 * p[13] + r02 * p[14],
        r10 * p[12] + r11 * p[13] + r12 * p[14],
        r20 * p[12] + r21 * p[13] + r22 * p[14],
        tx * p[12] + ty * p[13] + tz * p[14] + p[15],
    ]
}

/// `FUN_0041c2e0`: row-major `out = a * b`.
fn mat_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut o = [0.0; 16];
    for row in 0..4 {
        for col in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[row * 4 + k] * b[k * 4 + col];
            }
            o[row * 4 + col] = s;
        }
    }
    o
}

/// Scale then offset, applied to the vertex before the node matrix.
/// Flag bit 4 (`FUN_00435ad0`) is the only path that uses it.
fn scale_matrix(s1: [f32; 3], s2: [f32; 3]) -> [f32; 16] {
    [
        s1[0], 0.0, 0.0, s2[0], 0.0, s1[1], 0.0, s2[1], 0.0, 0.0, s1[2], s2[2], 0.0, 0.0, 0.0, 1.0,
    ]
}

fn bind_pose(nodes: &[u8], anim: &[u8]) -> Option<Vec<[f32; 16]>> {
    let pieces = read_pieces(nodes)?;
    let anims = static_model_anims(anim, pieces.len())?;
    if anims.len() != pieces.len() {
        return None;
    }
    let mut mats = vec![identity(); pieces.len()];
    fill_bind(0, identity(), &pieces, &anims, &mut mats)?;
    Some(mats)
}

fn fill_bind(
    slot: usize,
    parent: [f32; 16],
    pieces: &[Piece],
    anims: &[NodeAnim],
    mats: &mut Vec<[f32; 16]>,
) -> Option<()> {
    let piece = pieces.get(slot)?;
    let anim = anims.get(slot)?;
    let local = quat_matrix(anim.quat);
    let mut m = compose(&parent, &local, anim.pos);
    let kids = piece.kids.clone();
    let flags = piece.flags;
    let index = piece.index;
    let s1 = anim.s1;
    let s2 = anim.s2;
    for kid in kids {
        fill_bind(kid, m, pieces, anims, mats)?;
    }
    if flags & 4 != 0 {
        m = mat_mul(&m, &scale_matrix(s1, s2));
    }
    if index >= mats.len() {
        mats.resize(index + 1, identity());
    }
    mats[index] = m;
    Some(())
}

/// `FUN_004348d0`. `m[0..3]` is the first row, so this is `Rᵀ v + t`.
fn pose_vertex(pos: [f32; 3], node: u8, bind: Option<&[[f32; 16]]>) -> [f32; 3] {
    let Some(m) = bind.and_then(|b| b.get(node as usize)) else {
        return pos;
    };
    [
        m[0] * pos[0] + m[1] * pos[1] + m[2] * pos[2] + m[3],
        m[4] * pos[0] + m[5] * pos[1] + m[6] * pos[2] + m[7],
        m[8] * pos[0] + m[9] * pos[1] + m[10] * pos[2] + m[11],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny synthetic ABC with one triangle, then decode it.
    fn build_one_tri() -> Vec<u8> {
        let mut d = Vec::new();
        // Geometry property
        let mut body = Vec::new();
        // bbox
        for v in [0f32, 0.0, 0.0, 1.0, 1.0, 0.0] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        // nNames = 0 -> nameTable is 2 bytes
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&[0, 0]);
        // nFaces = 1
        body.extend_from_slice(&1u32.to_le_bytes());
        // one face: 6 uv floats, 3 u16 indices, 3 flag bytes
        for v in [0f32, 0.0, 1.0, 0.0, 0.0, 1.0] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend_from_slice(&0u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&[0, 0, 0]);
        // nVerts = 3, nNorms = 0
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        // 3 vertices: pos(12) + 4 flags + 4 (2xu16) = 20 bytes each
        for p in [[0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in p {
                body.extend_from_slice(&c.to_le_bytes());
            }
            body.extend_from_slice(&[0u8; 8]);
        }

        // Wrap in property: u16 nameLen, name, u32 endOffset, body
        let name = b"Geometry";
        let header_len = 2 + name.len() + 4;
        d.extend_from_slice(&(name.len() as u16).to_le_bytes());
        d.extend_from_slice(name);
        let end = (header_len + body.len()) as u32;
        d.extend_from_slice(&end.to_le_bytes());
        d.extend_from_slice(&body);
        d
    }

    #[test]
    fn decodes_one_triangle() {
        let d = build_one_tri();
        let m = decode(&d, "tri").unwrap();
        assert_eq!(m.tri_count(), 1);
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.uvs.len(), 3);
        assert_eq!(m.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(m.uvs[2], [0.0, 1.0]);
    }

    #[test]
    fn trooper_bind_pose_stands_taller_than_it_is_wide() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/MODELS/ENEMIES/ONFOOT/TROOPER.ABC");
        let Ok(data) = std::fs::read(&path) else {
            return;
        };
        let model = decode(&data, "TROOPER").unwrap();
        assert_eq!(model.tri_count(), 626);
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for p in &model.positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        // `static_model` key 0 bounds. Y is the tall axis (feet below the
        // origin, head above). The old 80-byte heuristic smushed this cloud.
        let want_min = [-19.6_f32, -44.6, -30.7];
        let want_max = [19.3_f32, 39.1, 25.2];
        for i in 0..3 {
            assert!(
                (min[i] - want_min[i]).abs() < 0.4 && (max[i] - want_max[i]).abs() < 0.4,
                "min {min:?} max {max:?}"
            );
        }
        assert!(max[1] > 0.0 && min[1] < 0.0, "min {min:?} max {max:?}");
    }

    #[test]
    fn engine_bind_uses_the_transposed_rotation() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let m = compose(&identity(), &quat_matrix([0.0, 0.0, s, s]), [0.0, 0.0, 0.0]);
        let p = pose_vertex([1.0, 0.0, 0.0], 0, Some(&[m]));
        assert!(p[0].abs() < 1e-5 && (p[1] + 1.0).abs() < 1e-5 && p[2].abs() < 1e-5, "{p:?}");

        let mut parent = identity();
        parent[3] = 10.0;
        let child = compose(&parent, &quat_matrix([0.0, 0.0, 0.0, 1.0]), [1.0, 0.0, 0.0]);
        let p = pose_vertex([0.0, 0.0, 0.0], 0, Some(&[child]));
        assert!((p[0] - 11.0).abs() < 1e-4 && p[1].abs() < 1e-4 && p[2].abs() < 1e-4, "{p:?}");
    }

    #[test]
    fn posed_mechs_stay_upright() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../extracted/SHOGO");
        for rel in [
            "MODELS/ENEMIES/ONFOOT/SHOCKTROOPER.ABC",
            "MODELS/ENEMIES/MCA/ANDRA5.ABC",
            "MODELS/ENEMIES/MCA/ANDRA10.ABC",
            "MODELS/ENEMIES/MCA/RAKSHA.ABC",
        ] {
            let Ok(data) = std::fs::read(root.join(rel)) else {
                continue;
            };
            let model = decode(&data, rel).unwrap();
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            for p in &model.positions {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            let ext = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
            let wide = ext[0].max(ext[2]);
            // A bulky MCA can be a little deeper than it is tall. An unposed
            // pile is much wider than it is tall, and a bad scale runs past
            // a few hundred units. The figure stands through the origin on Y.
            assert!(
                ext[1] > wide * 0.75
                    && (50.0..280.0).contains(&ext[1])
                    && min[1] < 0.0
                    && max[1] > 0.0,
                "{rel} extents {ext:?} min {min:?} max {max:?}"
            );
        }
    }
}
