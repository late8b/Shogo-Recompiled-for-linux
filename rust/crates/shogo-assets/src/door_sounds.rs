//! Door and barricade sound names from a world object list.
//!
//! Kept out of `dat` so the geometry loader can keep changing. The property
//! names are the ones `FUN_100284c0` reads on a `Door`: `OpenStartSound`,
//! `CloseStartSound`, and `SoundRadius`. Breakable shells (`DestructableDoor`,
//! `FUN_100264c0`) add `DestroySound`.

#[derive(Clone, Debug)]
pub struct DoorCue {
    pub name: String,
    pub pos: [f32; 3],
    pub open_start: String,
    pub close_start: String,
    pub destroy: String,
    pub radius: f32,
}

#[derive(Debug)]
pub enum DoorSoundError {
    Truncated,
    BadCount,
    Overrun,
}

impl std::fmt::Display for DoorSoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoorSoundError::Truncated => write!(f, "truncated world while reading door sounds"),
            DoorSoundError::BadCount => write!(f, "object count is not a door list"),
            DoorSoundError::Overrun => write!(f, "door object overran its size"),
        }
    }
}

impl std::error::Error for DoorSoundError {}

pub fn read_door_cues(data: &[u8]) -> Result<Vec<DoorCue>, DoorSoundError> {
    if data.len() < 8 {
        return Err(DoorSoundError::Truncated);
    }
    let mut p = u32le(data, 4)? as usize;
    let nobj = u32le(data, p)? as usize;
    p += 4;
    if nobj > 200_000 {
        return Err(DoorSoundError::BadCount);
    }
    let mut out = Vec::new();
    for _ in 0..nobj {
        let objsz = u16le(data, p)? as usize;
        p += 2;
        let ds = p;
        let nlen = u16le(data, p)? as usize;
        p += 2;
        let class = take(data, &mut p, nlen)?;
        let door = class == b"Door" || class == b"DestructableDoor";
        let nprops = u32le(data, p)? as usize;
        p += 4;
        if nprops > 10_000 {
            return Err(DoorSoundError::BadCount);
        }
        let mut name = String::new();
        let mut pos = [0f32; 3];
        let mut open_start = String::new();
        let mut close_start = String::new();
        let mut destroy = String::new();
        let mut radius = 0f32;
        for _ in 0..nprops {
            let pl = u16le(data, p)? as usize;
            p += 2;
            let pname = take(data, &mut p, pl)?;
            let ptype = *take(data, &mut p, 1)?.first().unwrap_or(&0);
            take(data, &mut p, 4)?;
            let vlen = u16le(data, p)? as usize;
            p += 2;
            let val = take(data, &mut p, vlen)?;
            if !door {
                continue;
            }
            if pname == b"Name" {
                name = lt_string(val);
            } else if pname == b"Pos" && (ptype == 1 || ptype == 2) && val.len() >= 12 {
                pos = [
                    f32::from_le_bytes(val[0..4].try_into().unwrap()),
                    f32::from_le_bytes(val[4..8].try_into().unwrap()),
                    f32::from_le_bytes(val[8..12].try_into().unwrap()),
                ];
            } else if pname == b"OpenStartSound" {
                open_start = lt_string(val);
            } else if pname == b"CloseStartSound" {
                close_start = lt_string(val);
            } else if pname == b"DestroySound" {
                destroy = lt_string(val);
            } else if pname == b"SoundRadius" && (ptype == 3 || ptype == 6) && val.len() >= 4 {
                radius = f32::from_le_bytes(val[0..4].try_into().unwrap());
            }
        }
        if p > ds + objsz {
            return Err(DoorSoundError::Overrun);
        }
        p = ds + objsz;
        if door && !name.is_empty() {
            out.push(DoorCue {
                name,
                pos,
                open_start,
                close_start,
                destroy,
                radius,
            });
        }
    }
    Ok(out)
}

fn lt_string(val: &[u8]) -> String {
    if val.len() < 2 {
        return String::new();
    }
    let n = u16::from_le_bytes([val[0], val[1]]) as usize;
    if n == 0 || n > val.len() - 2 {
        return String::new();
    }
    let bytes = &val[2..2 + n];
    if bytes.iter().any(|b| !(32..127).contains(b)) {
        return String::new();
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn take<'a>(data: &'a [u8], p: &mut usize, n: usize) -> Result<&'a [u8], DoorSoundError> {
    let end = p.checked_add(n).ok_or(DoorSoundError::Truncated)?;
    let slice = data.get(*p..end).ok_or(DoorSoundError::Truncated)?;
    *p = end;
    Ok(slice)
}

fn u16le(data: &[u8], off: usize) -> Result<u16, DoorSoundError> {
    let b = data.get(off..off + 2).ok_or(DoorSoundError::Truncated)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32le(data: &[u8], off: usize) -> Result<u32, DoorSoundError> {
    let b = data.get(off..off + 4).ok_or(DoorSoundError::Truncated)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarters_names_the_lift_and_the_tech_door() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/02_QUARTERS.DAT");
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let cues = read_door_cues(&data).unwrap();
        let lift = cues.iter().find(|c| c.name == "mainlift").expect("mainlift");
        assert_eq!(lift.open_start, "sounds\\doors\\openstart05.wav");
        assert_eq!(lift.close_start, "sounds\\doors\\openstart05.wav");
        let door = cues.iter().find(|c| c.name == "door2").expect("door2");
        assert_eq!(door.open_start, "sounds\\doors\\tech2.wav");
        assert_eq!(door.close_start, "sounds\\doors\\tech2.wav");
    }

    #[test]
    fn ambush_barricade_has_no_authored_break_sound() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let cues = read_door_cues(&data).unwrap();
        let barb = cues.iter().find(|c| c.name == "barb1").expect("barb1");
        assert!(barb.destroy.is_empty());
        assert!(barb.open_start.is_empty());
        assert!(barb.pos[0].is_finite());
    }
}
