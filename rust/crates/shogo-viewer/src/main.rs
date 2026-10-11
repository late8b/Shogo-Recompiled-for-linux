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
//!
//! Controls: right-drag to orbit, scroll to zoom.

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Shogo MAD — native viewer".into(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, (setup, load_model).chain())
        .add_systems(Update, orbit_camera)
        .run();
}

/// Orbit-camera state attached to the camera entity.
#[derive(Component)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    dist: f32,
    target: Vec3,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Reference ground plane.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.15, 0.15, 0.18),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, -2.0, 0.0),
    ));

    // Directional light.
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.6, 0.0)),
    ));

    // Camera + orbit controller.
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 3.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        Orbit {
            yaw: 0.0,
            pitch: 0.3,
            dist: 8.0,
            target: Vec3::ZERO,
        },
    ));
}

/// Load the ABC model named by the SHOGO_MODEL env var (once) and spawn it.
fn load_model(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut orbit_q: Query<&mut Orbit>,
) {
    let path = match std::env::var("SHOGO_MODEL") {
        Ok(p) => p,
        Err(_) => {
            info!("SHOGO_MODEL not set — pass the path to an original .ABC model");
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

/// LithTech (Z-up) -> Bevy (Y-up) rotation about X.
fn lt_to_yup(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
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
            orbit.dist = (orbit.dist - ev.y * 0.5).clamp(1.0, 200.0);
        }
        let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch + PI * 0.0, 0.0);
        let pos = orbit.target + rot * Vec3::new(0.0, 0.0, orbit.dist);
        *tf = Transform::from_translation(pos).looking_at(orbit.target, Vec3::Y);
    }
}
