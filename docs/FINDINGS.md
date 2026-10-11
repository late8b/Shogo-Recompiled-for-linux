# Shogo: Mobile Armor Division — Preservation & Decompilation Project

Started 2026-10-11. Goal: reverse-engineer, document and preserve the 1998
Monolith Productions game *Shogo: Mobile Armor Division* (the first game built
on the LithTech 1.0 engine).

## Source material

- `~/Downloads/shogo-mobile-armor-division/` — Internet Archive copy
  (identifier `shogo-mobile-armor-division`, mediatype `software`).
- Game files extracted from `Shogo Mobile Armor Division.zip` to
  `~/Downloads/shogo-mobile-armor-division/Shogo Mobile Armor Division/`.
- 271 MB extracted; the two big resource archives are `SHOGO.REZ` (259 MB) and
  `SOUND.REZ` (141 MB).

## Engine facts (established)

- **Engine:** LithTech 1.0 (Monolith's first engine). `RezMgr Version 1
  Copyright (C) 1995 MONOLITH INC.` appears in every REZ header.
- **`LITHTECH.DLL` is absent on purpose.** The engine is statically compiled
  into `Client.exe` — note the `"LithTech build %s initialized in %.2f seconds."`
  string inside it. This copy is self-contained and complete.
- Binaries are PE32 (x86, 32-bit), stripped, no PDB. Expect `FUN_XXXXXXX`
  names from the decompiler; real symbols must come from RTTI + string tables.

## Architecture / targets

| File | Role | Notes |
|------|------|-------|
| `Shogo.exe` | Launcher / config GUI | 830 KB; picks renderer, joins games |
| `Client.exe` | **Game client + statically-linked LithTech 1.0 engine** | 582 KB; the core |
| `server.dll` | LithTech server framework (generic) | mostly `ServerDE`/`CSBase` |
| `ShogoSrv.exe` | Dedicated server | |
| `d3d.ren` / `soft.ren` | Renderer modules (Direct3D / software) | loaded via `*.ren` plugin scan |
| `SHOGO.REZ` | Main resource archive | models, textures, scripts, **and embedded DLLs** |
| `SOUND.REZ` | Audio archive | RIFF/WAVE payloads |

### KEY FINDING: game logic lives INSIDE `SHOGO.REZ`

The actual Shogo game-object code is **not** in the loose `server.dll`; it is
embedded as PE DLLs inside `SHOGO.REZ`. Scanning the archive for validated `MZ` +
`PE\0\0` headers found **4 embedded DLLs**, payloads starting at offset 168:

| Offset | Size | Identified by |
|--------|------|---------------|
| 168 | 179,200 | Credits text ("Craig Hubbard, Lead Game Engineer", "Weapon and Character") — game shell/logic |
| 179,368 | 38,912 | small, unidentified |
| 218,280 | 525,824 | references `CShell.dll` + asset paths |
| 744,104 | 1,016,832 | `Object.lto` + live asset paths (`Models\`, `Sounds\`, `Skins\`, `Sprites\`) — the object/class library |

Extracted to `embedded/` (renamed `mod0_credits`, `mod1_small`, `mod2_CShellRef`,
`mod3_Object`).

#### Shell-module identification (CONFIRMED)

LithTech 1.0 loads game logic through a fixed **shell ABI** — each module exports
`GetServerShellFunctions` / `GetServerShellVersion` or `GetClientShellFunctions` /
`GetClientShellVersion`. Matching exports to modules:

| Embedded DLL | Exports | Role |
|--------------|---------|------|
| `mod2_CShellRef.dll` (525 KB) | `GetClientShellFunctions`, `GetClientShellVersion` | **ClientShell** (references `CShell.dll`) |
| `mod3_Object.dll` (1.0 MB) | `GetServerShellFunctions`, `GetServerShellVersion` | **ServerShell** (object/class library, `Object.lto`) |
| `mod0_credits.dll` (179 KB) | — | credits + game shell strings |
| `mod1_small.dll` (39 KB) | — | unidentified, small |

#### Real Shogo C++ classes recovered (from string/RTTI tables)

Game's internal codename was **"Riot"** — note `CRiotClientShell`. Weapon and
character classes recovered from the embedded modules:

```
CRiotClientShell        CShell                 CPlayerObj
CBaseCharacter          CWeapon                CPVWeaponModel
CProjectile             CDTrack                CClientSFX
CClientCastLineSFX      CStoryTrigger
```

Projectile classes (= actual Shogo weapons):
```
CBullgutProjectile      CJuggernautProjectile  CKatoGrenadeProjectile
CGrenadeProjectile      CRedRiotProjectile     CPulseRifleProjectile
CStickyGrenadeProjectile  CTOWProjectile
```

This is the preservation payload: the actual game logic, recoverable as C.

Asset-directory inventory recovered from `mod3_Object` strings:
`Models` (444), `Sounds` (297), `Skins` (296), `Sprites` (37), `SpecialFX` (20).

## REZ format (FULLY DECODED — see `tools/rezextract.py`)

```
bytes 0..127    ASCII info string, terminated by "\r\n\x1a\x01"  (header = 128 bytes)
byte  168       first resource payload (confirmed for all retail archives)
```

The **root directory pointer is NOT in the header** for retail archives (only the
tiny `ShogoP.rez` stores `dirStart`/`dirSize` as big-endian u32 at offset 128).
Instead the directory tree is found by scanning near EOF for a block of clean
records. In practice the root block of each archive sits at the very end:

| Archive | Root offset | Root block |
|---------|-------------|------------|
| `ShogoP.rez` | 182 | 1 file (`PATCH.TXT`) |
| `SOUND.REZ` | 140965951 | 13 dirs (VOICE, PROPS, DOORS, PLAYER, WEAPONS, ENEMIES, AMBIENT, SWITCHES, POWERUPS, ENV, SPECIALFX, MAJORCHARACTER, INTERFACE) |
| `SHOGO.REZ` | 259422029 | 8 dirs (SKINS, WORLDS, MODELS, SPRITES, TEXTURES, SPRITETEXTURES, SPECIALFX, INTERFACE) + SRES.DLL |

Directory records are **little-endian**, a 16-byte fixed header then a name:

```
uint32 type     ; 0 = file (payload), 1 = directory (child block)
uint32 offset   ; file: payload offset. dir: offset of child record block
uint32 size     ; file: stored size. dir: byte length of child block
uint32 dostime  ; DOS packed timestamp
-- type==0 (file):
    uint32 flags            ; usually 0x00989680
    char   ext[8]           ; extension, stored REVERSED, NUL-padded ("VAW"=="WAV", "LLD"=="DLL")
    char   name[]           ; NUL-terminated base name
-- type==1 (dir):
    char   name[]           ; NUL-terminated name (no flags/ext)
```

It is a TREE (directories recurse into child listings). Payloads are stored raw
(v1 has no compression). Two quirks the extractor must handle:

- **Leaf blocks open with a sentinel record** — `type=0, offset=0, size=0, ext
  empty, name "DIRTYPE<dirname>"` (e.g. `DIRTYPESOUNDS`, `DIRTYPEMODELS`) —
  followed by one pad byte; the first real record starts at +43. Skip any
  record with `offset==0`.
- **Self-referential footer:** reading *past* the real block end picks up a
  trailing record that points back to the root (offset == root), which re-walks
  the whole tree and doubles the file count. Pass the true block size (the root
  block runs to EOF), not an over-long window.

The extractor byte-resyncs past malformed headers, so it is robust to both.

## Extraction COMPLETE — all assets preserved

| Archive | Files | Size | Validation |
|---------|-------|------|-----------|
| `SOUND.REZ` → `extracted/SOUND/` | 2,023 | 137 MB | 100% RIFF/WAVE headers valid |
| `SHOGO.REZ` → `extracted/SHOGO/` | 5,643 | 251 MB | LithTech DTX/ABC/DAT/SPR magics valid |

`extracted/SHOGO/` breakdown: **538 ABC models** (LithTech property-tree,
`Header,` magic), **60 DAT worlds** (levels 01_AMBUSH … 37_GABRIEL), **4,672 DTX
textures** (LithTech DTX, version word `0xFFFFFE`), **211 SPR sprites**, **159
PCX images**, plus the embedded `SRES.DLL`/`OBJECT`/`RIOT` game-logic modules.

## Tooling built

- `tools/rezextract.py` — full REZ extractor: auto-root-detection, recursive
  walk, sentinel/footer handling, byte-resync. Extracts both retail archives.
- `tools/rezscan.py` / `tools/probe_rez.py` / `tools/find_roots.py` /
  `tools/dump_blocks.py` — REZ format discovery & root-hunting scripts.
- `ghidra/DecompileAll.java` — Ghidra headless post-script; dumps every function
  to C pseudocode.

## Decompiled output (`decompiled/`) — 6,455 functions, ~275k lines of C

| File | Functions | Lines |
|------|-----------|-------|
| `Client.exe.c` (client + engine) | 1,750 | 78,979 |
| `mod3_Object.c` (ServerShell) | 1,755 | 64,407 |
| `server.dll.c` (LithTech framework) | 1,481 | 56,576 |
| `mod2_CShellRef.c` (ClientShell) | 1,411 | 72,508 |
| `mod0_credits.c` | 58 | 2,836 |

## Asset formats (DECODED)

### DTX textures — DECODED (3641/3675 → PNG)
```
+0   u32 version (0)
+4   u32 flags (0xfffffffe)
+8   u16 width
+10  u16 height
+12  u16 mipcount (usually 4)
...
+44  palette[256], each 4 bytes stored A,R,G,B
+1068  mip0 indices (8bpp, w*h, row-major), then mip1 (w/2*h/2), ...
```
Tool: `tools/dtx2png.py` (single + `--batch`). 34 zero-dimension files are
placeholders. `GLASS/` subdir uses a 16-bit variant (skipped by default).

### SPR sprites — TRIVIAL
Just a path pointer to a DTX: `u32 ver, u32 x4, u16 len, char path[]`
(e.g. "Textures\flare3.dtx").

### ABC models — DECODED (538/538, exact byte consumption)
Property tree. Each property: `u16 nameLen, name, u32 endOffset, data`.
Top-level props: Header, Geometry, Nodes, Animation.

Geometry data (from decompiled loader FUN_00434ad0):
```
24B  bbox: f32 min.xyz, max.xyz
u32  nNames; u8 nameTable[nNames*2+2]
u32  nFaces
  per face (33B):
    6 x f32 (24B)  3 UV pairs (one per triangle corner)
    u16 x 3        vertex indices
    u8  x 3        per-corner flags
u32  nVerts
u32  nNorms
  per vert (20B):
    3 x f32 position (12B)
    u8 x 4, u16 x 2  (flags/light/unused)
```
Each face is a triangle with its own 3 UVs → expand per-corner to preserve UV
seams. Tools: `tools/ltabc.py` (parser) + `tools/abc2glb.py` (exporter).
Validated: 538/538 models, exact consumption, indices in range.

### DAT worlds — objects/geometry-header DECODED; polygon mesh PENDING

**Header (verified across all 22 retail worlds):**
```
+0   u32 magic (56)                 +4  u32 objects_offset (121)
+8   u32 geometry_offset (== object-list end)   +12 u32 text_len (73)
+16  text blob (text_len bytes): "MasterPalette <path>.dtx;WorldName \"<name>\""
@objects_offset: u32 nObjects, then per object (see grammar below)
... object list ends EXACTLY at geometry_offset (verified byte-exact on all files)
@geometry_offset: geometry section (see below)
```

**Object / property grammar — FULLY CRACKED & VERIFIED** (`tools/dat_walk_objects.py`,
`tools/dat_extract_objects.py`). Every object's byte consumption matches its stored
size exactly, across all 22 worlds:
```
per object:
    u16 objDataSize          # bytes of object data AFTER this field
    u16 nameLen, char cls[]  # class name ("GameStartPoint", "Light", "Sprite", ...)
    u32 nProps
    per prop:
        u16 nameLen, char pname[]
        u8  type             # value type tag
        4 bytes              # type-specific header (usually zero)
        u16 valueLen
        value[valueLen]
    # then seek(objDataSize) to next object
```
Property type tags (from `server.dll` loader `FUN_10007372`):
`1`=vector(3×f32,12B) `Pos`,`Rotation`; `2`=color(3×f32,12B) `LightColor`,`InnerColor`;
`3`=float `LightRadius`,`BrightScale`; `5`=byte `LightObjects`; string=`u16 len + chars`
`Name`. Unknown objects skipped via `seek(objDataSize)`.

Extracted gameplay data (all 22 worlds): **3442 Light, 912 DirLight, 309
DestructableDoor, 222 GameStartPoint, 190 Door, 159 Prop, 122 ScaleSprite, 112
Trigger, 75 ObjectLight, 68 SkyPointer, 63 Sound**, plus every powerup
(ShotgunPowerup×49, JuggernautPowerup×40, FirstAid_10/25, ArmorRepair_100/250, …).
`tools/dat_extract_objects.py <glob>` prints spawn positions, light
pos/radius/color, etc.

**Geometry section — header decoded, body pending.** Geometry starts at
`geometry_offset` (= object-list end). Header layout (locked by anchor scan
`tools/dat_geom_hdr.py`): **9×u32 + a 2-byte string + 22×u32 lands EXACTLY on the
texture-table size field**, then:
```
... u32 texture_table_bytes (1637)   u32 nTextures (37)
@next: nTextures NUL-terminated ".dtx" paths   (e.g. "Textures\Layouts\...dtx")
... after table: run of per-thing flag bytes (values 3–7, ~2081 bytes)
... @u32[0] of geometry header (=210907 here): section/lightmap directory
     u32 nSections, then per section: u16 nameLen, name[] ("Gird1Beam"), + data
```
Header bbox reads as grid-snapped world coords (e.g. −1600,−960,432 … 896,256,2496).

**Authoritative loader: `FUN_00418d80` in `decompiled/Client.exe.c` (lines
12859–13553, ~700 lines)** — the real geometry parser. Called by world loader
`FUN_00418980` (line 12703, "World load: %d ticks"). Array allocation (lines
13118–13139) reveals element sizes: **16B planes, 52B vertices (in-memory form),
48B surfaces, 92B portals, 36B leaves**. The mesh is stored as a **leaf/portal
tree**, NOT flat arrays — empirical flat-array scans (stride 52/34, position@0/14/18)
returned 0 world-scale candidates. A faithful port of `FUN_00418d80`'s read
sequence is required (same technique that cracked REZ/DTX/ABC).

Other loader refs: `FUN_00434270` (world-geometry UV/stage builder, 28-byte
vertex record w/ UV @0xc), "World %s doesn't have PBlocks" `FUN_004129b0`
(physics octree), "World ended" `FUN_00454160` (teardown).

## Next steps (state as of end of session)

**Working & verified (do not re-do):** REZ extraction, DTX→PNG, SPR, ABC→GLB,
Rust `shogo-assets` decoders (DTX+ABC, tested), Bevy `shogo-viewer` MVP (renders
original ABC via Vulkan), DAT object/property grammar (verified on all 22 worlds),
DAT geometry header + texture table.

**Recommended priority for the next agent:**

1. **World polygon geometry (the one hard asset format left).** Do NOT keep
   empirical-guessing flat vertex arrays — the mesh is a leaf/portal tree and
   flat scans fail. Instead **faithfully port the decompiled loader
   `FUN_00418d80`** (`decompiled/Client.exe.c` lines 12859–13553) read-by-read
   into Python, exactly as `tools/ltabc.py` did for the ABC loader
   `FUN_00434ad0`. Validate against the known anchors: texture count=37, table
   bytes=1637, header bbox grid-snapped coords. Element sizes from the
   allocation block (lines 13118–13139): 16/52/48/92/36 bytes. Once positions+
   faces+UVs+per-surface texture index come out in-range, export a world GLB
   (mirror `tools/abc2glb.py`) and add a native `dat` decoder to `shogo-assets`.

2. **Wire textures into the viewer.** ABC files carry NO .dtx references
   (verified on all models) — textures are assigned by game logic. World surfaces
   DO carry texture indices (into the .dtx table), so once (1) works, world
   geometry gets textured via `shogo-assets::dtx` → `StandardMaterial`
   base_color_texture. For standalone models, texture assignment requires
   reversing material→texture mapping from `mod3_Object.dll` (lower priority).

3. **Begin the gameplay rewrite.** Port physics (PBlocks octree, `FUN_004129b0`),
   AI, MCA/on-foot modes, weapon semantics from `Client.exe` +
   `mod2_CShellRef.dll` / `mod3_Object.dll` / `server.dll`. Spawn points, lights,
   doors, triggers are already extracted (see DAT section) — that's the level
   skeleton.

4. Optional: recover class-registration tables for real field names; get the
   original running under Wine for reference; cross-check community LithTech 1.0
   knowledge (OpenLith, ReZolved) — note: web search returned nothing useful
   earlier (tool appeared unavailable), so re-try if needed.

**Environment reminders:** Rust at `~/.cargo` (`source ~/.cargo/env`); Bevy 0.20
built with a restricted feature set (X11 only, NO audio/wayland) to avoid missing
system `-dev` headers — see `rust/Cargo.toml` rationale. Viewer run:
`cd rust && SHOGO_MODEL=../extracted/SHOGO/MODELS/CHARACTERS/GABRIEL.ABC WGPU_BACKEND=vulkan cargo run --release -p shogo-viewer`.
