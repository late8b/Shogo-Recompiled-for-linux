//! Retail dialogue string table.
//!
//! Shogo keeps mission copy in the shell DLL (`embedded/mod0_credits.dll`),
//! as a PE `RT_STRING` resource. `LoadString` ids are 0-based: block
//! `(id >> 4) + 1`, index `id & 15`. Each string is a `u16` character count
//! followed by UTF-16LE units and no NUL.
//!
//! `extracted/SHOGO/SRES.DLL` is a different, smaller table (powerup paths,
//! ids through the low hundreds). Transmission `StringID`s such as Ambush
//! `3671` are in the shell DLL. Objective `1040` is
//! "Fall back to Drop Point Renegade" in that same table.

use std::collections::BTreeMap;

/// Id → retail text. Missing ids are absent, not stored as empty strings.
#[derive(Clone, Debug, Default)]
pub struct StringTable {
    entries: BTreeMap<i32, String>,
}

#[derive(Debug)]
pub enum StringsError {
    NotPe,
    NoStringTable,
    Truncated,
}

impl std::fmt::Display for StringsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StringsError::NotPe => write!(f, "not a PE32 string DLL"),
            StringsError::NoStringTable => write!(f, "PE has no RT_STRING resources"),
            StringsError::Truncated => write!(f, "truncated PE string resource"),
        }
    }
}

impl std::error::Error for StringsError {}

impl StringTable {
    pub fn parse(data: &[u8]) -> Result<Self, StringsError> {
        let blocks = string_blocks(data)?;
        let mut entries = BTreeMap::new();
        for (block_id, blob) in blocks {
            let mut p = 0usize;
            let base = block_id.saturating_sub(1).saturating_mul(16);
            for index in 0..16 {
                if p + 2 > blob.len() {
                    break;
                }
                let chars = u16::from_le_bytes([blob[p], blob[p + 1]]) as usize;
                p += 2;
                if chars == 0 {
                    continue;
                }
                if chars > 8_192 || p + chars * 2 > blob.len() {
                    return Err(StringsError::Truncated);
                }
                let mut units = Vec::with_capacity(chars);
                for _ in 0..chars {
                    units.push(u16::from_le_bytes([blob[p], blob[p + 1]]));
                    p += 2;
                }
                let text = String::from_utf16_lossy(&units);
                if text.is_empty() {
                    continue;
                }
                let id = base + index;
                if id > i32::MAX as u32 {
                    continue;
                }
                entries.insert(id as i32, text);
            }
        }
        if entries.is_empty() {
            return Err(StringsError::NoStringTable);
        }
        Ok(Self { entries })
    }

    pub fn get(&self, id: i32) -> Option<&str> {
        self.entries.get(&id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn string_blocks(data: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, StringsError> {
    if data.len() < 0x40 || &data[0..2] != b"MZ" {
        return Err(StringsError::NotPe);
    }
    let e_lfanew = u32_at(data, 0x3C)? as usize;
    if e_lfanew + 24 > data.len() || &data[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return Err(StringsError::NotPe);
    }
    let coff = e_lfanew + 4;
    let nsec = u16_at(data, coff + 2)? as usize;
    let optsz = u16_at(data, coff + 16)? as usize;
    let opt = coff + 20;
    if opt + optsz > data.len() || u16_at(data, opt)? != 0x10B {
        return Err(StringsError::NotPe);
    }
    if opt + 96 > data.len() {
        return Err(StringsError::Truncated);
    }
    let n_dd = u32_at(data, opt + 92)? as usize;
    if n_dd < 3 {
        return Err(StringsError::NoStringTable);
    }
    let res_rva = u32_at(data, opt + 112)?;
    let res_size = u32_at(data, opt + 116)?;
    if res_rva == 0 || res_size == 0 {
        return Err(StringsError::NoStringTable);
    }
    let sections = sections(data, opt + optsz, nsec)?;
    let base = rva_to_off(&sections, res_rva).ok_or(StringsError::Truncated)?;
    let root = read_dir(data, base)?;
    let Some(strings) = root.into_iter().find(|(id, _, dir)| *id == 6 && *dir) else {
        return Err(StringsError::NoStringTable);
    };
    let blocks = read_dir(data, base + strings.1 as usize)?;
    let mut out = Vec::new();
    for (block_id, rel, is_dir) in blocks {
        if !is_dir {
            continue;
        }
        let langs = read_dir(data, base + rel as usize)?;
        let Some((_, data_rel, is_dir)) = langs.first() else {
            continue;
        };
        if *is_dir {
            continue;
        }
        let entry = base + *data_rel as usize;
        if entry + 16 > data.len() {
            return Err(StringsError::Truncated);
        }
        let data_rva = u32_at(data, entry)?;
        let size = u32_at(data, entry + 4)? as usize;
        let off = rva_to_off(&sections, data_rva).ok_or(StringsError::Truncated)?;
        if off + size > data.len() {
            return Err(StringsError::Truncated);
        }
        out.push((block_id, data[off..off + size].to_vec()));
    }
    if out.is_empty() {
        return Err(StringsError::NoStringTable);
    }
    Ok(out)
}

struct Section {
    va: u32,
    span: u32,
    raw: u32,
}

fn sections(data: &[u8], off: usize, nsec: usize) -> Result<Vec<Section>, StringsError> {
    if nsec > 96 || off + nsec * 40 > data.len() {
        return Err(StringsError::Truncated);
    }
    let mut out = Vec::with_capacity(nsec);
    for i in 0..nsec {
        let at = off + i * 40;
        let vsize = u32_at(data, at + 8)?;
        let va = u32_at(data, at + 12)?;
        let rawsize = u32_at(data, at + 16)?;
        let raw = u32_at(data, at + 20)?;
        out.push(Section {
            va,
            span: vsize.max(rawsize),
            raw,
        });
    }
    Ok(out)
}

fn rva_to_off(sections: &[Section], rva: u32) -> Option<usize> {
    for sec in sections {
        if rva >= sec.va && rva < sec.va.saturating_add(sec.span) {
            return Some((sec.raw as usize) + (rva - sec.va) as usize);
        }
    }
    None
}

/// `(id, offset_from_resource_base, is_directory)`.
fn read_dir(data: &[u8], dir: usize) -> Result<Vec<(u32, u32, bool)>, StringsError> {
    if dir + 16 > data.len() {
        return Err(StringsError::Truncated);
    }
    let named = u16_at(data, dir + 12)? as usize;
    let ids = u16_at(data, dir + 14)? as usize;
    let count = named + ids;
    if count > 4_096 || dir + 16 + count * 8 > data.len() {
        return Err(StringsError::Truncated);
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = dir + 16 + i * 8;
        let name_or_id = u32_at(data, at)?;
        let offset = u32_at(data, at + 4)?;
        let is_dir = offset & 0x8000_0000 != 0;
        let ident = if name_or_id & 0x8000_0000 != 0 {
            // Named entries are not string-table block ids.
            continue;
        } else {
            name_or_id
        };
        out.push((ident, offset & 0x7fff_ffff, is_dir));
    }
    Ok(out)
}

fn u16_at(data: &[u8], off: usize) -> Result<u16, StringsError> {
    let b = data.get(off..off + 2).ok_or(StringsError::Truncated)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], off: usize) -> Result<u32, StringsError> {
    let b = data.get(off..off + 4).ok_or(StringsError::Truncated)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits() -> Option<StringTable> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../embedded/mod0_credits.dll");
        let data = std::fs::read(path).ok()?;
        StringTable::parse(&data).ok()
    }

    #[test]
    fn ambush_ids_are_retail_copy() {
        let Some(table) = credits() else {
            return;
        };
        assert!(table.len() > 1_000);
        assert_eq!(
            table.get(1040),
            Some("Fall back to Drop Point Renegade")
        );
        assert_eq!(
            table.get(3671),
            Some(
                "Recon 4, fall back to Drop Point Renegade for immediate evac. We're picking up Fallen reinforcements approaching from the southeast. Get your team clear ASAP. Over."
            )
        );
        assert_eq!(
            table.get(2055),
            Some("This is Avenging Angel on final approach. Drop point secure. Over.")
        );
        assert_eq!(table.get(3093), Some("Gotta go!"));
        assert!(table.get(999_999).is_none());
    }

    #[test]
    fn sres_is_not_the_dialogue_table() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/SRES.DLL");
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let table = StringTable::parse(&data).unwrap();
        assert!(table.get(3671).is_none());
        assert!(table.len() < 200);
    }
}
