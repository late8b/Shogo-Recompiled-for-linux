//! LithTech 1.0 (DAT v56) world decoder — Shogo / Blood II WorldBSP.
//!
//! Layout matches `tools/ltdat.py` and haekb `bspv56.bt`, checked against all
//! 60 retail Shogo worlds. This reads the root WorldModel only (the bulk of
//! the level) and expands polygons into per-texture triangle groups.
//!
//! UVs are LithTech OPQ / world mapping, not stored per corner:
//! `u = P·pos + O.x`, `v = Q·pos + O.y`. Positions stay in LithTech Z-up space.

/// One texture's triangles. Positions and UVs are per-corner.
pub struct DatGroup {
    /// Index into [`DatWorld::textures`].
    pub texture: u16,
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices into `positions`.
    pub indices: Vec<u32>,
}

/// Decoded root world mesh.
pub struct DatWorld {
    pub name: String,
    /// `.dtx` paths as stored in the file (mixed case, backslashes).
    pub textures: Vec<String>,
    pub groups: Vec<DatGroup>,
    pub point_count: usize,
    pub poly_count: usize,
    pub surface_count: usize,
    /// Axis-aligned bounds of the point cloud, LithTech space.
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl DatWorld {
    pub fn tri_count(&self) -> usize {
        self.groups.iter().map(|g| g.indices.len() / 3).sum()
    }
}

#[derive(Debug)]
pub enum DatError {
    Truncated,
    BadHeader(&'static str),
}

impl std::fmt::Display for DatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatError::Truncated => write!(f, "truncated DAT"),
            DatError::BadHeader(m) => write!(f, "bad DAT header: {m}"),
        }
    }
}
impl std::error::Error for DatError {}

struct R<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn need(&self, n: usize) -> Result<(), DatError> {
        if self.p + n > self.d.len() {
            Err(DatError::Truncated)
        } else {
            Ok(())
        }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DatError> {
        self.need(n)?;
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DatError> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, DatError> {
        let b = self.take(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }
    fn u16(&mut self) -> Result<u16, DatError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn i32(&mut self) -> Result<i32, DatError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u32(&mut self) -> Result<u32, DatError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Result<f32, DatError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn vec3(&mut self) -> Result<[f32; 3], DatError> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    fn cstr(&mut self, maxn: usize) -> Result<String, DatError> {
        let mut out = Vec::new();
        for _ in 0..maxn {
            let c = self.u8()?;
            if c == 0 {
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            out.push(c);
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
    fn ltstr(&mut self) -> Result<String, DatError> {
        let n = self.i16()?;
        if n < 0 || n > 4096 {
            return Err(DatError::BadHeader("string length"));
        }
        let b = self.take(n as usize)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
}

struct Surface {
    o: [f32; 3],
    p: [f32; 3],
    q: [f32; 3],
    texture: u16,
}

struct Polygon {
    surface: i32,
    indices: Vec<i16>,
}

fn opq_uv(pos: [f32; 3], s: &Surface) -> [f32; 2] {
    let u = s.p[0] * pos[0] + s.p[1] * pos[1] + s.p[2] * pos[2] + s.o[0];
    let v = s.q[0] * pos[0] + s.q[1] * pos[1] + s.q[2] * pos[2] + s.o[1];
    [u, v]
}

fn read_leaves(r: &mut R, n: u32) -> Result<(), DatError> {
    for _ in 0..n {
        let llc = r.u16()?;
        if llc == 0xffff {
            r.u16()?;
        } else {
            if llc > 4000 {
                return Err(DatError::BadHeader("leaf list"));
            }
            for _ in 0..llc {
                r.u16()?;
                let sz = r.u16()? as usize;
                if sz > 20000 {
                    return Err(DatError::BadHeader("leaf data"));
                }
                r.take(sz)?;
            }
        }
        let pc = r.u16()? as usize;
        if pc > 8000 {
            return Err(DatError::BadHeader("leaf polys"));
        }
        r.take(pc * 4)?;
        r.f32()?;
    }
    Ok(())
}

/// Decode the root WorldBSP of a DAT v56 file into triangle groups.
pub fn decode(data: &[u8]) -> Result<DatWorld, DatError> {
    if data.len() < 16 {
        return Err(DatError::Truncated);
    }
    let ver = u32_at(data, 0);
    let geo = u32_at(data, 8) as usize;
    if ver != 56 {
        return Err(DatError::BadHeader("version"));
    }
    if geo + 40 > data.len() {
        return Err(DatError::BadHeader("geometry offset"));
    }

    let mut r = R { d: data, p: geo };
    let _next_section = r.u32()?;
    for _ in 0..8 {
        r.u32()?;
    }
    let raw_name = r.take(2)?;
    let name = String::from_utf8_lossy(raw_name.split(|&b| b == 0).next().unwrap_or(b"")).into_owned();

    let mut ints = [0u32; 12];
    for i in &mut ints {
        *i = r.u32()?;
    }
    let _unknown2 = r.u32()?;
    let _bmin = r.vec3()?;
    let _bmax = r.vec3()?;
    let _xlt = r.vec3()?;
    let table_bytes = r.u32()? as usize;
    let ntex = r.u32()? as usize;
    if ntex == 0 || ntex >= 4000 || table_bytes >= 200_000 {
        return Err(DatError::BadHeader("texture table"));
    }
    let table_start = r.p;
    let mut textures = Vec::with_capacity(ntex);
    for _ in 0..ntex {
        textures.push(r.cstr(200)?);
    }
    if r.p - table_start != table_bytes {
        return Err(DatError::BadHeader("texture table size"));
    }

    let point_count = ints[2] as usize;
    let plane_count = ints[3] as usize;
    let surface_count = ints[4] as usize;
    let portal_count = ints[5] as usize;
    let poly_count = ints[6] as usize;
    let leaf_count = ints[7];
    let node_count = ints[11] as usize;

    let mut vert_info = Vec::with_capacity(poly_count);
    for _ in 0..poly_count {
        vert_info.push((r.u8()?, r.u8()?));
    }
    read_leaves(&mut r, leaf_count)?;
    for _ in 0..plane_count {
        r.take(16)?;
    }

    let mut surfaces = Vec::with_capacity(surface_count);
    for _ in 0..surface_count {
        let o = r.vec3()?;
        let p = r.vec3()?;
        let q = r.vec3()?;
        r.vec3()?;
        r.vec3()?;
        r.vec3()?;
        let texture = r.u16()?;
        r.i32()?;
        r.u32()?;
        r.take(4)?;
        let use_fx = r.u8()?;
        if use_fx != 0 {
            r.ltstr()?;
            r.ltstr()?;
        }
        r.u16()?;
        surfaces.push(Surface { o, p, q, texture });
    }

    let mut polygons = Vec::with_capacity(poly_count);
    for (c, e) in vert_info {
        r.i16()?;
        r.i16()?;
        r.f32()?;
        r.f32()?;
        let surface = r.i32()?;
        let nv = c as usize + e as usize;
        if nv > 64 {
            return Err(DatError::BadHeader("polygon vertex count"));
        }
        let mut indices = Vec::with_capacity(nv);
        for _ in 0..nv {
            indices.push(r.i16()?);
            r.take(3)?;
        }
        polygons.push(Polygon { surface, indices });
    }

    for _ in 0..node_count {
        r.take(4 + 4 + 2 + 4 + 4 + 16)?;
    }
    for _ in 0..portal_count {
        r.ltstr()?;
        r.i32()?;
        r.u16()?;
        r.vec3()?;
        r.vec3()?;
    }
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        points.push(r.vec3()?);
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in &points {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }
    if points.is_empty() {
        min = [0.0; 3];
        max = [0.0; 3];
    }

    // Bucket by texture so the viewer can bind one material per .dtx.
    let mut buckets: Vec<DatGroup> = textures
        .iter()
        .enumerate()
        .map(|(i, _)| DatGroup {
            texture: i as u16,
            positions: Vec::new(),
            uvs: Vec::new(),
            indices: Vec::new(),
        })
        .collect();

    for poly in &polygons {
        let Some(surf) = surfaces.get(poly.surface as usize) else {
            continue;
        };
        let key = surf.texture as usize;
        if key >= buckets.len() {
            continue;
        }
        let idx: Vec<usize> = poly
            .indices
            .iter()
            .filter_map(|&i| {
                let i = i as usize;
                (i < points.len()).then_some(i)
            })
            .collect();
        if idx.len() < 3 {
            continue;
        }
        let g = &mut buckets[key];
        let base = g.positions.len() as u32;
        for &vi in &idx {
            let pos = points[vi];
            g.positions.push(pos);
            g.uvs.push(opq_uv(pos, surf));
        }
        for k in 1..idx.len() - 1 {
            g.indices.extend([base, base + k as u32, base + k as u32 + 1]);
        }
    }
    buckets.retain(|g| !g.indices.is_empty());

    Ok(DatWorld {
        name,
        textures,
        groups: buckets,
        point_count,
        poly_count,
        surface_count,
        min,
        max,
    })
}

fn u32_at(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

/// A `GameStartPoint` from the object list (LithTech Z-up position).
pub struct StartPoint {
    pub name: String,
    pub pos: [f32; 3],
}

/// Read spawn markers. The object grammar is the one verified in `tools/dat_extract_objects.py`.
pub fn start_points(data: &[u8]) -> Result<Vec<StartPoint>, DatError> {
    if data.len() < 8 {
        return Err(DatError::Truncated);
    }
    let mut r = R {
        d: data,
        p: u32_at(data, 4) as usize,
    };
    let nobj = r.u32()? as usize;
    if nobj > 200_000 {
        return Err(DatError::BadHeader("object count"));
    }
    let mut out = Vec::new();
    for _ in 0..nobj {
        let objsz = r.u16()? as usize;
        let ds = r.p;
        let nlen = r.u16()? as usize;
        let cls = String::from_utf8_lossy(r.take(nlen)?).into_owned();
        let nprops = r.u32()? as usize;
        let mut pos = None;
        let mut label = String::new();
        for _ in 0..nprops {
            let pl = r.u16()? as usize;
            let pname = String::from_utf8_lossy(r.take(pl)?).into_owned();
            let ptype = r.u8()?;
            r.take(4)?;
            let vlen = r.u16()? as usize;
            let val = r.take(vlen)?;
            if cls == "GameStartPoint" && pname == "Pos" && ptype == 1 && val.len() >= 12 {
                pos = Some([
                    f32::from_le_bytes(val[0..4].try_into().unwrap()),
                    f32::from_le_bytes(val[4..8].try_into().unwrap()),
                    f32::from_le_bytes(val[8..12].try_into().unwrap()),
                ]);
            }
            if cls == "GameStartPoint" && pname == "Name" && val.len() >= 2 {
                let sl = u16::from_le_bytes([val[0], val[1]]) as usize;
                if sl <= val.len() - 2 {
                    label = String::from_utf8_lossy(&val[2..2 + sl]).into_owned();
                }
            }
        }
        if r.p > ds + objsz {
            return Err(DatError::BadHeader("object overrun"));
        }
        r.p = ds + objsz;
        if let Some(pos) = pos {
            out.push(StartPoint { name: label, pos });
        }
    }
    Ok(out)
}

/// One authored object the campaign can place: a character, a vehicle, or a prop.
pub struct LevelActor {
    pub class: String,
    pub name: String,
    pub pos: [f32; 3],
    /// Retail path such as `Models\Enemies\OnFoot\Trooper.abc`, or empty.
    pub model: String,
    pub skin: String,
}

/// Characters (`CRONIAN_Trooper`, …) and anything with a model path.
pub fn level_actors(data: &[u8]) -> Result<Vec<LevelActor>, DatError> {
    if data.len() < 8 {
        return Err(DatError::Truncated);
    }
    let mut r = R {
        d: data,
        p: u32_at(data, 4) as usize,
    };
    let nobj = r.u32()? as usize;
    if nobj > 200_000 {
        return Err(DatError::BadHeader("object count"));
    }
    let mut out = Vec::new();
    for _ in 0..nobj {
        let objsz = r.u16()? as usize;
        let ds = r.p;
        let nlen = r.u16()? as usize;
        let class = String::from_utf8_lossy(r.take(nlen)?).into_owned();
        let nprops = r.u32()? as usize;
        let mut pos = None;
        let mut name = String::new();
        let mut model = String::new();
        let mut skin = String::new();
        for _ in 0..nprops {
            let pl = r.u16()? as usize;
            let pname = String::from_utf8_lossy(r.take(pl)?).into_owned();
            let ptype = r.u8()?;
            r.take(4)?;
            let vlen = r.u16()? as usize;
            let val = r.take(vlen)?;
            if pname == "Pos" && ptype == 1 && val.len() >= 12 {
                pos = Some([
                    f32::from_le_bytes(val[0..4].try_into().unwrap()),
                    f32::from_le_bytes(val[4..8].try_into().unwrap()),
                    f32::from_le_bytes(val[8..12].try_into().unwrap()),
                ]);
            } else if pname == "Name" {
                if let Some(s) = lt_string(val) {
                    name = s;
                }
            } else if pname == "Filename" {
                if let Some(s) = lt_string(val) {
                    model = s;
                }
            } else if pname == "Skin" {
                if let Some(s) = lt_string(val) {
                    skin = s;
                }
            }
        }
        if r.p > ds + objsz {
            return Err(DatError::BadHeader("object overrun"));
        }
        r.p = ds + objsz;
        let placed = class.contains('_') || model.to_ascii_lowercase().ends_with(".abc");
        if let Some(pos) = pos {
            if placed {
                out.push(LevelActor {
                    class,
                    name,
                    pos,
                    model,
                    skin,
                });
            }
        }
    }
    Ok(out)
}

/// Title from the world header, e.g. `The Ambush`.
pub fn world_title(data: &[u8]) -> Option<String> {
    if data.len() < 16 {
        return None;
    }
    let n = u32_at(data, 12) as usize;
    let text = data.get(16..16 + n)?;
    let text = String::from_utf8_lossy(text);
    let rest = text.split("WorldName \"").nth(1)?;
    let title = rest.split('"').next()?.trim();
    (!title.is_empty()).then(|| title.to_string())
}

fn lt_string(val: &[u8]) -> Option<String> {
    if val.len() < 2 {
        return None;
    }
    let n = u16::from_le_bytes([val[0], val[1]]) as usize;
    if n == 0 || n > val.len() - 2 {
        return None;
    }
    let bytes = &val[2..2 + n];
    if bytes.iter().any(|b| !(32..127).contains(b)) {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maritropa2_if_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/MULTI/MCA_MARITROPA2.DAT");
        if !path.exists() {
            return;
        }
        let data = std::fs::read(&path).unwrap();
        let w = decode(&data).unwrap();
        assert_eq!(w.point_count, 898);
        assert_eq!(w.poly_count, 682);
        assert_eq!(w.surface_count, 506);
        assert_eq!(w.textures.len(), 37);
        assert_eq!(w.tri_count(), 4447);
        assert!(w.min[0] <= w.max[0]);
        let sp = start_points(&data).unwrap();
        assert_eq!(sp.len(), 5);
        assert_eq!(sp[0].name, "start1");
        assert_eq!(sp[0].pos, [-1280.0, -832.0, 1856.0]);
    }

    #[test]
    fn ambush_lists_the_authored_cast() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        if !path.exists() {
            return;
        }
        let data = std::fs::read(&path).unwrap();
        assert_eq!(world_title(&data).as_deref(), Some("The Ambush"));
        let sp = start_points(&data).unwrap();
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].name, "start1");
        assert_eq!(sp[0].pos, [3072.0, 64.0, 2688.0]);
        let actors = level_actors(&data).unwrap();
        let troop = actors.iter().find(|a| a.name == "ctroop1").expect("ctroop1");
        assert_eq!(troop.class, "CRONIAN_Trooper");
        assert_eq!(troop.pos, [-448.0, 32.0, -4512.0]);
        assert!(troop.model.to_ascii_lowercase().ends_with("trooper.abc"));
        assert!(actors.iter().any(|a| a.class == "UCA_Raksha" && a.name == "dudley"));
        assert!(actors.iter().any(|a| a.class == "FALLEN_Andra10"));
        assert!(actors
            .iter()
            .any(|a| a.model.to_ascii_lowercase().ends_with("truck.abc")));
    }
}
