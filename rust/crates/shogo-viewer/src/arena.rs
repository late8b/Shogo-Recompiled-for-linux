//! Walls around the whole retail mesh, plus the nav bake that fills it.
//!
//! The yard used to be a square centered on the spawn, and that square cut
//! off the plaza east of the alley. The walls now sit just outside the
//! collision bounds, so the fight can use the full level.

use bevy::prelude::*;

use crate::nav::{NavMesh, WalkNav};
use crate::{Checkpoint, WorldCollide, BODY};

const THICK: f32 = 48.0;
const WALL_H: f32 = 640.0;
const NAV_CELL: f32 = 96.0;
const MARGIN: f32 = 48.0;

pub fn assemble(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    checkpoint: Option<Res<Checkpoint>>,
    mut collide: Option<ResMut<WorldCollide>>,
) {
    let (Some(checkpoint), Some(collide)) = (checkpoint, collide.as_mut()) else {
        return;
    };
    let floor = checkpoint.pos.y - BODY;
    let Some((min_b, max_b)) = mesh_bounds(&collide.0) else {
        return;
    };
    let mut extra = Vec::new();
    let y0 = floor - 40.0;
    let y1 = floor + WALL_H;
    let t = THICK;
    let min_x = min_b.x - MARGIN;
    let max_x = max_b.x + MARGIN;
    let min_z = min_b.z - MARGIN;
    let max_z = max_b.z + MARGIN;
    // North / south run along X. East / west run along Z and meet them at the corners.
    push_box(
        &mut extra,
        Vec3::new(min_x - t, y0, max_z),
        Vec3::new(max_x + t, y1, max_z + t),
    );
    push_box(
        &mut extra,
        Vec3::new(min_x - t, y0, min_z - t),
        Vec3::new(max_x + t, y1, min_z),
    );
    push_box(
        &mut extra,
        Vec3::new(max_x, y0, min_z),
        Vec3::new(max_x + t, y1, max_z),
    );
    push_box(
        &mut extra,
        Vec3::new(min_x - t, y0, min_z),
        Vec3::new(min_x, y1, max_z),
    );

    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.2, 0.26),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    for tri_box in extra.chunks(12) {
        let min = tri_box.iter().flatten().copied().reduce(|a, b| a.min(b)).unwrap_or(Vec3::ZERO);
        let max = tri_box.iter().flatten().copied().reduce(|a, b| a.max(b)).unwrap_or(Vec3::ONE);
        let size = max - min;
        let center = (min + max) * 0.5;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(center),
        ));
    }

    collide.0.append(extra);
    let center = Vec3::new(
        (min_b.x + max_b.x) * 0.5,
        floor,
        (min_b.z + max_b.z) * 0.5,
    );
    let half = (max_b.x - min_b.x).max(max_b.z - min_b.z) * 0.5 + MARGIN;
    let started = std::time::Instant::now();
    let nav = NavMesh::bake(&collide.0, center, half, NAV_CELL);
    info!(
        "arena {:.0} x {:.0}, nav {} walkable cells in {:.1}ms",
        max_x - min_x,
        max_z - min_z,
        nav.walkable_count(),
        started.elapsed().as_secs_f32() * 1000.0
    );
    commands.insert_resource(WalkNav(nav));
}

fn mesh_bounds(col: &crate::collide::Collider) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut n = 0u32;
    for tri in col.triangles() {
        for p in tri {
            if p.is_finite() {
                min = min.min(*p);
                max = max.max(*p);
                n += 1;
            }
        }
    }
    (n > 0).then_some((min, max))
}

fn push_box(out: &mut Vec<[Vec3; 3]>, min: Vec3, max: Vec3) {
    let p = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];
    const FACES: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [5, 4, 7, 6],
        [4, 0, 3, 7],
        [1, 5, 6, 2],
        [3, 2, 6, 7],
        [4, 5, 1, 0],
    ];
    for f in FACES {
        out.push([p[f[0]], p[f[1]], p[f[2]]]);
        out.push([p[f[0]], p[f[2]], p[f[3]]]);
    }
}
