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
- **Rust workspace** (`rust/`): `shogo-assets` (DTX, ABC with `static_model`
  bind pose, DAT v56 geometry + `GameStartPoint` + level actors +
  `read_mission`) and `shogo-viewer` (Bevy 0.20). Tests: `cargo test -p
  shogo-assets --lib` and `cargo test -p shogo-viewer`.
- **Ghidra** wired to Temurin JDK 21; all binaries decompiled to `decompiled/`
  (6,455 functions). `server.dll.c` + `Client.exe.c` are the key references.
- **DAT object/property grammar** — cracked & verified on all 22 worlds (byte
  consumption exact). 3442 Light, 222 GameStartPoint, 309 doors, 112 triggers,
  all powerups extracted with positions/colors/radii.

## What is DONE — native world + mission 1

Root-model mesh is decoded on **all 60 worlds** (Python and Rust): points,
planes, surfaces (OPQ UVs + texture index), polygons. `tools/ltdat.py` +
`tools/dat2glb.py` still write `converted/worlds/` (60/60 GLB). The name slot
after the 9-u32 WorldModel header is **always 2 bytes**.

The Bevy app loads the original `.DAT` and binds DTX textures. Default world
is **The Ambush** (`WORLDS/01_AMBUSH.DAT`). `SHOGO_WORLD` selects any other
file. `MCA_MARITROPA2.DAT` has no character classes, so it still runs the
wave sandbox.

Playable slice, verified in the window:

- Menu on `INTERFACE/MENU/BACKGRND.PCX`. New Game, Continue, Quit.
- Intro card `INTERFACE/BUMPERS/AMBUSH.PCX` (Shinara District), then the mission.
- Player at `start1` `(3072, 32, 2688)`. Authored cast from the object list:
  8 hostiles (Cronian troopers, Ruin 150s, Sparrowhawk, Vigilance, two Fallen
  Andra 10s) and 3 UCA escorts who do not shoot or take damage. Dropships and
  APCs keep authored height. Trucks sit on the road.
- Character ABC pieces are posed with the parent chain in `static_model`.
  Faction skins are `SKINS/ENEMIES/{STEM}_{FACTION}.DTX`.
- On-foot and MCA movement, assault rifle and pulse rifle, autoreload, HUD.
- Sphere collision and a nav grid. This is not the retail PBlock octree.
- One continue file: `~/.local/share/shogo-mad/continue.sav` (override with
  `SHOGO_SAVE`). Autosave at deploy, F5, and the pause menu. Continue skips
  the bumper and restores position, health, ammo, and body.
- Music: `paplay` loops `SOUND/INTERFACE/LOOP3.WAV` on the menu, plays
  `BUMPER.WAV` into the intro, then `AMBIENT/MUSIC1.WAV` into `WIND3.WAV`.
  `SHOGO_MUTE=1` skips it. Bevy's audio feature stays off (no ALSA headers).

## What is DONE — triggers, doors, objectives

`read_mission` walks the same object list as the cast. It keeps `Trigger`,
`ExitTrigger`, `ObjectiveTrigger`, `TransmissionTrigger`, `AIKey`, `Key`,
`KeyFramer`, `Door`, and `DestructableDoor`. DAT type 6 is an `f32`, which is
how `StateFlags` and `Waveform` are stored. Sky shells (no hit points and no
slide) are skipped. `01_AMBUSH.DAT` doors are breakable only (`barb1` is 100
HP, `MoveDist` 0). `02_QUARTERS.DAT` `mainlift` slides 640 along +Y at speed
300 with flag bit 4. `door2` waits 3 seconds open and has flag bit 1.

`shogo-viewer/src/script.rs` runs that graph. On boot it delivers `Trigger` to
`start`. Handled messages: `Trigger`, `Lock`, `Unlock`, `On`, `FollowPath`,
`SetMovement`, `Destroy`, `Remove`.

| Piece | Runtime |
|-------|---------|
| `TriggerVolume` | Player sphere against `Dims`. Rising edge fires the node when it is player-touchable or an exit. AI touch is separate. |
| `ObjectiveState` | Add / remove / complete id lists. The HUD line is `Objective {id}`. |
| `Transmission` | Shows `Transmission {StringID}` for 6 seconds. The retail string table is not loaded. |
| `ExitTrigger` | Locked until `Unlock`. Then the line is `Mission complete. Next: {world}`. `outtahere` names `02_Quarters` / `start1`. The next DAT is not loaded. |
| `FollowPath` | Numbered `AIKey`s (`b0`, `b1`, …). Authored allies stay on the path. Hostiles leave it once they see the player. |
| `KeyFramer` | `On` moves the named prop along `{BaseKeyName}0`, `{BaseKeyName}1`, … |
| `Barricade` | Breakable door, 72×96×36 cuboid. Shots and the player sphere hit it. |
| `SlidingDoor` | Closed → delay → opening → open → closing when flag bit 4 is set. Bit 1 touch-opens. Bit 2 starts open. Travel time is `MoveDist / Speed`. |

Door flag bits, from the retail float: **1** touch-open, **2** start open, **4**
auto-close after `OpenWaitTime`. Waveforms are pinned so a fully open door
stays open: 0 linear, 1 smoothstep, 2 sine slow-off, 3 cosine slow-on.

A Quarters boot logged 6 sliding doors, 35 barricades, and 82 trigger volumes.
The panels are steel cuboids sized from the travel axis. They are not the
WorldModel brushes. Open and close sounds are not played.

## Decompilation state

`decompiled/` is a Ghidra dump of the retail binaries: 6,455 functions,
`FUN_` names, not a source tree that builds. `Client.exe.c` and `server.dll.c`
are the references for loaders and physics. The Rust game is a behavior
rewrite on top of the decoded assets, not a translation of those functions.

Still only in the dump, not in the runtime:

- Loading the next mission when an exit finishes. The machine records the
  destination and the start point, then stops.
- Retail door brushes (the named WorldModel section) and open/close sounds.
- Transmission text from the string table. The HUD shows the numeric id.
- Retail physics (`FUN_004129b0`, PBlocks).
- The rest of the campaign loop, inventory, and the original weapon set.
- Extra WorldModels after the root BSP, and the named section directory.

## NEXT ACTION (priority order)
1. **Load the next world.** When `Machine::finished` is `02_Quarters` /
   `start1`, swap `SHOGO_WORLD` to `WORLDS/02_QUARTERS.DAT` and respawn there.
   The script already records that pair. Nothing reads it outside tests.
2. **Door brushes.** Replace the sliding-door and barricade cuboids with the
   WorldModel BSP named by each door.
3. **Transmission copy.** Resolve `StringID` through the retail string table
   instead of printing the number.
4. **Physics closer to LithTech.** PBlocks in the DAT are parsed by `ltdat.py`
   but the viewer still uses a triangle mesh and a sphere.

## Key files
- `docs/FINDINGS.md` — master spec (REZ/DTX/SPR/ABC/DAT formats, class names).
- `README.md` — overview + status table + quick start.
- `tools/` — Python converters (`dtx2png.py`, `abc2glb.py`, `ltabc.py`,
  `ltdat.py`, `dat2glb.py`, `dat_extract_objects.py`, …).
- `rust/crates/shogo-assets/src/{dtx.rs,abc.rs,dat.rs}` — native decoders.
  `dat.rs` `read_mission` reads triggers, keys, keyframers, and doors. Type 6
  properties are floats.
- `rust/crates/shogo-viewer/src/` — `main.rs` world load, `front.rs` menu /
  intro / save, `combat.rs` cast and weapons, `script.rs` mission graph and
  doors, `tune.rs` WAV playback, `collide.rs`, `nav.rs`, `arena.rs`,
  `input.rs`, `pad.rs`.
- `rust/Cargo.toml` — Bevy 0.20 features (X11, UI, no `bevy_audio`, no wayland).
- `decompiled/Client.exe.c` — world/model loaders (`FUN_00418d80` geometry,
  `FUN_00434ad0` ABC geometry, `FUN_00434270` world UV builder).
- `decompiled/server.dll.c` — object/property loader `FUN_10007372` (Light props).
- `decompiled/mod3_Object.c` — door property load `FUN_100284c0` and the
  open/close tick `FUN_100287e0`.
- `extracted/SHOGO/WORLDS/01_AMBUSH.DAT` — the mission the runtime boots.
  `WORLDS/MULTI/MCA_MARITROPA2.DAT` is the wave-sandbox map.

## Environment
- Linux Mint 22.3 (Cinnamon/X11), `DISPLAY=:0`; JDK 21 `~/.local/opt/jdk-21...`;
  Ghidra `~/Downloads/ghidra_12.1.4_PUBLIC`; Rust `~/.cargo` (`source ~/.cargo/env`);
  PIL 10.2.0; cc 13.3.0.
- Vulkan works (AMD RADV RENOIR / Mesa). No x11/wayland/alsa/vulkan `-dev`
  headers → Bevy built with restricted features (see `rust/Cargo.toml`).
- Game: `cd rust && WGPU_BACKEND=vulkan cargo run -p shogo-viewer`.
  `SHOGO_SKIP_MENU=1` skips the title. `SHOGO_GRAB=0` leaves the mouse free.
  A wireless DualSense needs the user in group `input`; keyboard still works.
- Git repo exists (`master`). Converted GLBs are gitignored (regenerate via tools).
