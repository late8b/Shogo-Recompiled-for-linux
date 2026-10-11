//! LithTech 1.0 DTX texture decoder.
//!
//! Layout (little-endian), reverse-engineered and validated against 3,641/3,675
//! retail textures:
//!
//! ```text
//! +0   u32 version   (= 0)
//! +4   u32 flags     (= 0xffff_fffe)
//! +8   u16 width
//! +10  u16 height
//! +12  u16 mipcount  (usually 4)
//! ...
//! +44  palette[256]  each entry stored as [a, r, g, b]
//! +1068  mip0 indices (8bpp, width*height, row-major)
//!        then mip1 (w/2 * h/2), mip2, ... down to `mipcount` levels
//! ```
//!
//! We decode mip0 (the full-resolution base level) into RGBA8.

/// A decoded RGBA8 texture (mip level 0 only).
pub struct DtxTexture {
    pub width: u32,
    pub height: u32,
    pub mip_count: u16,
    /// RGBA8 pixels, row-major, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Byte offset where the palette begins, and its byte length.
const PALETTE_OFFSET: usize = 44;
const PALETTE_LEN: usize = 256 * 4;
const HEADER_MIN: usize = PALETTE_OFFSET + PALETTE_LEN;

#[derive(Debug)]
pub enum DtxError {
    TooSmall(usize),
    ZeroDimensions,
    TruncatedPixels { need: usize, have: usize },
}

impl std::fmt::Display for DtxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DtxError::TooSmall(n) => write!(f, "file too small for a DTX header ({n} bytes)"),
            DtxError::ZeroDimensions => write!(f, "zero width or height"),
            DtxError::TruncatedPixels { need, have } => {
                write!(f, "truncated pixel data (need {need} bytes, have {have})")
            }
        }
    }
}
impl std::error::Error for DtxError {}

fn read_u16(d: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([d[off], d[off + 1]])
}

/// Decode a DTX file into an RGBA8 texture (mip 0).
pub fn decode(data: &[u8]) -> Result<DtxTexture, DtxError> {
    if data.len() < HEADER_MIN {
        return Err(DtxError::TooSmall(data.len()));
    }
    let width = read_u16(data, 8) as u32;
    let height = read_u16(data, 10) as u32;
    let mip_count = read_u16(data, 12);
    if width == 0 || height == 0 {
        return Err(DtxError::ZeroDimensions);
    }

    // Palette stored as [a, r, g, b]; expand to [r, g, b, a] for RGBA output.
    let pal = &data[PALETTE_OFFSET..PALETTE_OFFSET + PALETTE_LEN];
    let mut rgba_pal = [0u8; 1024];
    for i in 0..256 {
        let a = pal[i * 4];
        let r = pal[i * 4 + 1];
        let g = pal[i * 4 + 2];
        let b = pal[i * 4 + 3];
        rgba_pal[i * 4] = r;
        rgba_pal[i * 4 + 1] = g;
        rgba_pal[i * 4 + 2] = b;
        rgba_pal[i * 4 + 3] = a;
    }

    // mip0 indices.
    let need = (width * height) as usize;
    let start = HEADER_MIN;
    if start + need > data.len() {
        return Err(DtxError::TruncatedPixels {
            need: start + need,
            have: data.len(),
        });
    }
    let idx = &data[start..start + need];

    let mut rgba = Vec::with_capacity(need * 4);
    for &ix in idx {
        let o = ix as usize * 4;
        rgba.extend_from_slice(&rgba_pal[o..o + 4]);
    }

    Ok(DtxTexture {
        width,
        height,
        mip_count,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // An 8x8 INVISIBLE.DTX-style texture: header + ARGB palette + 64 indices.
    #[test]
    fn decodes_8x8() {
        let mut d = vec![0u8; HEADER_MIN + 64];
        d[8..10].copy_from_slice(&8u16.to_le_bytes());
        d[10..12].copy_from_slice(&8u16.to_le_bytes());
        d[12..14].copy_from_slice(&4u16.to_le_bytes());
        // palette[1] = opaque gray (a=255, r=g=b=16)
        d[PALETTE_OFFSET + 4] = 255;
        d[PALETTE_OFFSET + 5] = 16;
        d[PALETTE_OFFSET + 6] = 16;
        d[PALETTE_OFFSET + 7] = 16;
        // all indices = 1
        for i in 0..64 {
            d[HEADER_MIN + i] = 1;
        }
        let t = decode(&d).unwrap();
        assert_eq!((t.width, t.height), (8, 8));
        assert_eq!(&t.rgba[0..4], &[16, 16, 16, 255]);
        assert_eq!(t.rgba.len(), 64 * 4);
    }

    #[test]
    fn rejects_zero_dims() {
        let d = vec![0u8; HEADER_MIN];
        assert!(matches!(decode(&d), Err(DtxError::ZeroDimensions)));
    }
}
