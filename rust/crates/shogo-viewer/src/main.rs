//! Native viewer for original Shogo: MAD assets.
//!
//! Instead of going through a converted intermediate (GLB), this loads the
//! **original `.ABC` model directly** via the `shogo-assets` decoder and builds
//! a Bevy mesh from it. That is the seed of the native runtime: it proves we can
//! take a retail game asset and render it in a Bevy app on Linux with no
//! conversion step, preserving the original player data path.
//!
//! Usage:
//!   SHOGO_MODEL=../extracted/SHOGO/MODELS/CHARACTERS/GABRIEL.ABC cargo run -p shogo-viewer
//!   SHOGO_WORLD=../extracted/SHOGO/WORLDS/01_AMBUSH.DAT cargo run -p shogo-viewer
//!
//! Controls (keyboard / mouse, and a wireless DualSense on /dev/input):
//!   model — right-drag to orbit, scroll to zoom
//!   world — mouse or right stick to look, WASD or left stick to move,
//!           Shift or L3 to sprint, Space or Cross to jump,
//!           left click or R2 to fire, right click or L2 to aim,
//!           R or Square to reload, Q or Triangle to cycle,
//!           1 assault rifle, 2 pulse rifle,
//!           F or D-Pad Up to toggle on-foot / MCA scale,
//!           click, Enter, or Cross to respawn, Esc pauses
//!
//! The game opens on the menu. New Game plays the Ambush bumper, then mission 1.
//! Continue skips the bumper and loads the mission named in the save.
//! Esc pauses. F5 or the pause menu writes a continue file. SHOGO_SKIP_MENU=1
//! boots straight into the mission.

mod arena;
mod collide;
mod combat;
mod door_audio;
mod feedback;
mod front;
mod script;
mod input;
mod nav;
mod pad;
mod pblock;
mod transmission;
mod tune;
mod waypoint;

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Hdr};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::ecs::observer::On;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

fn main() {
    let boot = if std::env::var("SHOGO_SKIP_MENU").ok().as_deref() == Some("1") {
        front::Screen::Mission
    } else {
        front::Screen::Menu
    };
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.45, 0.62, 0.78)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Shogo MAD — native viewer".into(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_state(boot)
        .init_resource::<input::Actions>()
        .init_resource::<pad::Pads>()
        .init_resource::<front::Paused>()
        .init_resource::<front::Launch>()
        .insert_resource(tune::Mixer::start())
        .insert_resource(transmission::Dialogue::load())
        .add_systems(OnEnter(front::Screen::Mission), (
            front::dismiss,
            spawn_stage,
            load_asset,
            front::apply_launch,
            arena::assemble,
            combat::spawn_fight,
            script::place,
            front::mission_started,
        ).chain())
        .add_systems(Update, input::gather)
        .add_systems(
            Update,
            (
                orbit_camera,
                player_move,
                follow_sky,
                combat::weapons,
                combat::bolts,
                script::drive,
                combat::waves,
                script::touch_volumes,
                waypoint::project,
                script::doors,
                combat::enemies,
                // enemies() rewrites yaw each tick. The flinch is applied after that write.
                feedback::flinch,
                feedback::flash,
                feedback::debris,
                combat::muzzle_flashes,
                combat::hud,
                script::readout,
                perform_transition,
            )
                .chain()
                .after(input::gather)
                .run_if(front::sim_runs),
        )
        .add_observer(queue_transition)
        .add_observer(door_audio::on_barricade)
        .add_observer(feedback::on_hit)
        .init_resource::<feedback::DebrisBank>();
    front::install(&mut app);
    app.run();
}

/// Orbit-camera state attached to the camera entity.
#[derive(Component)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    dist: f32,
    target: Vec3,
}

/// First-person body. The camera sits at the sphere center.
#[derive(Component)]
pub(crate) struct Player {
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) vel: Vec3,
    pub(crate) grounded: bool,
    pub(crate) health: f32,
    /// Seconds of red flash left after a hit.
    pub(crate) hurt: f32,
    /// 0 = assault rifle, 1 = pulse rifle.
    pub(crate) weapon: u8,
    pub(crate) fire_cd: f32,
    /// 0 is on-foot, 1 is MCA. Lerps over about half a second.
    pub(crate) scale: f32,
    pub(crate) want_mca: bool,
    pub(crate) scale_lock: f32,
    pub(crate) radius: f32,
    /// 0 is hip fire, 1 is aimed.
    pub(crate) ads: f32,
    pub(crate) ammo: [u16; 2],
    pub(crate) reserve: [u16; 2],
    /// Seconds left on a reload. 0 means the gun is ready.
    pub(crate) reloading: f32,
}

/// Geometry, cast, triggers, and props that belong to the loaded world.
/// The player, their guns, and the HUD do not carry this, so a transition
/// can clear the map and keep health, ammo, and weapons.
#[derive(Component)]
pub(crate) struct LevelEntity;

/// Path of the world currently in memory. Boot still comes from `world_path`.
#[derive(Resource, Clone)]
pub(crate) struct CampaignFile(pub String);

/// Named `GameStartPoint` to use the next time the world loader places the player.
#[derive(Resource)]
struct SpawnRequest(String);

/// Exit resolved to a real `.DAT`, waiting for the end of this tick.
#[derive(Resource)]
struct PendingTransition {
    path: String,
    spawn: String,
}

#[derive(Resource)]
pub(crate) struct WorldCollide(pub(crate) collide::Collider);

/// Retail PBlock grid for the player. AI still uses [`WorldCollide`] and the nav grid.
#[derive(Resource)]
pub(crate) struct WorldPhysics(pub(crate) pblock::PBlocks);

/// Where a dead player comes back, and where the fight was first placed.
#[derive(Resource)]
pub(crate) struct Checkpoint {
    pub(crate) pos: Vec3,
    pub(crate) yaw: f32,
}

/// Mouse look is captured until Esc is pressed.
#[derive(Resource)]
pub(crate) struct LookGrabbed(bool);

const WALK: f32 = 280.0;
const RUN: f32 = 560.0;
const GRAVITY: f32 = 1600.0;
const JUMP: f32 = 420.0;
pub(crate) const BODY: f32 = 32.0;
/// Larger than every enemy health pool in the roster.
pub(crate) const PLAYER_HP: f32 = 500.0;
pub(crate) const MAG_SIZE: [u16; 2] = [30, 8];
/// Spare rounds behind the magazine. Ambush has no ammo pickups. The authored
/// hostiles plus `barb1` and `barb2` are 2240 hit points, and the rifle hits
/// for 15, so 30+90 (1800 damage) cannot clear the mission on the rifle alone.
/// 30+360 is 390 shots: about two in five can miss and the cast is still
/// clearable. The pulse rifle stays a short sidearm. A pack with nothing in
/// the mag and nothing in reserve does not invent rounds.
pub(crate) const RESERVE_SIZE: [u16; 2] = [360, 24];

/// Continue keeps whatever rounds are still in the file. Both guns empty is
/// the starting pack: reload cannot pull rounds out of an empty reserve, and
/// Ambush never hands more out.
pub(crate) fn carried_rounds(ammo: [u16; 2], reserve: [u16; 2]) -> ([u16; 2], [u16; 2]) {
    if ammo == [0, 0] && reserve == [0, 0] {
        (MAG_SIZE, RESERVE_SIZE)
    } else {
        (ammo, reserve)
    }
}

/// Blended on-foot / MCA movement. `scale` 0 is the infantry body.
pub(crate) struct BodyTune {
    pub radius: f32,
    pub walk: f32,
    pub sprint: f32,
    pub accel: f32,
    pub brake: f32,
    pub yaw_rate: f32,
    pub jump: f32,
    pub gravity: f32,
    pub fov: f32,
}

impl BodyTune {
    pub fn at(scale: f32) -> Self {
        let t = {
            let t = scale.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Self {
            radius: lerp(BODY, 108.0),
            walk: lerp(WALK, 460.0),
            sprint: lerp(RUN, 920.0),
            accel: lerp(14_000.0, 520.0),
            brake: lerp(14_000.0, 260.0),
            yaw_rate: lerp(18.0, 0.9),
            jump: lerp(JUMP, 860.0),
            gravity: lerp(GRAVITY, 1_050.0),
            fov: lerp(75.0_f32.to_radians(), 98.0_f32.to_radians()),
        }
    }
}

fn spawn_stage(mut commands: Commands) {
    // SHOGO_GRAB=0 leaves the mouse free so a still frame shows the spawn view.
    let grab = std::env::var("SHOGO_GRAB").ok().as_deref() != Some("0");
    commands.insert_resource(LookGrabbed(grab));
    commands.spawn((
        Camera3d::default(),
        Camera {
            // Drawn after the sky camera. `None` loads the shared HDR target
            // the sky camera just cleared. Bloom requires `Hdr`, so a sky
            // camera that is not HDR writes a different texture and this one
            // keeps last frame: additive flashes and the viewmodel smear.
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            near: 0.5,
            far: 100_000.0,
            fov: BodyTune::at(0.0).fov,
            ..default()
        }),
        Transform::default(),
        bevy::light::ShadowFilteringMethod::Hardware2x2,
        bevy::post_process::bloom::Bloom::OLD_SCHOOL,
        AmbientLight {
            color: Color::WHITE,
            brightness: 180.0,
            affects_lightmapped_meshes: true,
        },
        Player {
            yaw: 0.0,
            pitch: 0.0,
            vel: Vec3::ZERO,
            grounded: false,
            health: PLAYER_HP,
            hurt: 0.0,
            weapon: 0,
            fire_cd: 0.0,
            scale: 0.0,
            want_mca: false,
            scale_lock: 0.0,
            radius: BODY,
            ads: 0.0,
            ammo: MAG_SIZE,
            reserve: RESERVE_SIZE,
            reloading: 0.0,
        },
    ));
}

static ACTIVE_WORLD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub(crate) fn remember_world(path: &str) {
    if let Ok(mut slot) = ACTIVE_WORLD.lock() {
        *slot = Some(path.to_string());
    }
}

pub(crate) fn forget_world() {
    if let Ok(mut slot) = ACTIVE_WORLD.lock() {
        *slot = None;
    }
}

pub(crate) fn world_path() -> Option<String> {
    if let Ok(slot) = ACTIVE_WORLD.lock() {
        if let Some(path) = slot.as_ref() {
            if std::path::Path::new(path).is_file() {
                return Some(path.clone());
            }
        }
    }
    if let Some(p) = std::env::var("SHOGO_WORLD").ok().filter(|p| !p.is_empty()) {
        if std::path::Path::new(&p).is_file() {
            return Some(p);
        }
        eprintln!("shogo-viewer: SHOGO_WORLD={p} is not a file");
    }
    // Campaign starts at mission 1. SHOGO_WORLD still selects any other .DAT.
    let campaign = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
    if campaign.is_file() {
        return Some(campaign.to_string_lossy().into_owned());
    }
    let bundled = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extracted/SHOGO/WORLDS/MULTI/MCA_MARITROPA2.DAT");
    if bundled.is_file() {
        return Some(bundled.to_string_lossy().into_owned());
    }
    None
}

fn model_path() -> Option<String> {
    std::env::var("SHOGO_MODEL").ok().filter(|p| !p.is_empty())
}

/// Load either a world (`SHOGO_WORLD`) or a character model (`SHOGO_MODEL`).
fn load_asset(
    launch: Res<front::Launch>,
    mut commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    images: ResMut<Assets<Image>>,
    players: Query<(&mut Player, &mut Transform)>,
    orbit_q: Query<&mut Orbit>,
) {
    // Forget the previous exit. New Game then boots SHOGO_WORLD or Ambush.
    // Continue boots the mission named in the save.
    forget_world();
    if let Some(path) = boot_path(&launch) {
        commands.insert_resource(CampaignFile(path.clone()));
        load_world(path, None, commands, meshes, materials, images, players);
    } else {
        load_model(commands, meshes, materials, orbit_q);
    }
}

/// Load the ABC model named by the SHOGO_MODEL env var (once) and spawn it.
fn load_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut orbit_q: Query<&mut Orbit>,
) {
    let path = match model_path() {
        Some(p) => p,
        None => {
            info!("set SHOGO_MODEL (.ABC) or SHOGO_WORLD (.DAT)");
            return;
        }
    };

    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            error!("failed to read {path}: {e}");
            return;
        }
    };
    let name = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());

    let model = match shogo_assets::abc::decode(&data, &name) {
        Ok(m) => m,
        Err(e) => {
            error!("failed to decode {path}: {e}");
            return;
        }
    };
    info!(
        "loaded {} ({} corner verts, {} tris)",
        path,
        model.positions.len(),
        model.tri_count()
    );

    let mesh = build_mesh(&model);
    let mesh_handle = meshes.add(mesh);

    let material = StandardMaterial {
        base_color: Color::srgb(0.85, 0.85, 0.9),
        perceptual_roughness: 0.9,
        metallic: 0.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    };

    // Fit the orbit target/distance to the model bounds so it's framed nicely.
    let (center, radius) = bounds(&model);

    commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(materials.add(material)),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.6, 0.0)),
    ));

    // Frame the orbit camera on the model: center the target on the model's
    // (Y-up) centroid and place the camera far enough back to see it all.
    let center_yup = lt_to_yup([center.x, center.y, center.z]);
    let dist = radius * 2.2;
    for mut orbit in orbit_q.iter_mut() {
        orbit.target = center_yup;
        orbit.dist = dist;
    }
    info!(
        "framed camera: center=({:.1},{:.1},{:.1}) radius={:.1} dist={:.1}",
        center_yup.x, center_yup.y, center_yup.z, radius, dist
    );
}

/// Load a retail `.DAT` world and spawn one mesh per texture.
fn load_world(
    path: String,
    spawn_name: Option<String>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut players: Query<(&mut Player, &mut Transform)>,
) {
    remember_world(&path);
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            error!("failed to read {path}: {e}");
            return;
        }
    };
    let world = match shogo_assets::dat::decode(&data) {
        Ok(w) => w,
        Err(e) => {
            error!("failed to decode {path}: {e}");
            return;
        }
    };
    let title = shogo_assets::dat::world_title(&data).unwrap_or_else(|| world.name.clone());
    info!(
        "loaded {title} — {path} ({} points, {} polys, {} tris, {} textures)",
        world.point_count,
        world.poly_count,
        world.tri_count(),
        world.textures.len()
    );

    let asset_root = find_asset_root(std::path::Path::new(&path));
    let mut textured = 0usize;
    for group in &world.groups {
        let positions: Vec<[f32; 3]> = group
            .positions
            .iter()
            .map(|p| {
                let g = world_pos(*p);
                [g.x, g.y, g.z]
            })
            .collect();
        let tex_name = world
            .textures
            .get(group.texture as usize)
            .map(|s| s.as_str())
            .unwrap_or("");
        // `Sky.dtx` is the portal placeholder (nearly white). The visible sky
        // is the camera skybox, so these faces are not drawn.
        if is_sky_portal(tex_name) {
            continue;
        }
        let decoded = asset_root
            .as_ref()
            .and_then(|root| resolve_ci(root, tex_name))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| shogo_assets::dtx::decode(&bytes).ok());
        let (tw, th) = decoded
            .as_ref()
            .map(|t| (t.width as f32, t.height as f32))
            .filter(|(w, h)| *w > 0.0 && *h > 0.0)
            .unwrap_or((64.0, 64.0));
        // OPQ u/v are in pixels. Divide by the texture size and let the sampler
        // repeat, so a polygon that spans many tiles interpolates correctly.
        let uvs: Vec<[f32; 2]> = group
            .uvs
            .iter()
            .map(|[u, v]| [u / tw, 1.0 - v / th])
            .collect();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(group.indices.clone()));
        mesh.compute_area_weighted_normals();

        // A missing file is still a real surface. Grass in this level is ground
        // beside the sidewalk, so it gets a flat color instead of a hole.
        textured += decoded.is_some() as usize;
        let base_color = if decoded.is_some() {
            Color::WHITE
        } else if tex_name.to_ascii_lowercase().contains("grass") {
            Color::srgb(0.22, 0.38, 0.16)
        } else {
            Color::srgb(0.35, 0.35, 0.35)
        };
        let material = StandardMaterial {
            base_color,
            base_color_texture: decoded.map(|tex| {
                images.add(rgba_image(tex.width, tex.height, tex.rgba))
            }),
            perceptual_roughness: 1.0,
            metallic: 0.0,
            reflectance: 0.0,
            unlit: false,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        commands.spawn((
            LevelEntity,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
            Transform::default(),
        ));
    }
    info!("bound {textured}/{} textures", world.groups.len());
    spawn_dat_lights(&mut commands, &data);

    let mut tris = Vec::new();
    for group in &world.groups {
        let name = world
            .textures
            .get(group.texture as usize)
            .map(|s| s.as_str())
            .unwrap_or("");
        // Sky portals are invisible. Leaving them in the collider creates a
        // floor the search will walk along while the picture stays empty.
        if is_sky_portal(name) {
            continue;
        }
        for tri in group.indices.chunks_exact(3) {
            let p = |i: u32| world_pos(group.positions[i as usize]);
            tris.push([p(tri[0]), p(tri[1]), p(tri[2])]);
        }
    }
    let ntris = tris.len();
    let collider = collide::Collider::from_tris(tris);
    let physics = pblock::PBlocks::from_dat(&data);
    if let Some(grid) = physics.as_ref() {
        let dims = grid.dims();
        info!(
            "pblock grid {}x{}x{} block {:.0}",
            dims[0], dims[1], dims[2], grid.block_size().x
        );
    } else {
        warn!("no PBlock table; player movement uses the triangle sphere");
    }

    // World positions are Y-up. A named start (the exit's StartPointName) wins.
    let starts = shogo_assets::dat::start_points(&data).unwrap_or_default();
    let chosen = spawn_name.as_deref().and_then(|want| {
        starts
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(want))
    });
    let chosen = chosen.or(starts.first());
    let (spawn, face, pitch) = chosen
        .map(|s| {
            let feet = world_pos(s.pos);
            let floor = if let Some(grid) = physics.as_ref() {
                grid.floor_y(feet.x, feet.z, feet.y + 128.0)
            } else {
                collider.floor_y(feet.x, feet.z, feet.y + 128.0)
            };
            let y = floor.map(|fy| fy + BODY).unwrap_or(feet.y);
            let (yaw, pitch) = facing(s.rot);
            (Vec3::new(feet.x, y, feet.z), yaw, pitch)
        })
        .unwrap_or_else(|| {
            let min = world_pos(world.min);
            let max = world_pos(world.max);
            ((min + max) * 0.5 + Vec3::Y * 80.0, 0.0, 0.0)
        });
    commands.insert_resource(WorldCollide(collider));
    if let Some(grid) = physics {
        commands.insert_resource(WorldPhysics(grid));
    } else {
        commands.remove_resource::<WorldPhysics>();
    }
    commands.insert_resource(Checkpoint {
        pos: spawn,
        yaw: face,
    });
    for (mut player, mut tf) in &mut players {
        player.yaw = face;
        player.pitch = pitch;
        player.vel = Vec3::ZERO;
        player.grounded = true;
        tf.translation = spawn;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, face, pitch, 0.0);
    }
    spawn_sky(
        &mut commands,
        meshes.as_mut(),
        materials.as_mut(),
        images.as_mut(),
        asset_root.as_deref(),
        spawn,
    );
    info!(
        "player spawn ({:.0},{:.0},{:.0}) yaw {:.2}, {} collision tris",
        spawn.x,
        spawn.y,
        spawn.z,
        face,
        ntris
    );
}

/// Sky pass. It shares the bloom camera's HDR target, clears that target, and
/// does not present. `Tonemapping::None` leaves the cleared sky in the buffer
/// for the world camera to draw on. A non-HDR sky camera would clear a
/// different texture, and the bloom camera's `ClearColorConfig::None` would
/// keep last frame.
fn sky_camera_parts(fov: f32) -> (Camera, Projection, Tonemapping) {
    (
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.45, 0.62, 0.78)),
            output_mode: CameraOutputMode::Skip,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            near: 0.5,
            far: 100_000.0,
            fov,
            ..default()
        }),
        Tonemapping::None,
    )
}

fn sky_camera(fov: f32) -> impl Bundle {
    let (camera, projection, tonemap) = sky_camera_parts(fov);
    // Camera3d would otherwise enable dither, which warns when tonemapping is off.
    (camera, projection, Hdr, tonemap, DebandDither::Disabled)
}

/// Follows the camera's position and stays world-aligned, so the clouds don't
/// stick to the view.
#[derive(Component)]
struct SkyRoot;

/// Renders only the sky, before the world camera.
#[derive(Component)]
struct SkyCam;

pub(crate) fn is_sky_portal(name: &str) -> bool {
    name.rsplit(['\\', '/'])
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("sky.dtx")
}

fn follow_sky(
    players: Query<(&Transform, &Projection), With<Player>>,
    mut skies: Query<&mut Transform, (With<SkyRoot>, Without<Player>, Without<SkyCam>)>,
    mut cams: Query<
        (&mut Transform, &mut Projection),
        (With<SkyCam>, Without<Player>, Without<SkyRoot>),
    >,
) {
    let Ok((player, proj)) = players.single() else {
        return;
    };
    let fov = match proj {
        Projection::Perspective(p) => p.fov,
        _ => BodyTune::at(0.0).fov,
    };
    for mut sky in &mut skies {
        sky.translation = player.translation;
    }
    for (mut cam, mut cam_proj) in &mut cams {
        *cam = *player;
        if let Projection::Perspective(p) = &mut *cam_proj {
            p.fov = fov;
        }
    }
}

/// PCF shadows from the level's own lights. Cascades cover a few dozen body
/// lengths; the engine default of 150 units would stop at the player's feet.
fn spawn_dat_lights(commands: &mut Commands, data: &[u8]) {
    let lights = shogo_assets::dat::read_lights(data).unwrap_or_default();
    let cascades: bevy::light::CascadeShadowConfig = bevy::light::CascadeShadowConfigBuilder {
        num_cascades: 3,
        minimum_distance: 1.0,
        first_cascade_far_bound: 280.0,
        maximum_distance: 2400.0,
        overlap_proportion: 0.2,
    }
    .into();
    let mut suns: Vec<_> = lights
        .iter()
        .filter(|light| light.kind == shogo_assets::dat::LightKind::Directional)
        .collect();
    let mut spots: Vec<_> = lights
        .iter()
        .filter(|light| light.kind == shogo_assets::dat::LightKind::Spot && light.radius >= 200.0)
        .collect();
    let mut points: Vec<_> = lights
        .iter()
        .filter(|light| light.kind == shogo_assets::dat::LightKind::Point && light.radius >= 500.0)
        .collect();
    spots.sort_by(|a, b| b.radius.total_cmp(&a.radius));
    points.sort_by(|a, b| b.radius.total_cmp(&a.radius));
    spots.truncate(12);
    points.truncate(6);
    if suns.is_empty() && spots.is_empty() && points.is_empty() {
        commands.spawn((
            LevelEntity,
            DirectionalLight {
                illuminance: 9_000.0,
                shadow_maps_enabled: true,
                ..default()
            },
            cascades,
            Transform::from_rotation(Quat::from_rotation_arc(
                Vec3::NEG_Z,
                Vec3::new(-0.35, -0.85, -0.4).normalize(),
            )),
        ));
        return;
    }
    let spot_shadow = suns.is_empty();
    for (i, light) in suns.drain(..).enumerate() {
        let dir = sun_direction(light);
        commands.spawn((
            LevelEntity,
            DirectionalLight {
                color: light_color(light.color),
                illuminance: 9_000.0 * light.bright.clamp(0.15, 4.0),
                shadow_maps_enabled: i == 0,
                ..default()
            },
            cascades.clone(),
            aim(Vec3::ZERO, dir),
        ));
    }
    for (i, light) in spots.iter().enumerate() {
        let range = light.radius.max(80.0);
        let outer = (light.fov_deg * 0.5).to_radians().clamp(0.08, std::f32::consts::FRAC_PI_2 - 0.02);
        commands.spawn((
            LevelEntity,
            SpotLight {
                color: light_color(light.color),
                intensity: 8.0e5 * light.bright.clamp(0.2, 4.0),
                range,
                radius: 0.0,
                shadow_maps_enabled: spot_shadow && i == 0,
                outer_angle: outer,
                inner_angle: outer * 0.75,
                ..default()
            },
            aim(world_pos(light.pos), spot_direction(light)),
        ));
    }
    for light in points {
        let range = light.radius.max(80.0);
        commands.spawn((
            LevelEntity,
            PointLight {
                color: light_color(light.color),
                intensity: light.bright.clamp(0.2, 4.0) * range * range * 0.35,
                range,
                radius: 0.0,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(world_pos(light.pos)),
        ));
    }
}

fn light_color(c: [f32; 3]) -> Color {
    Color::linear_rgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0))
}

fn rotation_is_identity(q: [f32; 4]) -> bool {
    q[0].abs() < 1.0e-4 && q[1].abs() < 1.0e-4 && q[2].abs() < 1.0e-4 && (q[3] - 1.0).abs() < 1.0e-3
}

/// +Z through the engine's transposed quaternion. Identity is +Z.
fn lith_forward(q: [f32; 4]) -> Vec3 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let len2 = x * x + y * y + z * z + w * w;
    if len2 < 1.0e-8 {
        return Vec3::Z;
    }
    let s = 2.0 / len2;
    Vec3::new(
        s * (x * z - w * y),
        s * (y * z + w * x),
        1.0 - s * (x * x + y * y),
    )
}

fn sun_direction(light: &shogo_assets::dat::LevelLight) -> Vec3 {
    let pos = world_pos(light.pos);
    if rotation_is_identity(light.rotation) && pos.length() > 1_000.0 {
        return -pos.normalize_or_zero();
    }
    let dir = lith_forward(light.rotation);
    if dir.length_squared() < 1.0e-6 {
        Vec3::new(-0.3, -0.8, -0.4).normalize()
    } else {
        dir.normalize()
    }
}

fn spot_direction(light: &shogo_assets::dat::LevelLight) -> Vec3 {
    if rotation_is_identity(light.rotation) {
        return Vec3::NEG_Y;
    }
    let dir = lith_forward(light.rotation);
    if dir.length_squared() < 1.0e-6 {
        Vec3::NEG_Y
    } else {
        dir.normalize()
    }
}

fn aim(pos: Vec3, dir: Vec3) -> Transform {
    let dir = if dir.length_squared() < 1.0e-6 {
        Vec3::NEG_Y
    } else {
        dir.normalize()
    };
    let up = if dir.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    Transform::from_translation(pos).looking_to(dir, up)
}

fn spawn_sky(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    root: Option<&std::path::Path>,
    at: Vec3,
) {
    let Some(root) = root else {
        return;
    };
    // One inward-facing cube so every view direction has sky, not just -Z.
    let Some(image) = load_dtx(root, "Textures/SKY/SKY1.DTX")
        .or_else(|| load_dtx(root, "Textures/ENVIRONMENTMAPS/CLOUDS/CLOUDS_UP.DTX"))
    else {
        return;
    };
    commands.spawn((
        LevelEntity,
        SkyCam,
        Camera3d::default(),
        sky_camera(BodyTune::at(0.0).fov),
        Transform::from_translation(at),
        RenderLayers::layer(1),
    ));
    commands.spawn((
        LevelEntity,
        SkyRoot,
        Visibility::default(),
        RenderLayers::layer(1),
        Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(at).with_scale(Vec3::splat(8_000.0)),
    ));
}

pub(crate) fn load_dtx(root: &std::path::Path, rel: &str) -> Option<Image> {
    let path = resolve_ci(root, rel)?;
    let bytes = std::fs::read(path).ok()?;
    let tex = shogo_assets::dtx::decode(&bytes).ok()?;
    Some(rgba_image(tex.width, tex.height, tex.rgba))
}

pub(crate) fn rgba_image(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.mipmap_filter = ImageFilterMode::Nearest;
    sampler.set_address_mode(ImageAddressMode::Repeat);
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

/// Walk up from the `.DAT` until a sibling `TEXTURES` directory appears.
pub(crate) fn find_asset_root(dat: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut dir = dat.parent()?;
    for _ in 0..6 {
        if dir.join("TEXTURES").is_dir() || dir.join("textures").is_dir() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
    None
}

/// Case-insensitive path join. Retail DTX files are uppercase; DAT paths are not.
pub(crate) fn resolve_ci(root: &std::path::Path, rel: &str) -> Option<std::path::PathBuf> {
    let mut cur = root.to_path_buf();
    for part in rel.replace('\\', "/").split('/') {
        if part.is_empty() {
            continue;
        }
        let hit = std::fs::read_dir(&cur).ok()?.find_map(|e| {
            let e = e.ok()?;
            e.file_name()
                .to_str()
                .filter(|n| n.eq_ignore_ascii_case(part))
                .map(|_| e.path())
        })?;
        cur = hit;
    }
    cur.is_file().then_some(cur)
}

/// Convert a LithTech ABC model into a Bevy `Mesh`.
///
/// LithTech is Z-up / right-handed; Bevy (like glTF) is Y-up / right-handed.
/// The change of basis `(x, y, z) -> (x, z, -y)` is a rotation about X, so it
/// preserves handedness and winding — no index flipping needed. Normals are
/// computed by Bevy from the (already de-indexed, per-corner) geometry.
fn build_mesh(model: &shogo_assets::AbcModel) -> Mesh {
    let positions: Vec<[f32; 3]> = model
        .positions
        .iter()
        .map(|p| {
            let g = lt_to_yup(*p);
            [g.x, g.y, g.z]
        })
        .collect();

    let uvs: Vec<[f32; 2]> = model
        .uvs
        .iter()
        .map(|[u, v]| [*u, 1.0 - v])
        .collect();

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(model.indices.clone()));
    // ABC stores per-corner vertices with no shared normals; compute area-weighted
    // smooth normals here.
    mesh.compute_area_weighted_normals();
    mesh
}

/// ABC models are Z-up. Rotate them about X into Bevy's Y-up frame.
fn lt_to_yup(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

/// World BSP positions are already Y-up. Road, sidewalk, and grass normals
/// point along +Y. Treating Z as up stands the player on a building wall.
pub(crate) fn world_pos(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Axis-aligned bounds (center + radius) in LithTech space, computed from positions.
fn bounds(model: &shogo_assets::AbcModel) -> (Vec3, f32) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in &model.positions {
        let v = Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
    }
    let center = (min + max) * 0.5;
    let radius = (max - min).length() * 0.5;
    (center, radius.max(1.0))
}

fn player_move(
    time: Res<Time>,
    actions: Res<input::Actions>,
    collide: Option<Res<WorldCollide>>,
    physics: Option<Res<WorldPhysics>>,
    mut players: Query<(&mut Player, &mut Transform, &mut Projection)>,
    enemies: Query<(&combat::Enemy, &Transform), Without<Player>>,
    doors: Query<(&script::Barricade, &Transform), Without<Player>>,
    slides: Query<(&script::SlidingDoor, &Transform), Without<Player>>,
) {
    if collide.is_none() && physics.is_none() {
        return;
    }
    let dt = time.delta_secs().min(0.05);

    for (mut player, mut tf, mut proj) in &mut players {
        player.scale_lock = (player.scale_lock - dt).max(0.0);
        if actions.toggle_scale && player.scale_lock <= 0.0 && player.health > 0.0 {
            player.want_mca = !player.want_mca;
            player.scale_lock = 0.4;
            info!(
                "body scale -> {}",
                if player.want_mca { "MCA" } else { "on-foot" }
            );
        }
        let target = if player.want_mca { 1.0 } else { 0.0 };
        player.scale = approach_f(player.scale, target, dt / 0.45);
        let tune = BodyTune::at(player.scale);
        player.radius = tune.radius;
        let ads_target = if actions.aim && player.health > 0.0 {
            1.0
        } else {
            0.0
        };
        player.ads = approach_f(player.ads, ads_target, dt * 7.0);
        if let Projection::Perspective(persp) = &mut *proj {
            persp.fov = tune.fov * (1.0 - 0.2 * player.ads);
        }

        if player.health <= 0.0 {
            player.vel = Vec3::ZERO;
            continue;
        }

        let look_scale = 1.0 - 0.45 * player.ads;
        player.yaw -= actions.look.x * look_scale;
        player.pitch = (player.pitch + actions.look.y * look_scale).clamp(-1.45, 1.45);

        // Modern console aim-assist / auto-lock snapping when aiming (right click / L2)
        if actions.aim && player.health > 0.0 {
            let player_pos = tf.translation;
            let fwd = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0) * -Vec3::Z;
            let mut best_angle = 0.35; // ~20 degrees threshold
            let mut best_target_dir = None;

            for (enemy, etf) in &enemies {
                if !enemy.alive || enemy.ally {
                    continue;
                }
                let to_enemy = etf.translation - player_pos;
                let dist = to_enemy.length();
                if dist < 10.0 || dist > 3_000.0 {
                    continue;
                }
                let dir = to_enemy / dist;
                let dot = fwd.dot(dir);
                if dot > 0.75 { // in front
                    let angle = 1.0 - dot;
                    if angle < best_angle {
                        // check line of sight
                        let wall_dist = collide
                            .as_ref()
                            .and_then(|walls| walls.0.raycast(player_pos, dir, dist))
                            .unwrap_or(dist);
                        if wall_dist >= dist - 20.0 {
                            best_angle = angle;
                            best_target_dir = Some(dir);
                        }
                    }
                }
            }

            if let Some(target_dir) = best_target_dir {
                let target_yaw = (-target_dir.x).atan2(-target_dir.z);
                let target_pitch = target_dir.y.asin().clamp(-1.45, 1.45);
                let snap_rate = dt * 12.0; // snappy console snap
                // Smoothly slerp yaw/pitch toward target
                let mut dyaw = target_yaw - player.yaw;
                while dyaw > PI { dyaw -= 2.0 * PI; }
                while dyaw < -PI { dyaw += 2.0 * PI; }
                player.yaw += dyaw * snap_rate.min(1.0);
                player.pitch += (target_pitch - player.pitch) * snap_rate.min(1.0);
            }
        }

        tf.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);

        let fwd = tf.forward();
        let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
        // flat × up is the camera's right in a Y-up right-handed frame.
        let right = flat.cross(Vec3::Y);
        let wish = right * actions.wish.x + flat * actions.wish.y;
        let mag = wish.length().min(1.0);
        let mut dir = wish.normalize_or_zero();
        let ads_slow = 1.0 - 0.4 * player.ads;
        let speed = if actions.sprint { tune.sprint } else { tune.walk } * mag * ads_slow;

        if player.scale <= 0.001 {
            player.vel.x = dir.x * speed;
            player.vel.z = dir.z * speed;
        } else if mag < 0.05 {
            let stopped = approach_v(Vec3::new(player.vel.x, 0.0, player.vel.z), Vec3::ZERO, tune.brake * dt);
            player.vel.x = stopped.x;
            player.vel.z = stopped.z;
        } else {
            let current = Vec3::new(player.vel.x, 0.0, player.vel.z);
            if current.length() > 80.0 {
                dir = rotate_toward(current.normalize_or_zero(), dir, tune.yaw_rate * dt);
            }
            let steered = approach_v(current, dir * speed, tune.accel * dt);
            player.vel.x = steered.x;
            player.vel.z = steered.z;
        }

        if player.grounded && actions.jump {
            player.vel.y = tune.jump;
            player.grounded = false;
        }
        if player.grounded {
            if player.vel.y < 0.0 {
                player.vel.y = 0.0;
            }
        } else {
            player.vel.y -= tune.gravity * dt;
        }
        let next = tf.translation + player.vel * dt;
        let (mut center, mut grounded) = if let Some(grid) = physics.as_ref() {
            grid.0.depenetrate(next, player.radius)
        } else {
            collide
                .as_ref()
                .map(|walls| walls.0.depenetrate(next, player.radius))
                .unwrap_or((next, false))
        };
        for (door, dtf) in &doors {
            if door.hp <= 0.0 {
                continue;
            }
            center = script::push_sphere(center, player.radius, dtf.translation, door.half);
        }
        for (door, dtf) in &slides {
            if door.along > 0.85 && door.phase != script::DoorPhase::Closing {
                continue;
            }
            center = script::push_sphere(center, player.radius, dtf.translation, door.half);
        }
        if grounded && player.vel.y < 0.0 {
            player.vel.y = 0.0;
        }
        // Catch a landing that stepped through a thin floor. Skip this while
        // moving upward, or a jump is pulled straight back onto the ground.
        if !grounded && player.vel.y <= 0.0 && center.is_finite() {
            if let Some(fy) =
                player_floor(physics.as_deref(), collide.as_deref(), center.x, center.z, center.y + 4.0)
            {
                let gap = center.y - player.radius - fy;
                if (-0.5..24.0).contains(&gap) {
                    center.y = fy + player.radius;
                    player.vel.y = 0.0;
                    grounded = true;
                }
            }
        }
        // Shrinking back to on-foot leaves the camera floating. Pull it down
        // onto the floor while the radius is still catching up.
        if grounded && center.is_finite() {
            if let Some(fy) =
                player_floor(physics.as_deref(), collide.as_deref(), center.x, center.z, center.y + 8.0)
            {
                let gap = center.y - player.radius - fy;
                if (0.5..player.radius).contains(&gap) {
                    center.y = fy + player.radius;
                }
            }
        }
        if center.is_finite() {
            tf.translation = center;
        }
        player.grounded = grounded;
    }
}

/// Player ground comes from the PBlock walk when the world has one.
fn player_floor(
    physics: Option<&WorldPhysics>,
    collide: Option<&WorldCollide>,
    x: f32,
    z: f32,
    max_y: f32,
) -> Option<f32> {
    if let Some(grid) = physics {
        grid.0.floor_y(x, z, max_y)
    } else {
        collide.and_then(|walls| walls.0.floor_y(x, z, max_y))
    }
}

fn approach_f(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = target - current;
    if delta.abs() <= max_step {
        target
    } else {
        current + delta.signum() * max_step
    }
}

fn approach_v(current: Vec3, target: Vec3, max_step: f32) -> Vec3 {
    let delta = target - current;
    let len = delta.length();
    if len <= max_step || len < 1e-4 {
        target
    } else {
        current + delta / len * max_step
    }
}

fn rotate_toward(from: Vec3, to: Vec3, max_ang: f32) -> Vec3 {
    let ang_from = from.z.atan2(from.x);
    let ang_to = to.z.atan2(to.x);
    let mut d = ang_to - ang_from;
    let pi = std::f32::consts::PI;
    while d > pi {
        d -= pi * 2.0;
    }
    while d < -pi {
        d += pi * 2.0;
    }
    let ang = ang_from + d.clamp(-max_ang, max_ang);
    Vec3::new(ang.cos(), 0.0, ang.sin())
}

fn orbit_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut scroll: MessageReader<MouseWheel>,
    mut q: Query<(&mut Orbit, &mut Transform)>,
) {
    for (mut orbit, mut tf) in q.iter_mut() {
        if buttons.pressed(MouseButton::Right) {
            let mut delta = Vec2::ZERO;
            for ev in motion.read() {
                delta += ev.delta;
            }
            orbit.yaw -= delta.x * 0.005;
            orbit.pitch = (orbit.pitch - delta.y * 0.005).clamp(-1.5, 1.5);
        } else {
            motion.clear();
        }
        for ev in scroll.read() {
            let step = orbit.dist * 0.08;
            orbit.dist = (orbit.dist - ev.y * step).clamp(1.0, 200_000.0);
        }
        let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch + PI * 0.0, 0.0);
        let pos = orbit.target + rot * Vec3::new(0.0, 0.0, orbit.dist);
        *tf = Transform::from_translation(pos).looking_at(orbit.target, Vec3::Y);
    }
}

/// LithTech `Rotation` is a vector. Empty retail starts stay at the origin facing.
/// A component past one full turn is degrees; anything smaller is already radians.
fn facing(rot: [f32; 3]) -> (f32, f32) {
    let scale = if rot.iter().any(|c| c.abs() > std::f32::consts::TAU) {
        std::f32::consts::PI / 180.0
    } else {
        1.0
    };
    (rot[1] * scale, rot[0] * scale)
}

pub(crate) fn dat_stem(name: &str) -> &str {
    let name = name.trim();
    if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".dat") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

fn ambush_dat() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT")
}

/// Continue mission name, matched to a `.DAT` in the `WORLDS` directory.
fn saved_world(mission: &str) -> Option<std::path::PathBuf> {
    resolve_next_world(&ambush_dat(), mission)
}

/// New Game keeps the usual boot. Continue loads the saved mission, or Ambush
/// when that name is not a world on disk.
fn boot_path(launch: &front::Launch) -> Option<String> {
    if let front::Launch::Continue(save) = launch {
        if let Some(path) = saved_world(&save.mission) {
            return Some(path.to_string_lossy().into_owned());
        }
        let ambush = ambush_dat();
        if ambush.is_file() {
            return Some(ambush.to_string_lossy().into_owned());
        }
    }
    world_path()
}

/// `02_Quarters` matches `02_QUARTERS.DAT` in the same `WORLDS` directory.
pub(crate) fn resolve_next_world(current: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let dir = current.parent()?;
    let want = dat_stem(name);
    if want.is_empty() {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.eq_ignore_ascii_case(want) {
            return Some(path);
        }
    }
    None
}

fn queue_transition(event: On<script::LevelTransitionEvent>, mut commands: Commands) {
    let Some(current) = world_path() else {
        return;
    };
    let Some(path) = resolve_next_world(std::path::Path::new(&current), &event.next_world) else {
        warn!("no world file for {}", event.next_world);
        return;
    };
    info!("exit to {} at {}", path.display(), event.spawn_point);
    commands.insert_resource(PendingTransition {
        path: path.to_string_lossy().into_owned(),
        spawn: event.spawn_point.clone(),
    });
}

fn reload_campaign(
    campaign: Option<Res<CampaignFile>>,
    spawn: Option<Res<SpawnRequest>>,
    mut commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    images: ResMut<Assets<Image>>,
    players: Query<(&mut Player, &mut Transform)>,
) {
    let Some(path) = campaign.map(|file| file.0.clone()).or_else(world_path) else {
        return;
    };
    let name = spawn.map(|request| request.0.clone());
    commands.remove_resource::<SpawnRequest>();
    load_world(path, name, commands, meshes, materials, images, players);
}

fn perform_transition(world: &mut World) {
    let Some(pending) = world.remove_resource::<PendingTransition>() else {
        return;
    };
    let ids = {
        let mut query = world.query_filtered::<Entity, With<LevelEntity>>();
        query.iter(world).collect::<Vec<_>>()
    };
    for id in ids {
        world.despawn(id);
    }
    world.remove_resource::<script::Script>();
    world.remove_resource::<script::ObjectiveState>();
    world.remove_resource::<waypoint::Guide>();
    world.remove_resource::<combat::Waves>();
    world.remove_resource::<combat::Roster>();
    world.remove_resource::<combat::Mission>();
    world.remove_resource::<nav::WalkNav>();
    world.remove_resource::<WorldCollide>();
    world.remove_resource::<WorldPhysics>();
    world.remove_resource::<Checkpoint>();
    world.insert_resource(CampaignFile(pending.path.clone()));
    world.insert_resource(SpawnRequest(pending.spawn.clone()));
    remember_world(&pending.path);
    info!("loading {} at {}", pending.path, pending.spawn);
    if let Err(err) = world.run_system_once(reload_campaign) {
        error!("level reload failed: {err}");
        return;
    }
    if let Err(err) = world.run_system_once(arena::assemble) {
        error!("arena rebuild failed: {err}");
    }
    if let Err(err) = world.run_system_once(combat::spawn_cast) {
        error!("cast reload failed: {err}");
    }
    if let Err(err) = world.run_system_once(script::place) {
        error!("script reload failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        carried_rounds, rotate_toward, sky_camera_parts, BodyTune, BODY, GRAVITY, JUMP, MAG_SIZE,
        RESERVE_SIZE, RUN, WALK,
    };
    use bevy::camera::ClearColorConfig;
    use bevy::core_pipeline::tonemapping::Tonemapping;
    use bevy::prelude::Vec3;

    #[test]
    fn dry_continue_gets_the_starting_pack_and_a_partial_pack_is_kept() {
        assert_eq!(carried_rounds([0, 0], [0, 0]), (MAG_SIZE, RESERVE_SIZE));
        assert_eq!(carried_rounds([4, 0], [0, 0]), ([4, 0], [0, 0]));
        assert_eq!(carried_rounds([0, 0], [12, 0]), ([0, 0], [12, 0]));
    }

    #[test]
    fn sky_pass_clears_the_hdr_target_the_bloom_camera_loads() {
        let (camera, _, tonemap) = sky_camera_parts(1.0);
        assert!(matches!(camera.clear_color, ClearColorConfig::Custom(_)));
        assert!(matches!(
            camera.output_mode,
            bevy::camera::CameraOutputMode::Skip
        ));
        assert_eq!(tonemap, Tonemapping::None);
    }

    #[test]
    fn quarters_exit_resolves_to_the_retail_dat() {
        let current = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        let next = super::resolve_next_world(&current, "02_Quarters").expect("quarters dat");
        let file = next.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert!(file.eq_ignore_ascii_case("02_QUARTERS.DAT"));
        let data = std::fs::read(&next).unwrap();
        let starts = shogo_assets::dat::start_points(&data).unwrap();
        let start = starts
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case("start1"))
            .expect("start1");
        assert!((start.pos[0] + 80.0).abs() < 0.1);
        assert!((start.pos[2] - 1088.0).abs() < 0.1);
    }

    #[test]
    fn continue_selects_the_saved_dat_and_new_game_stays_on_ambush() {
        let path = std::env::temp_dir().join(format!(
            "shogo-continue-world-{}.sav",
            std::process::id()
        ));
        let written = super::front::Save {
            mission: "02_Quarters".into(),
            x: 12.0,
            y: 34.0,
            z: 56.0,
            yaw: 0.2,
            pitch: -0.1,
            health: 250.0,
            scale: 0.0,
            mca: false,
            weapon: 1,
            ammo: [7, 2],
            reserve: [20, 4],
        };
        for mission in ["02_Quarters", "02_QUARTERS.DAT"] {
            let mut save = written.clone();
            save.mission = mission.into();
            std::fs::write(&path, super::front::encode(&save)).expect("write save");
            let text = std::fs::read_to_string(&path).expect("read save");
            let parsed = super::front::decode(&text).expect("decode save");
            let continued =
                super::boot_path(&super::front::Launch::Continue(parsed)).expect("continue dat");
            let continued_name = std::path::Path::new(&continued)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            assert!(
                continued_name.eq_ignore_ascii_case("02_QUARTERS.DAT"),
                "{mission} -> {continued_name}"
            );
        }
        let _ = std::fs::remove_file(&path);

        super::forget_world();
        let fresh = super::boot_path(&super::front::Launch::Fresh).expect("new game");
        let fresh_name = std::path::Path::new(&fresh)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        assert!(
            fresh_name.eq_ignore_ascii_case("01_AMBUSH.DAT"),
            "{fresh_name}"
        );

        let mut missing = written;
        missing.mission = "no_such_world".into();
        let fallback =
            super::boot_path(&super::front::Launch::Continue(missing)).expect("fallback");
        let fallback_name = std::path::Path::new(&fallback)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        assert!(
            fallback_name.eq_ignore_ascii_case("01_AMBUSH.DAT"),
            "{fallback_name}"
        );
    }

    #[test]
    fn on_foot_matches_the_old_walk_and_mca_is_heavier() {
        let foot = BodyTune::at(0.0);
        let mca = BodyTune::at(1.0);
        assert!((foot.radius - BODY).abs() < 1e-3);
        assert!((foot.walk - WALK).abs() < 1e-3);
        assert!((foot.sprint - RUN).abs() < 1e-3);
        assert!((foot.jump - JUMP).abs() < 1e-3);
        assert!((foot.gravity - GRAVITY).abs() < 1e-3);
        assert!(mca.radius > foot.radius * 2.0);
        assert!(mca.sprint > foot.sprint);
        assert!(mca.accel < foot.accel * 0.1);
        assert!(mca.yaw_rate < 1.2);
        assert!(mca.jump > foot.jump);
        assert!(mca.fov > foot.fov);
        let mid = BodyTune::at(0.5);
        assert!(mid.radius > foot.radius && mid.radius < mca.radius);
    }

    #[test]
    fn mca_turn_is_capped() {
        let from = Vec3::new(0.0, 0.0, -1.0);
        let to = Vec3::new(1.0, 0.0, 0.0);
        let turned = rotate_toward(from, to, 0.2);
        let ang = turned.z.atan2(turned.x) - from.z.atan2(from.x);
        assert!(ang.abs() < 0.25, "{ang}");
        assert!(turned.length() > 0.9);
    }
}
