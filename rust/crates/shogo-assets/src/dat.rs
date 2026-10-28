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

/// One authored message, `TargetNameN` / `MessageNameN`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionMessage {
    pub target: String,
    pub body: String,
}

/// Trigger, exit, objective, or transmission. Positions are LithTech space.
#[derive(Clone, Debug)]
pub struct MissionNode {
    pub class: NodeClass,
    pub name: String,
    pub pos: [f32; 3],
    pub dims: [f32; 3],
    pub send_delay: f32,
    pub touch: bool,
    pub player: bool,
    pub ai: bool,
    pub ai_name: String,
    pub locked: bool,
    pub messages: Vec<MissionMessage>,
    pub destination: String,
    pub start_name: String,
    pub add_objectives: String,
    pub remove_objectives: String,
    pub complete_objectives: String,
    /// `TransmissionTrigger` string table id. `0` when the node has none.
    pub string_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeClass {
    Trigger,
    Exit,
    Objective,
    Transmission,
}

/// `AIKey` marker. `FollowPath b` walks `b0`, `b1`, `b2`, …
#[derive(Clone, Debug)]
pub struct Marker {
    pub name: String,
    pub pos: [f32; 3],
}

/// `Key` on a keyframer path. `time` is seconds from the previous key.
#[derive(Clone, Debug)]
pub struct MotionKey {
    pub name: String,
    pub pos: [f32; 3],
    pub time: f32,
    pub target: String,
    pub message: String,
}

/// `KeyFramer`. `On` moves `object` along keys named `{base}0`, `{base}1`, …
#[derive(Clone, Debug)]
pub struct Framer {
    pub name: String,
    pub object: String,
    pub base: String,
}

/// A door brush. Breakable shells have hit points. Sliding doors have a move.
///
/// `state_flags` is the retail float bitfield: 1 touch-open, 2 start open,
/// 4 close again after `open_wait`. `waveform` is 0 linear, 1 sine, 2 slow-off,
/// 3 slow-on.
#[derive(Clone, Debug)]
pub struct Door {
    pub name: String,
    pub pos: [f32; 3],
    pub hit_points: f32,
    pub move_dist: f32,
    pub move_dir: [f32; 3],
    pub speed: f32,
    pub move_delay: f32,
    pub open_wait: f32,
    pub close_wait: f32,
    pub trigger_dims: [f32; 3],
    pub waveform: u8,
    pub state_flags: u32,
}

/// The mission objects the viewer can run without the retail script VM.
#[derive(Clone, Debug, Default)]
pub struct MissionData {
    pub nodes: Vec<MissionNode>,
    pub markers: Vec<Marker>,
    pub keys: Vec<MotionKey>,
    pub framers: Vec<Framer>,
    pub doors: Vec<Door>,
}

/// Read triggers, AI keys, keyframers, and breakable doors from the object list.
pub fn read_mission(data: &[u8]) -> Result<MissionData, DatError> {
    let raw = raw_objects(data)?;
    let mut out = MissionData::default();
    for (class, props) in raw {
        let name = prop_str(&props, "Name");
        let pos = prop_vec(&props, "Pos");
        match class.as_str() {
            "AIKey" => out.markers.push(Marker { name, pos }),
            "Key" => out.keys.push(MotionKey {
                name,
                pos,
                time: prop_f32(&props, "TimeStamp"),
                target: prop_str(&props, "MessageTarget"),
                message: prop_str(&props, "MessageName"),
            }),
            "KeyFramer" => out.framers.push(Framer {
                name,
                object: prop_str(&props, "ObjectName"),
                base: prop_str(&props, "BaseKeyName"),
            }),
            "Trigger" | "ExitTrigger" | "ObjectiveTrigger" | "TransmissionTrigger" => {
                let mut messages = Vec::new();
                for i in 1..=10 {
                    let target = prop_str(&props, &format!("TargetName{i}"));
                    let body = prop_str(&props, &format!("MessageName{i}"));
                    if target.is_empty() {
                        continue;
                    }
                    messages.push(MissionMessage { target, body });
                }
                let class = match class.as_str() {
                    "ExitTrigger" => NodeClass::Exit,
                    "ObjectiveTrigger" => NodeClass::Objective,
                    "TransmissionTrigger" => NodeClass::Transmission,
                    _ => NodeClass::Trigger,
                };
                out.nodes.push(MissionNode {
                    class,
                    name,
                    pos,
                    dims: prop_vec(&props, "Dims"),
                    send_delay: prop_f32(&props, "SendDelay"),
                    touch: prop_flag(&props, "TriggerTouch"),
                    player: prop_flag(&props, "PlayerTriggerable"),
                    ai: prop_flag(&props, "AITriggerable"),
                    ai_name: prop_str(&props, "AITriggerName"),
                    locked: prop_flag(&props, "Locked"),
                    messages,
                    destination: prop_str(&props, "DestinationWorld"),
                    start_name: prop_str(&props, "StartPointName"),
                    add_objectives: prop_str(&props, "AddObjectives"),
                    remove_objectives: prop_str(&props, "RemoveObjectives"),
                    complete_objectives: prop_str(&props, "CompleteObjectives"),
                    string_id: prop_f32(&props, "StringID") as i32,
                });
            }
            "Door" | "DestructableDoor" => {
                let hit_points = prop_f32(&props, "HitPoints");
                let move_dist = prop_f32(&props, "MoveDist");
                // Sky shells are doors with neither hit points nor a slide.
                if hit_points <= 0.0 && move_dist.abs() < 1.0 {
                    continue;
                }
                out.doors.push(Door {
                    name,
                    pos,
                    hit_points,
                    move_dist,
                    move_dir: prop_vec(&props, "MoveDir"),
                    speed: prop_f32(&props, "Speed"),
                    move_delay: prop_f32(&props, "MoveDelay"),
                    open_wait: prop_f32(&props, "OpenWaitTime"),
                    close_wait: prop_f32(&props, "CloseWaitTime"),
                    trigger_dims: prop_vec(&props, "TriggerDims"),
                    waveform: prop_f32(&props, "Waveform").round().max(0.0) as u8,
                    state_flags: prop_f32(&props, "StateFlags").round().max(0.0) as u32,
                });
            }
            _ => {}
        }
    }
    Ok(out)
}

enum ObjProp {
    Vec([f32; 3]),
    Num(f32),
    Byte(u8),
    Str(String),
}

fn raw_objects(data: &[u8]) -> Result<Vec<(String, Vec<(String, ObjProp)>)>, DatError> {
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
        let mut props = Vec::with_capacity(nprops);
        for _ in 0..nprops {
            let pl = r.u16()? as usize;
            let pname = String::from_utf8_lossy(r.take(pl)?).into_owned();
            let ptype = r.u8()?;
            r.take(4)?;
            let vlen = r.u16()? as usize;
            let val = r.take(vlen)?;
            let prop = if (ptype == 1 || ptype == 2) && val.len() >= 12 {
                ObjProp::Vec([
                    f32::from_le_bytes(val[0..4].try_into().unwrap()),
                    f32::from_le_bytes(val[4..8].try_into().unwrap()),
                    f32::from_le_bytes(val[8..12].try_into().unwrap()),
                ])
            } else if (ptype == 3 || ptype == 6) && val.len() >= 4 {
                // Type 6 is the same f32 as a float. Door flags and waveforms use it.
                ObjProp::Num(f32::from_le_bytes(val[0..4].try_into().unwrap()))
            } else if ptype == 5 && !val.is_empty() {
                ObjProp::Byte(val[0])
            } else if let Some(s) = lt_string(val) {
                ObjProp::Str(s)
            } else {
                ObjProp::Str(String::new())
            };
            props.push((pname, prop));
        }
        if r.p > ds + objsz {
            return Err(DatError::BadHeader("object overrun"));
        }
        r.p = ds + objsz;
        out.push((class, props));
    }
    Ok(out)
}

fn prop_str(props: &[(String, ObjProp)], key: &str) -> String {
    props
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, prop)| match prop {
            ObjProp::Str(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn prop_f32(props: &[(String, ObjProp)], key: &str) -> f32 {
    props
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, prop)| match prop {
            ObjProp::Num(n) => Some(*n),
            ObjProp::Byte(b) => Some(*b as f32),
            _ => None,
        })
        .unwrap_or(0.0)
}

fn prop_flag(props: &[(String, ObjProp)], key: &str) -> bool {
    prop_f32(props, key) != 0.0
}

fn prop_vec(props: &[(String, ObjProp)], key: &str) -> [f32; 3] {
    props
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, prop)| match prop {
            ObjProp::Vec(v) => Some(*v),
            _ => None,
        })
        .unwrap_or([0.0, 0.0, 0.0])
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
        let mission = read_mission(&data).unwrap();
        let exit = mission
            .nodes
            .iter()
            .find(|n| n.class == NodeClass::Exit && n.name == "outtahere")
            .expect("exit");
        assert!(exit.locked);
        assert_eq!(exit.destination, "02_Quarters");
        assert_eq!(exit.start_name, "start1");
        assert!(mission.markers.iter().any(|m| m.name == "b0"));
        assert!(mission.markers.iter().any(|m| m.name == "b4"));
        let barb = mission.doors.iter().find(|d| d.name == "barb1").expect("barb1");
        assert_eq!(barb.hit_points, 100.0);
        assert!(mission.doors.iter().all(|d| d.hit_points > 0.0));
        assert!(mission.doors.iter().all(|d| d.move_dist.abs() < 1.0));
        assert!(mission.framers.iter().any(|f| f.name == "firstdrop" && f.object == "drop1"));
        let start = mission
            .nodes
            .iter()
            .find(|n| n.name == "start")
            .expect("start");
        assert!(start.messages.iter().any(|m| m.target == "trans1" && m.body == "Trigger"));
    }

    #[test]
    fn quarters_lift_is_a_sliding_door() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/02_QUARTERS.DAT");
        if !path.exists() {
            return;
        }
        let data = std::fs::read(&path).unwrap();
        let mission = read_mission(&data).unwrap();
        let lift = mission
            .doors
            .iter()
            .find(|d| d.name == "mainlift")
            .expect("mainlift");
        assert!((lift.move_dist - 640.0).abs() < 0.1);
        assert!((lift.speed - 300.0).abs() < 0.1);
        assert_eq!(lift.move_dir, [0.0, 1.0, 0.0]);
        assert_eq!(lift.state_flags & 4, 4);
        assert_eq!(lift.waveform, 0);
        let door = mission.doors.iter().find(|d| d.name == "door2").expect("door2");
        assert!((door.open_wait - 3.0).abs() < 0.1);
        assert_eq!(door.state_flags & 1, 1);
    }
}
