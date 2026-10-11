//! Native decoders for Shogo: Mobile Armor Division (LithTech 1.0) asset formats.
//!
//! Reverse-engineered from the decompiled `Client.exe` engine and validated
//! against every retail asset in `SHOGO.REZ`. See `docs/FINDINGS.md` for the
//! full format specification.
//!
//! Two formats are implemented:
//! - [`dtx`]: 8-bit palettized textures with an embedded 256-entry ARGB palette.
//! - [`abc`]: "MonolithExport Model File v6" property-tree models (triangles
//!   with per-corner UVs).

pub mod dtx;
pub mod abc;

pub use dtx::DtxTexture;
pub use abc::AbcModel;
