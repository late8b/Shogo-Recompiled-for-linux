//! Native decoders for Shogo: Mobile Armor Division (LithTech 1.0) asset formats.
//!
//! Reverse-engineered from the decompiled `Client.exe` engine and validated
//! against every retail asset in `SHOGO.REZ`. See `docs/FINDINGS.md` for the
//! full format specification.
//!
//! Formats implemented here:
//! - [`dtx`]: 8-bit palettized textures with an embedded 256-entry ARGB palette.
//! - [`abc`]: "MonolithExport Model File v6" property-tree models (triangles
//!   with per-corner UVs).
//! - [`dat`]: DAT v56 world BSP (points, surfaces, OPQ UVs, polygons).
//! - [`strings`]: shell-DLL dialogue (`RT_STRING` in `mod0_credits.dll`).
//! - [`door_sounds`]: `OpenStartSound` / `CloseStartSound` / `DestroySound`.

pub mod dtx;
pub mod abc;
pub mod dat;
pub mod strings;
pub mod door_sounds;

pub use dtx::DtxTexture;
pub use abc::AbcModel;
pub use dat::DatWorld;
pub use strings::StringTable;
