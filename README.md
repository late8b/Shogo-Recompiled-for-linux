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
| Rust asset decoders | **Done** — `shogo-assets` (DTX, ABC bind pose, DAT v56), unit-tested |
| Bevy runtime | **Ambush playable, exit loads the next DAT** — original DAT/ABC/DTX/PCX/WAV, menu, bumper, combat, save, triggers, doors, objectives |
| DAT world objects/properties | **Done** — grammar verified; cast, triggers, keys, and doors come from the object list |
| DAT world geometry | **Done** — v56 WorldBSP on all 60 worlds; native decoder plus `ltdat.py` / `dat2glb.py` |
| Ghidra decompilation | **Reference only** — 6,455 functions in `decompiled/`; not a rebuilt executable |
| Mission script | **Running** — the loaded DAT's trigger volumes, objectives, patrol keys, keyframers, barricades, sliding doors, and a level change |
| Full campaign | Ambush → Quarters (`02_QUARTERS.DAT`, Calm Before the Storm) at `start1` has been walked. Later exits use the same loader; Quarters → `03_MCA_Dock` has not. Doors draw named WorldModel brushes; a missing brush keeps collision and draws nothing. Transmissions show retail text; objectives stay `Objective {id}`. Continue from the menu still boots Ambush. The save applies only when its mission name matches that world. Retail physics is still in the dump |

## Repository layout

```
docs/FINDINGS.md          master technical writeup (formats, engine, classes)
tools/                    Python pipeline (extraction + format decoders)
  rezextract.py           REZ v1 archive extractor (auto root detection)
  dtx2png.py              DTX texture -> PNG (single + --batch)
  ltabc.py                ABC model parser
  abc2glb.py              ABC -> glTF 2.0 binary (.glb) exporter
  ltdat.py                DAT v56 WorldBSP parser
  dat2glb.py              DAT world -> glTF 2.0 binary (.glb) exporter
extracted/                raw assets pulled from the REZ archives
  SOUND/                  2,023 WAV files
  SHOGO/                  models (.ABC), worlds (.DAT), textures (.DTX), ...
converted/                ready-to-use assets
  textures/               PNG (mirrors the DTX tree)
  models/                 .glb (mirrors the ABC tree)
  worlds/                 .glb (mirrors the DAT tree)
rust/                     Cargo workspace
  crates/shogo-assets/    native DTX, ABC, and DAT decoders (no Bevy dependency)
  crates/shogo-viewer/    Bevy runtime: menu, Ambush, level change, combat, mission script, save
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

# worlds -> glTF binary
python3 tools/dat2glb.py --batch extracted/SHOGO/WORLDS converted/worlds
```

`.glb` files load in Blender (File → Import → glTF 2.0). The Bevy app does
not use them; it reads the retail DAT, ABC, and DTX files.

### Run the game (Rust + Bevy)

The runtime loads retail files directly: `01_AMBUSH.DAT`, character `.ABC`
files, `.DTX` textures, menu `.PCX` art, and `.WAV` music. Nothing is converted
first.

```bash
cd rust
WGPU_BACKEND=vulkan cargo run -p shogo-viewer
```

The window opens on the original menu. **New Game** plays the Shinara District
bumper, then starts **The Ambush** at the authored `start1` point. **Continue**
reads `~/.local/share/shogo-mad/continue.sav`, skips the bumper, and still
starts Ambush. Saved position, health, ammo, and weapon apply only when that
file's mission name matches the world that booted. **Esc** pauses. **F5**
quicksaves.

On foot: mouse or right stick looks, WASD or the left stick moves, Shift or L3
sprints, Space or Cross jumps, left click or R2 fires, right click or L2 aims,
R or Square reloads, Q or Triangle cycles weapons, 1 / 2 pick the rifle, F or
D-pad Up toggles the MCA. Click, Enter, or Cross respawns.

```bash
# another retail world (the multi map still runs the wave sandbox)
SHOGO_WORLD=../extracted/SHOGO/WORLDS/MULTI/MCA_MARITROPA2.DAT cargo run -p shogo-viewer

# one model, orbit camera
SHOGO_MODEL=../extracted/SHOGO/MODELS/CHARACTERS/GABRIEL.ABC cargo run -p shogo-viewer

SHOGO_SKIP_MENU=1   # boot straight into the world
SHOGO_MUTE=1        # no paplay
SHOGO_GRAB=0        # leave the mouse free
SHOGO_SAVE=/tmp/continue.sav
```

Build notes (see `rust/Cargo.toml`): Bevy 0.20 is built with `default-features =
false` plus an explicit feature list so it needs **no system dev headers**.
`bevy_audio` stays off (that needs ALSA headers). Music is the retail WAVs
played through `paplay`. A sliding door plays `OpenStartSound` or
`CloseStartSound` as it starts to open or close (fallbacks
`SOUND/DOORS/DOOROPEN1.WAV` and `DOORCLOSE.WAV`). A barricade break plays
`DestroySound` or `SOUND/PROPS/CRATE.WAV`. Windowing is X11. UI uses
`bevy_ui` / `bevy_text`. The camera uses PCF `ShadowFilteringMethod::Hardware2x2`
and `Bloom::OLD_SCHOOL` (intensity 0.05). When the DAT has lights, those light
the level. One directional casts the shadow map, or the largest spot if the
world has no sun. A distant identity sun aims at the origin. A `DirLight`
with no rotation aims down.

```bash
cd rust
cargo test -p shogo-assets --lib
cargo test -p shogo-viewer
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
  N×20-byte vertices (12 = position). Each face carries its own UVs. Character
  pieces are parent-local; `static_model` is applied as a bind pose so the
  figure stands in one mesh.
- **SPR** — just a path pointer to a DTX.
- **DAT** — DEdit world (v56): object/entity list (spawns, lights, triggers
  with Pos/Rotation, doors with MoveDist/MoveDir/StateFlags), then the root
  WorldBSP (points, surfaces with OPQ UVs, polygons) and the named WorldModels
  after it. `shogo-assets` `read_mission` reads triggers, AI keys, keyframers,
  and doors; `world_models` reads those brushes. `tools/ltdat.py`,
  `tools/dat2glb.py`.

## Goals / roadmap

1. **Asset conversion** — done for textures, models, and worlds.
2. **Native Linux runtime** — done for loading. The Bevy app reads original
   DAT, ABC, DTX, PCX, and WAV through `shogo-assets` and a small PCX/WAV path.
3. **Decompilation** — Ghidra has dumped `Client.exe`, `server.dll`, and the
   four game DLLs to `decompiled/` (6,455 functions). That tree is the
   reference. It is not cleaned up and it does not build.
4. **Gameplay rewrite** — mission 1 is playable (menu, bumper, authored cast,
   two weapons, on-foot/MCA, one continue file). The same loop runs the DAT
   mission graph: trigger volumes, objective ids, `FollowPath` on AI keys,
   keyframers, breakable barricades, and sliding doors. Those doors and
   barricades draw the named WorldModel brush. A missing brush keeps collision
   and draws nothing. An unlocked exit emits one transition, despawns the
   current level, and loads the next `.DAT` in that `WORLDS` folder, keeping
   the player's health, ammo, and weapon. Ambush's `outtahere` has been walked
   into **Calm Before the Storm** (`02_QUARTERS.DAT`) at `start1`. Quarters'
   exits name `03_MCA_Dock` at `start1` and use the same loader; that hop has
   not been walked. A HUD line shaped like `Transmission {id}` shows the retail
   string from `embedded/mod0_credits.dll`. Objective lines still print
   `Objective {id}` (Ambush 1040 is "Fall back to Drop Point Renegade" in that
   table). When the active objective has a walkable exit or touch trigger, the
   HUD marks it with a screen-space diamond, or an edge chevron when it is off
   screen, plus meters. Ambush 1040 marks `outtahere` at (−2304, 160, 2624).
   A landed rifle ray or pulse bolt flashes a cloned material, flinches the
   body after its yaw, and bursts infantry quads or mech tetrahedrons and
   cuboids. Mechs are Andra, Raksha, and Uhlan. Ruin is on foot. Continue from
   the menu still boots Ambush. Saved health, ammo, weapon, and position apply
   only when the save's mission name matches the world just loaded. The retail
   physics octree is still only in the decompiler output.

## Licensing note

Game assets, names, and code referenced here are property of Monolith
Productions / Sierra Entertainment. This is a preservation and interoperability
effort; assets are not redistributed here — bring your own retail copy.
