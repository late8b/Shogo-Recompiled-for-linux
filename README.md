# Shogo: Mobile Armor Division — Preservation & Native-Runtime Project

Reverse-engineering and preservation of **Shogo: Mobile Armor Division** (Monolith
Productions, 1998, LithTech 1.0), with the goal of running it natively on Linux
as a Bevy application while preserving the original assets and player runtime.

Everything here was reverse-engineered from the retail game (`SHOGO.REZ`,
`SOUND.REZ`, `Client.exe`, and the four game-logic DLLs embedded inside the REZ).
See `docs/FINDINGS.md` for the full technical writeup (engine facts, REZ spec,
asset-format specs, recovered C++ class names).

## Status

| Area | State |
|------|-------|
| REZ archive extraction | **Done** — both retail archives fully extracted & validated |
| DTX textures → PNG | **Done** — 3,641 / 3,675 decoded (34 are zero-dim placeholders) |
| ABC models → glTF | **Done** — 538 / 538 models, exact byte consumption |
| Rust asset decoders | **Done** — `shogo-assets` (DTX + ABC), unit-tested + parity vs all retail assets |
| Bevy native viewer | **Done (MVP)** — loads original .ABC directly, orbit camera, renders via Vulkan |
| DAT world objects/properties | **Done** — grammar verified on all 22 worlds; spawns/lights/doors/triggers/powerups extracted |
| DAT world geometry | **Partial** — header + texture table decoded; polygon mesh (leaf/portal tree) needs a faithful port of loader `FUN_00418d80` |
| Full gameplay rewrite | Not started |

## Repository layout

```
docs/FINDINGS.md          master technical writeup (formats, engine, classes)
tools/                    Python pipeline (extraction + format decoders)
  rezextract.py           REZ v1 archive extractor (auto root detection)
  dtx2png.py              DTX texture -> PNG (single + --batch)
  ltabc.py                ABC model parser
  abc2glb.py              ABC -> glTF 2.0 binary (.glb) exporter
extracted/                raw assets pulled from the REZ archives
  SOUND/                  2,023 WAV files
  SHOGO/                  models (.ABC), worlds (.DAT), textures (.DTX), ...
converted/                ready-to-use assets
  textures/               PNG (mirrors the DTX tree)
  models/                 .glb (mirrors the ABC tree)
rust/                     Cargo workspace
  crates/shogo-assets/    native DTX + ABC decoders (no Bevy dependency)
  crates/shogo-viewer/    Bevy app: GLB viewer with orbit camera
embedded/                 the 4 game-logic DLLs extracted from SHOGO.REZ
decompiled/               Ghidra decompiler output (6,455 functions)
ghidra/                   headless decompile script
```

## Quick start

### Convert assets (Python, no extra deps beyond Pillow)

```bash
# textures -> PNG
python3 tools/dtx2png.py --batch extracted/SHOGO/TEXTURES converted/textures

# models -> glTF binary
python3 tools/abc2glb.py --batch extracted/SHOGO/MODELS converted/models

# a single model
python3 tools/abc2glb.py extracted/SHOGO/MODELS/CHARACTERS/GABRIEL.ABC gabriel.glb
```

`.glb` files load directly in Blender (File → Import → glTF 2.0) and in the
Bevy viewer below.

### Run the native viewer (Rust + Bevy)

The viewer does **not** go through a converted intermediate — it loads the
**original `.ABC` model directly** via the `shogo-assets` decoder and builds a
Bevy mesh from it. This is the native-runtime path: original game asset → Rust
decoder → Bevy render, with no conversion step at runtime.

```bash
cd rust
# point it at an original model from the extraction:
SHOGO_MODEL=../extracted/SHOGO/MODELS/CHARACTERS/GABRIEL.ABC cargo run --release -p shogo-viewer
```

Right-drag to orbit, scroll to zoom. The camera auto-frames the model bounds.

Build notes (see `rust/Cargo.toml`): Bevy is built with `default-features =
false` plus an explicit feature list so it needs **no system dev headers** —
`audio` (alsa) and `wayland` (pkg-config) are disabled, and windowing is X11.
This matches the target machine (Cinnamon/X11, Mesa Vulkan).

Run the native decoders' tests, and validate against every retail asset:

```bash
cd rust && cargo test -p shogo-assets
cargo run --release --example decode_all -p shogo-assets -- ../extracted/SHOGO
#   ABC  ok=538  fail=0  total_tris=98534
#   DTX  ok=4638 fail=34  (the 34 are zero-dimension placeholders)
```

## Asset format specs (summary)

Full detail in `docs/FINDINGS.md`.

- **REZ v1** — 128-byte info string; directory records
  `type(4) offset(4) size(4) dostime(4) + name`; type 0 = file (adds
  `flags(4)`, reversed `ext[8]`, NUL-terminated name); payloads raw, no
  compression. Retail root offsets: `SOUND.REZ` @140965951, `SHOGO.REZ`
  @259422029.
- **DTX** — 44-byte header (`@8 w, @10 h, @12 mipcount`), 256×ARGB palette @44,
  then 8-bit indices per mip. `GLASS/` uses a 16-bit variant.
- **ABC** — property tree (`u16 nameLen, name, u32 endOffset, data`). Geometry =
  bbox + name table + N×33-byte triangles (3 UV pairs + 3 indices + flags) +
  N×20-byte vertices (12 = position). Each face carries its own UVs.
- **SPR** — just a path pointer to a DTX.
- **DAT** — DEdit world: text header, object/entity list (spawns, lights,
  triggers with Pos/Rotation), then polygon geometry (in progress).

## Goals / roadmap

1. **Asset conversion** — ✅ textures and models; worlds pending.
2. **Native Linux runtime** — a Bevy app that loads the original assets
   directly (via `shogo-assets`) rather than a converted intermediate.
3. **Preserve the player runtime** — keep the original behavior (physics, AI,
   weapon semantics, MCA/on-foot modes) reproducible; the decompiled
   `decompiled/` output is the ground-truth reference.
4. **Gameplay rewrite** — port game logic (currently in `Client.exe` +
   `mod2_CShellRef.dll` / `mod3_Object.dll`) to Rust, module by module.

## Licensing note

Game assets, names, and code referenced here are property of Monolith
Productions / Sierra Entertainment. This is a preservation and interoperability
effort; assets are not redistributed here — bring your own retail copy.
