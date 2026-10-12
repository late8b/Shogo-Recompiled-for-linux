# HANDOFF — Shogo → glTF / Rust+Bevy rewrite

Status snapshot for the next agent. Full technical detail is in
`docs/FINDINGS.md`; this file is the "where things stand + what to do next".

## The goal
Convert Shogo: Mobile Armor Division (1998, LithTech 1.0) assets to
glTF/Blender-friendly formats, then rewrite the game in Rust + Bevy, preserving
the **player runtime environment** (original assets loadable at runtime, no
pre-conversion step), running natively on Linux as a single app.

Key decision already made: use **native Rust decoders** (`shogo-assets`) so the
Bevy app reads **original game assets directly** — this is what "preserve the
player runtime" means. GLB/PNG exporters exist for Blender/preservation but the
runtime path is the Rust decoders.

## What is DONE and VERIFIED (do not redo)
- **REZ** archives fully decoded & extracted (`extracted/SOUND/` 2,023 WAVs;
  `extracted/SHOGO/` 5,643 files).
- **DTX** textures decoded (44B header, 256×ARGB palette @44, 8-bit indices/mip);
  `tools/dtx2png.py` → 3,641/3,675 PNG (34 are zero-dim; GLASS/ is a 16-bit
  variant, mip0 still decodes).
- **SPR** decoded (just a pointer to a DTX path).
- **ABC** models fully decoded (property grammar `u16 nameLen, name, u32 endOffset,
  data`; Geometry = bbox + name table + nFaces×33B + nVerts×20B). 538/538 models,
  exact byte consumption, indices valid. `tools/ltabc.py`, `tools/abc2glb.py`
  (Z-up→Y-up, UV v'=1-v, smooth normals).
- **Rust workspace** (`rust/`): `shogo-assets` (native DTX + ABC decoders, tests
  3/3 pass, parity vs every retail asset) + `shogo-viewer` (Bevy 0.20, loads
  original .ABC directly, auto-framed orbit camera, renders via Vulkan —
  screenshot confirmed). Compiles clean; release binary built.
- **Ghidra** wired to Temurin JDK 21; all binaries decompiled to `decompiled/`
  (6,455 functions). `server.dll.c` + `Client.exe.c` are the key references.
- **DAT object/property grammar** — cracked & verified on all 22 worlds (byte
  consumption exact). 3442 Light, 222 GameStartPoint, 309 doors, 112 triggers,
  all powerups extracted with positions/colors/radii.

## What is DONE (new) — DAT v56 WorldBSP
Root-model mesh is decoded on **all 60 worlds**: points, planes, surfaces
(OPQ UVs + texture index), polygons (fan), nodes, portals, PBlocks.
`tools/ltdat.py` (indices in-range) + `tools/dat2glb.py` → `converted/worlds/`
(60/60 GLB). MCA_MARITROPA2: 898 pts, 682 polys, 4447 tris, 37 materials.

Name slot after the 9-u32 WorldModel header is **always 2 bytes** (not a
C-string — large maps put a non-NUL pair there). Count mapping is bspv56:
Point/Plane/Surface/Portal/Poly/Leaf/Vert/Vis/LeafList/Node.

## NEXT ACTION (priority order)
1. **Native `dat` decoder in `shogo-assets`** + load original `.DAT` in
   `shogo-viewer`. Bind DTX textures from surface indices.
2. **Gameplay rewrite**: physics (PBlocks `FUN_004129b0`), AI, MCA/on-foot,
   weapons. Level skeleton (spawns/lights/doors/triggers) is extracted.
3. Optional: extra WorldModels after the root BSP; named section directory.

## Key files
- `docs/FINDINGS.md` — master spec (REZ/DTX/SPR/ABC/DAT formats, class names).
- `README.md` — overview + status table + quick start.
- `tools/` — Python converters (`dtx2png.py`, `abc2glb.py`, `ltabc.py`,
  `ltdat.py`, `dat2glb.py`, `dat_extract_objects.py`, …).
- `rust/crates/shogo-assets/src/{dtx.rs,abc.rs}` — native decoders.
- `rust/crates/shogo-viewer/src/main.rs` — Bevy viewer (loads original ABC).
- `rust/Cargo.toml` — Bevy 0.20 feature set rationale (X11-only, no audio/wayland).
- `decompiled/Client.exe.c` — world/model loaders (`FUN_00418d80` geometry,
  `FUN_00434ad0` ABC geometry, `FUN_00434270` world UV builder).
- `decompiled/server.dll.c` — object/property loader `FUN_10007372` (Light props).
- `extracted/SHOGO/` — 5,643 assets; `WORLDS/MULTI/MCA_MARITROPA2.DAT` is the
  representative world (272,654 B).

## Environment
- Linux Mint 22.3 (Cinnamon/X11), `DISPLAY=:0`; JDK 21 `~/.local/opt/jdk-21...`;
  Ghidra `~/Downloads/ghidra_12.1.4_PUBLIC`; Rust `~/.cargo` (`source ~/.cargo/env`);
  PIL 10.2.0; cc 13.3.0.
- Vulkan works (AMD RADV RENOIR / Mesa). No x11/wayland/alsa/vulkan `-dev`
  headers → Bevy built with restricted features (see `rust/Cargo.toml`).
- Viewer run: `cd rust && SHOGO_MODEL=../extracted/SHOGO/MODELS/CHARACTERS/GABRIEL.ABC WGPU_BACKEND=vulkan cargo run --release -p shogo-viewer`.
- Git repo exists (`master`). Converted GLBs are gitignored (regenerate via tools).
