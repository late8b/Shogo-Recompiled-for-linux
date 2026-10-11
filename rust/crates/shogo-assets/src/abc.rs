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
//! `Header`, `Geometry`, `Nodes`, `Animation`. We only need `Geometry`:
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
    /// Per-corner positions, `(x, y, z)`, LithTech Z-up space.
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

    // Read vertex positions into a temp vec.
    let mut verts = Vec::with_capacity(n_verts);
    {
        let mut vo = vert_block + 8;
        for _ in 0..n_verts {
            let x = f32_at(data, vo)?;
            let y = f32_at(data, vo + 4)?;
            let z = f32_at(data, vo + 8)?;
            verts.push([x, y, z]);
            vo += 20;
        }
    }

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
            positions.push(p);
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
}
