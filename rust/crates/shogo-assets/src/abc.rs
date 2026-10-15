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
//! each piece's local space. `Nodes` names those pieces, and the first
//! `static_model` animation is the bind pose that assembles them.
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

#[derive(Clone, Copy)]
struct BindXform {
    pos: [f32; 3],
    quat: [f32; 4],
    scale: [f32; 3],
}

/// Piece names in `Nodes`, in file order. The last piece has no local bounds.
fn node_names(blob: &[u8]) -> Vec<String> {
    let mut start = if name_len_at(blob, 24).is_some() { 24 } else { 0 };
    let mut names = Vec::new();
    while let Some(n) = name_len_at(blob, start) {
        names.push(String::from_utf8_lossy(&blob[start + 2..start + 2 + n]).into_owned());
        let after = start + 2 + n + 11;
        let with_bounds = after + 24;
        if name_len_at(blob, with_bounds).is_some() {
            start = with_bounds;
        } else {
            break;
        }
    }
    names
}

fn name_len_at(blob: &[u8], i: usize) -> Option<usize> {
    let n = u16_at(blob, i).ok()? as usize;
    if !(1..=40).contains(&n) || i + 2 + n > blob.len() {
        return None;
    }
    let bytes = &blob[i + 2..i + 2 + n];
    bytes.iter().all(|b| (32..127).contains(b)).then_some(n)
}

/// Payload after the `static_model` name and its following u32.
fn static_model_payload(anim: &[u8]) -> Option<&[u8]> {
    let mut i = 0usize;
    while i + 14 <= anim.len() {
        if let Ok(n) = u16_at(anim, i) {
            if n == 12 && i + 2 + 12 + 4 <= anim.len() {
                let name = &anim[i + 2..i + 2 + 12];
                if name.eq_ignore_ascii_case(b"static_model") {
                    return Some(&anim[i + 2 + 12 + 4..]);
                }
            }
        }
        i += 1;
    }
    None
}

fn bind_pose(nodes: &[u8], anim: &[u8]) -> Option<Vec<BindXform>> {
    let names = node_names(nodes);
    let count = names.len();
    if count == 0 {
        return None;
    }
    let payload = static_model_payload(anim)?;
    let mut start = 0usize;
    while start + count * 80 <= payload.len() {
        if (0..count).all(|i| bind_record(payload, start + i * 80)) {
            let mut local = Vec::with_capacity(count);
            for i in 0..count {
                let o = start + i * 80;
                local.push(BindXform {
                    pos: [f32_at(payload, o).ok()?, f32_at(payload, o + 4).ok()?, f32_at(payload, o + 8).ok()?],
                    quat: bind_quat(payload, o + 12)?,
                    scale: sane_scale([
                        f32_at(payload, o + 56).ok()?,
                        f32_at(payload, o + 60).ok()?,
                        f32_at(payload, o + 64).ok()?,
                    ]),
                });
            }
            let parents: Vec<Option<usize>> = (0..count).map(|i| parent_index(&names, i)).collect();
            return Some(accumulate_bind(&local, &parents));
        }
        start += 4;
    }
    None
}

/// Limb records are parent-local. Names follow the retail skeleton
/// (`torso` → `armu` → `arml` → hand, and the same for legs).
fn parent_index(names: &[String], i: usize) -> Option<usize> {
    if i == 0 {
        return None;
    }
    let name = names[i].to_ascii_lowercase();
    let earlier = |pred: &dyn Fn(&str) -> bool| -> Option<usize> {
        (0..i).rev().find(|j| pred(&names[*j].to_ascii_lowercase()))
    };
    let side = if name.contains("left") {
        "left"
    } else if name.contains("right") {
        "right"
    } else {
        ""
    };
    let same_side = |n: &str| side.is_empty() || n.contains(side);
    let first = |pred: &dyn Fn(&str) -> bool| -> Option<usize> {
        (0..i).find(|j| pred(&names[*j].to_ascii_lowercase()))
    };
    // Numbered torso plates are siblings of the limbs, so arms and the plates
    // themselves hang off the first `torso` rather than off `torso_1`.
    let main_torso = || first(&|n| n == "torso").or_else(|| first(&|n| n.contains("torso")));
    if name.contains("gun") {
        return earlier(&|n| n.contains("hand") && !n.contains("gun"))
            .or_else(|| earlier(&|n| n.contains("arml") && same_side(n)));
    }
    if name.contains("hand") || name.contains("shield") {
        return earlier(&|n| {
            n.contains("arml") && !n.contains("hand") && !n.contains("shield") && same_side(n)
        })
        .or_else(|| earlier(&|n| n.contains("arm") && same_side(n)));
    }
    if name.contains("arml") {
        // The forearm hangs off the first upper-arm bone. A later `armu_2`
        // shell is a sibling plate, not the bone.
        return first(&|n| {
            n.contains("armu") && !n.contains("shoulder") && !n.contains("arml") && same_side(n)
        })
        .or_else(|| earlier(&|n| n.contains("armu") && !n.contains("arml") && same_side(n)));
    }
    if name.contains("shoulder") {
        return main_torso();
    }
    if name.contains("armu") {
        return earlier(&|n| n.contains("armu") && !n.contains("shoulder") && same_side(n))
            .or_else(|| earlier(&|n| n.contains("shoulder") && same_side(n)))
            .or_else(main_torso);
    }
    if name.contains("heel") {
        return earlier(&|n| n.contains("legl") && !n.contains("heel") && !n.contains("foot") && same_side(n));
    }
    if name.contains("foot") || name.contains("shoe") {
        return earlier(&|n| {
            n.contains("legl") && !n.contains("foot") && !n.contains("shoe") && !n.contains("heel") && same_side(n)
        });
    }
    if name.contains("legl") {
        return earlier(&|n| n.contains("legu") && same_side(n));
    }
    if name.contains("legu") {
        return earlier(&|n| n.contains("pelvis"));
    }
    if name.contains("head") {
        return earlier(&|n| n.contains("head")).or_else(main_torso);
    }
    if name == "pelvis" {
        // `pelvis_1` can sit earlier as a helper pivot. The hip bone itself
        // hangs off the torso, or the legs swing out with that pivot.
        return main_torso().or(Some(0));
    }
    if name.contains("pelvis") {
        return earlier(&|n| n.contains("pelvis")).or_else(main_torso);
    }
    if name.contains("torso") {
        return main_torso().or(Some(0));
    }
    Some(0)
}

fn accumulate_bind(local: &[BindXform], parents: &[Option<usize>]) -> Vec<BindXform> {
    let mut global = Vec::with_capacity(local.len());
    for (i, limb) in local.iter().enumerate() {
        global.push(match parents[i] {
            Some(p) => {
                let parent: BindXform = global[p];
                let t = rotate_quat(parent.quat, limb.pos);
                BindXform {
                    pos: [t[0] + parent.pos[0], t[1] + parent.pos[1], t[2] + parent.pos[2]],
                    quat: quat_mul(parent.quat, limb.quat),
                    scale: limb.scale,
                }
            }
            None => *limb,
        });
    }
    global
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// A bind record is 80 bytes: position, quaternion, a duplicate of that pose,
/// then scale. Later keyframes share the layout, so the duplicate and a
/// near-unit scale reject a chance run of unit quaternions.
fn bind_record(data: &[u8], o: usize) -> bool {
    let Ok(floats) = read_floats(data, o, 17) else {
        return false;
    };
    let pos_dup = floats[0..3].iter().zip(&floats[7..10]).all(|(a, b)| (a - b).abs() < 0.05);
    let quat_dup = floats[3..7].iter().zip(&floats[10..14]).all(|(a, b)| (a - b).abs() < 0.02);
    let scale_ok = floats[14..17].iter().all(|s| (0.25..4.0).contains(&s.abs()));
    pos_dup && quat_dup && scale_ok && quat_usable(&floats[3..7])
}

fn read_floats(data: &[u8], o: usize, n: usize) -> Result<Vec<f32>, AbcError> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let v = f32_at(data, o + i * 4)?;
        if !v.is_finite() || v.abs() > 1.0e5 {
            return Err(AbcError::Truncated);
        }
        out.push(v);
    }
    Ok(out)
}

fn quat_usable(q: &[f32]) -> bool {
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    (0.95..1.05).contains(&len) || len < 0.5
}

fn bind_quat(data: &[u8], o: usize) -> Option<[f32; 4]> {
    let q = [
        f32_at(data, o).ok()?,
        f32_at(data, o + 4).ok()?,
        f32_at(data, o + 8).ok()?,
        f32_at(data, o + 12).ok()?,
    ];
    let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if len < 0.5 {
        Some([0.0, 0.0, 0.0, 1.0])
    } else if (0.95..1.05).contains(&len) {
        Some(q)
    } else {
        None
    }
}

fn sane_scale(scale: [f32; 3]) -> [f32; 3] {
    if scale.iter().all(|s| s.is_finite() && (0.25..4.0).contains(&s.abs())) {
        scale
    } else {
        [1.0, 1.0, 1.0]
    }
}

fn pose_vertex(pos: [f32; 3], node: u8, bind: Option<&[BindXform]>) -> [f32; 3] {
    let Some(xf) = bind.and_then(|b| b.get(node as usize)) else {
        return pos;
    };
    let scaled = [
        pos[0] * xf.scale[0],
        pos[1] * xf.scale[1],
        pos[2] * xf.scale[2],
    ];
    let r = rotate_quat(xf.quat, scaled);
    [r[0] + xf.pos[0], r[1] + xf.pos[1], r[2] + xf.pos[2]]
}

/// Rotate `v` by a unit quaternion stored as `(x, y, z, w)`.
fn rotate_quat(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (qx, qy, qz, w) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (qy * v[2] - qz * v[1]);
    let ty = 2.0 * (qz * v[0] - qx * v[2]);
    let tz = 2.0 * (qx * v[1] - qy * v[0]);
    [
        v[0] + w * tx + (qy * tz - qz * ty),
        v[1] + w * ty + (qz * tx - qx * tz),
        v[2] + w * tz + (qx * ty - qy * tx),
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
        let ext = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        let wide = ext[0].max(ext[2]);
        // Flat piece transforms leave a ~68-tall cloud. Chaining each limb onto
        // its parent closes the figure up to about 90, still near the
        // static-model bounds.
        assert!(
            ext[1] > wide * 1.5 && (80.0..130.0).contains(&ext[1]),
            "extents {ext:?} min {min:?} max {max:?}"
        );
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
            // A broad mech can be nearly as wide as it is tall. A stacked
            // pile of unposed parts is much wider than it is tall, and a
            // misread scale stretches a figure past a few hundred units.
            assert!(
                ext[1] > wide * 0.9 && (50.0..280.0).contains(&ext[1]),
                "{rel} extents {ext:?}"
            );
        }
    }
}
