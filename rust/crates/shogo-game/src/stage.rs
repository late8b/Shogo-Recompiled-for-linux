//! Flat arena for the player loop. One hostile is a retail ABC plus DTX when
//! those files are on disk. The rest are volumes so lock-on still has a row
//! of targets if the archive is absent.

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::player::{self, Fresh, Player};
use crate::targeting::Hostile;

pub fn spawn_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 14_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3_200.0, 4.0, 3_200.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.17, 0.15),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -2.0, 0.0),
    ));
    for (x, z, w) in [(180.0, -220.0, 70.0), (-140.0, -300.0, 48.0)] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(w, 40.0, w))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.28, 0.27, 0.24),
                ..default()
            })),
            Transform::from_xyz(x, 20.0, z),
        ));
    }

    player::spawn_player(&mut commands);

    let spots = [
        Vec3::new(0.0, 0.0, -420.0),
        Vec3::new(260.0, 0.0, -560.0),
        Vec3::new(-280.0, 0.0, -640.0),
        Vec3::new(70.0, 0.0, -900.0),
    ];
    let mesh_ok = spawn_trooper(&mut commands, &mut meshes, &mut materials, &mut images, spots[0]);
    let start = if mesh_ok { 1 } else { 0 };
    for spot in spots.iter().skip(start) {
        spawn_capsule(&mut commands, &mut meshes, &mut materials, *spot);
    }
}

fn spawn_capsule(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    feet: Vec3,
) {
    let home = Vec3::new(feet.x, 36.0, feet.z);
    commands.spawn((
        Hostile {
            health: 80.0,
            max_health: 80.0,
            radius: 22.0,
            chest: 10.0,
            shot: 0.8,
            home,
        },
        Visibility::Visible,
        Mesh3d(meshes.add(Cuboid::new(36.0, 72.0, 36.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.18, 0.16),
            ..default()
        })),
        Transform::from_translation(home),
    ));
}

fn spawn_trooper(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    feet: Vec3,
) -> bool {
    let root = shogo_root();
    let Some(model_path) = resolve(&root, "MODELS/ENEMIES/ONFOOT/SHOCKTROOPER.ABC") else {
        info!("shocktrooper ABC not found — lock-on targets are volumes");
        return false;
    };
    let bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(err) => {
            warn!("read {}: {err}", model_path.display());
            return false;
        }
    };
    let model = match shogo_assets::abc::decode(&bytes, "shocktrooper") {
        Ok(m) => m,
        Err(err) => {
            warn!("decode {}: {err}", model_path.display());
            return false;
        }
    };
    let mesh = abc_mesh(&model);
    let texture = resolve(&root, "SKINS/ENEMIES/SHOCKTROOPER_CRONIAN.DTX").and_then(|path| {
        let bytes = std::fs::read(path).ok()?;
        let tex = shogo_assets::dtx::decode(&bytes).ok()?;
        Some(images.add(rgba_image(tex.width, tex.height, tex.rgba)))
    });
    let (min_y, height) = mesh_height(&model);
    let scale = 78.0 / height;
    let home = Vec3::new(feet.x, -min_y * scale, feet.z);
    commands.spawn((
        Hostile {
            health: 80.0,
            max_health: 80.0,
            radius: 22.0,
            chest: 48.0,
            shot: 0.6,
            home,
        },
        Visibility::Visible,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: texture,
            perceptual_roughness: 0.85,
            double_sided: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(home).with_scale(Vec3::splat(scale)),
    ));
    info!("streamed SHOCKTROOPER.ABC through shogo-assets");
    true
}

pub fn reset_if_fresh(
    mut fresh: ResMut<Fresh>,
    mut players: Query<(&mut Player, &mut Transform)>,
    mut hostiles: Query<(&mut Hostile, &mut Transform, &mut Visibility)>,
) {
    if !fresh.0 {
        return;
    }
    fresh.0 = false;
    if let Ok((mut player, mut tf)) = players.single_mut() {
        *player = Player::new();
        tf.translation = Vec3::new(0.0, player.radius, 0.0);
        tf.rotation = Quat::IDENTITY;
    }
    for (mut hostile, mut tf, mut vis) in &mut hostiles {
        hostile.health = hostile.max_health;
        hostile.shot = 0.4;
        tf.translation = hostile.home;
        *vis = Visibility::Visible;
    }
}

fn shogo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../extracted/SHOGO")
}

fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = root.to_path_buf();
    for part in rel.split('/') {
        let hit = std::fs::read_dir(&cur).ok()?.find_map(|entry| {
            let entry = entry.ok()?;
            entry
                .file_name()
                .to_str()
                .filter(|name| name.eq_ignore_ascii_case(part))
                .map(|_| entry.path())
        })?;
        cur = hit;
    }
    cur.is_file().then_some(cur)
}

fn abc_mesh(model: &shogo_assets::AbcModel) -> Mesh {
    let positions: Vec<[f32; 3]> = model
        .positions
        .iter()
        .map(|p| {
            let v = lt_to_yup(*p);
            [v.x, v.y, v.z]
        })
        .collect();
    let uvs: Vec<[f32; 2]> = model.uvs.iter().map(|[u, v]| [*u, 1.0 - v]).collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(model.indices.clone()));
    mesh.compute_area_weighted_normals();
    mesh
}

fn mesh_height(model: &shogo_assets::AbcModel) -> (f32, f32) {
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for p in &model.positions {
        let y = lt_to_yup(*p).y;
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if !min_y.is_finite() {
        return (0.0, 1.0);
    }
    (min_y, (max_y - min_y).max(1.0))
}

/// LithTech ABC corners are Z-up. `(x, y, z) -> (x, z, -y)` stands the mesh up.
fn lt_to_yup(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

fn rgba_image(width: u32, height: u32, rgba: Vec<u8>) -> Image {
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
