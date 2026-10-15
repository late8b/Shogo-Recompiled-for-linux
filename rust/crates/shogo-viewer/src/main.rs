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
//! Esc pauses. F5 or the pause menu writes a continue file. SHOGO_SKIP_MENU=1
//! boots straight into the mission.

mod arena;
mod collide;
mod combat;
mod front;
mod input;
mod nav;
mod pad;
mod tune;

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::camera::visibility::RenderLayers;
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
        .add_systems(OnEnter(front::Screen::Mission), (
            front::dismiss,
            spawn_stage,
            load_asset,
            front::apply_launch,
            arena::assemble,
            combat::spawn_fight,
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
                combat::waves,
                combat::enemies,
                combat::muzzle_flashes,
                combat::hud,
            )
                .chain()
                .run_if(front::sim_runs),
        );
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

#[derive(Resource)]
pub(crate) struct WorldCollide(pub(crate) collide::Collider);

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
pub(crate) const RESERVE_SIZE: [u16; 2] = [90, 24];

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
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.6, 0.0)),
    ));
    // SHOGO_GRAB=0 leaves the mouse free so a still frame shows the spawn view.
    let grab = std::env::var("SHOGO_GRAB").ok().as_deref() != Some("0");
    commands.insert_resource(LookGrabbed(grab));
    commands.spawn((
        Camera3d::default(),
        Camera {
            // Drawn after the sky camera, without wiping its color.
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

pub(crate) fn world_path() -> Option<String> {
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
    commands: Commands,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    images: ResMut<Assets<Image>>,
    players: Query<(&mut Player, &mut Transform)>,
    orbit_q: Query<&mut Orbit>,
) {
    if let Some(path) = world_path() {
        load_world(path, commands, meshes, materials, images, players);
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
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut players: Query<(&mut Player, &mut Transform)>,
) {
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
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        };
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(material)),
            Transform::default(),
        ));
    }
    info!("bound {textured}/{} textures", world.groups.len());

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

    // World positions are Y-up. start1 is 64 units above Road1.
    let (spawn, face) = shogo_assets::dat::start_points(&data)
        .ok()
        .and_then(|s| s.into_iter().next())
        .map(|s| {
            let feet = world_pos(s.pos);
            let y = collider
                .floor_y(feet.x, feet.z, feet.y + 128.0)
                .map(|fy| fy + BODY)
                .unwrap_or(feet.y);
            (Vec3::new(feet.x, y, feet.z), 0.0)
        })
        .unwrap_or_else(|| {
            let min = world_pos(world.min);
            let max = world_pos(world.max);
            ((min + max) * 0.5 + Vec3::Y * 80.0, 0.0)
        });
    commands.insert_resource(WorldCollide(collider));
    commands.insert_resource(Checkpoint {
        pos: spawn,
        yaw: face,
    });
    for (mut player, mut tf) in &mut players {
        player.yaw = face;
        player.grounded = true;
        tf.translation = spawn;
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

/// Follows the camera's position and stays world-aligned, so the clouds don't
/// stick to the view.
#[derive(Component)]
struct SkyRoot;

/// Renders only the sky, before the world camera.
#[derive(Component)]
struct SkyCam;

fn is_sky_portal(name: &str) -> bool {
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
        SkyCam,
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.45, 0.62, 0.78)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            near: 0.5,
            far: 100_000.0,
            fov: BodyTune::at(0.0).fov,
            ..default()
        }),
        Transform::from_translation(at),
        RenderLayers::layer(1),
    ));
    commands.spawn((
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
    mut players: Query<(&mut Player, &mut Transform, &mut Projection)>,
) {
    let Some(collide) = collide else {
        return;
    };
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
        let (mut center, mut grounded) = collide.0.depenetrate(next, player.radius);
        if grounded && player.vel.y < 0.0 {
            player.vel.y = 0.0;
        }
        // Catch a landing that stepped through a thin floor. Skip this while
        // moving upward, or a jump is pulled straight back onto the ground.
        if !grounded && player.vel.y <= 0.0 && center.is_finite() {
            if let Some(fy) = collide.0.floor_y(center.x, center.z, center.y + 4.0) {
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
            if let Some(fy) = collide.0.floor_y(center.x, center.z, center.y + 8.0) {
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

#[cfg(test)]
mod tests {
    use super::{rotate_toward, BodyTune, BODY, GRAVITY, JUMP, RUN, WALK};
    use bevy::prelude::Vec3;

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
