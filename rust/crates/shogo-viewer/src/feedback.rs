//! Hit flash, a short flinch, and burst debris for a landed shot.
//!
//! `combat::weapons` and `combat::bolts` already apply damage. They trigger
//! [`EntityHitEvent`] on that same path. This module does not track health.
//!
//! `combat::enemies` writes `Quat::from_rotation_y(enemy.yaw)` every tick for a
//! living body, including idle holds that skip a fresh write. A flinch stored
//! in the transform would be wiped or would stack. [`flinch`] runs after that
//! system and poses `yaw * pitch`, with the pitch kept on [`HitFlinch`]. When
//! the spring settles, the rotation is yaw again. ABC skinning is not involved:
//! the posed figure is a static mesh under the body entity.

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;

use crate::combat::Enemy;
use crate::LevelEntity;

const FLASH_TIME: f32 = 0.15;
/// Hot white-red. Enemy skins are unlit, so this tint is what the camera sees.
const FLASH_TINT: Color = Color::linear_rgb(3.5, 0.7, 0.55);
const FLASH_GLOW: LinearRgba = LinearRgba::new(7.0, 0.9, 0.6, 1.0);

const FLINCH_KICK: f32 = 2.6;
const FLINCH_MAX: f32 = 0.14;
const SPRING_K: f32 = 160.0;
const SPRING_D: f32 = 12.0;

const FOOT_BITS: usize = 8;
const MECH_BITS: usize = 6;

/// A weapon ray or bolt entered a living hostile's hit sphere.
#[derive(Event, Clone, Copy)]
pub(crate) struct EntityHitEvent {
    pub(crate) target: Entity,
    pub(crate) impact_point: Vec3,
    pub(crate) normal: Vec3,
    pub(crate) is_mech: bool,
}

/// Unique material swap for one mesh. The shared roster handle stays put.
#[derive(Component)]
#[component(on_remove = release_flash)]
pub(crate) struct HitFlash {
    timer: f32,
    original: Handle<StandardMaterial>,
    owned: Handle<StandardMaterial>,
    base: Color,
    emissive: LinearRgba,
}

/// Pitch offset on top of `Enemy::yaw`. Zero offset is the baseline facing.
#[derive(Component)]
pub(crate) struct HitFlinch {
    /// Body-local axis. Positive offset leans the head along the shot.
    axis: Vec3,
    offset: f32,
    vel: f32,
}

/// Marker so motion only touches burst pieces.
#[derive(Component)]
pub(crate) struct Debris;

#[derive(Component)]
pub(crate) struct Velocity(Vec3);

#[derive(Component)]
pub(crate) struct Gravity(f32);

#[derive(Component)]
pub(crate) struct Lifetime(f32);

#[derive(Component)]
pub(crate) struct Spin(Vec3);

#[derive(Resource)]
pub(crate) struct DebrisBank {
    quad: Handle<Mesh>,
    tetra: Handle<Mesh>,
    sliver: Handle<Mesh>,
    blood: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    iron: Handle<StandardMaterial>,
    hot: Handle<StandardMaterial>,
}

impl FromWorld for DebrisBank {
    fn from_world(world: &mut World) -> Self {
        let quad = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Rectangle::new(1.0, 1.0)));
        let tetra = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Tetrahedron::default()));
        let sliver = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Cuboid::new(1.0, 0.2, 0.45)));
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let blood = materials.add(unlit(Color::linear_rgb(1.8, 0.04, 0.03)));
        let spark = materials.add(unlit(Color::linear_rgb(4.0, 2.4, 0.35)));
        let steel = materials.add(unlit(Color::srgb(0.78, 0.8, 0.84)));
        let iron = materials.add(unlit(Color::srgb(0.32, 0.34, 0.36)));
        let hot = materials.add(unlit(Color::linear_rgb(3.2, 1.1, 0.25)));
        Self {
            quad,
            tetra,
            sliver,
            blood,
            spark,
            steel,
            iron,
            hot,
        }
    }
}

fn unlit(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

fn release_flash(mut world: DeferredWorld, ctx: HookContext) {
    let Some(owned) = world
        .get_mut::<HitFlash>(ctx.entity)
        .map(|flash| flash.owned.clone())
    else {
        return;
    };
    let Some(mut materials) = world.get_resource_mut::<Assets<StandardMaterial>>() else {
        return;
    };
    materials.remove(owned.id());
}

/// Observer for [`EntityHitEvent`]. Inserts the flash and flinch, then spawns debris.
pub(crate) fn on_hit(
    hit: On<EntityHitEvent>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bank: Res<DebrisBank>,
    children: Query<&Children>,
    mut visuals: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut flashes: Query<&mut HitFlash>,
    mut flinches: Query<&mut HitFlinch>,
    enemies: Query<&Enemy>,
    mut rng: Local<u32>,
) {
    let Ok(enemy) = enemies.get(hit.target) else {
        return;
    };
    if *rng == 0 {
        *rng = 0xA5A5_5A5A;
    }
    if enemy.alive {
        let axis = lean_axis(hit.normal, enemy.yaw);
        if let Ok(mut flinch) = flinches.get_mut(hit.target) {
            absorb(&mut flinch, axis);
        } else {
            commands.entity(hit.target).insert(HitFlinch {
                axis,
                offset: 0.0,
                vel: FLINCH_KICK,
            });
        }
    }
    if let Ok(kids) = children.get(hit.target) {
        for child in kids.iter() {
            tint_mesh(
                &mut commands,
                &mut materials,
                &mut visuals,
                &mut flashes,
                child,
            );
        }
    }
    burst(
        &mut commands,
        &bank,
        hit.impact_point,
        hit.normal,
        hit.is_mech,
        &mut rng,
    );
}

fn tint_mesh(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    visuals: &mut Query<&mut MeshMaterial3d<StandardMaterial>>,
    flashes: &mut Query<&mut HitFlash>,
    mesh: Entity,
) {
    if let Ok(mut flash) = flashes.get_mut(mesh) {
        flash.timer = FLASH_TIME;
        if let Some(mut asset) = materials.get_mut(&flash.owned) {
            asset.base_color = FLASH_TINT;
            asset.emissive = FLASH_GLOW;
        }
        return;
    }
    let Ok(mut slot) = visuals.get_mut(mesh) else {
        return;
    };
    let Some(flash) = open_flash(materials, &slot.0) else {
        return;
    };
    slot.0 = flash.owned.clone();
    commands.entity(mesh).insert(flash);
}

fn open_flash(
    materials: &mut Assets<StandardMaterial>,
    source: &Handle<StandardMaterial>,
) -> Option<HitFlash> {
    let src = materials.get(source)?.clone();
    let base = src.base_color;
    let emissive = src.emissive;
    let mut next = src;
    next.base_color = FLASH_TINT;
    next.emissive = FLASH_GLOW;
    Some(HitFlash {
        timer: FLASH_TIME,
        original: source.clone(),
        owned: materials.add(next),
        base,
        emissive,
    })
}

pub(crate) fn flash(
    time: Res<Time>,
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: Query<(Entity, &mut HitFlash, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mut slot) in &mut meshes {
        flash.timer = (flash.timer - dt).max(0.0);
        let u = (flash.timer / FLASH_TIME).clamp(0.0, 1.0);
        let k = u * u * (3.0 - 2.0 * u);
        if let Some(mut mat) = materials.get_mut(&flash.owned) {
            mat.base_color = flash.base.mix(&FLASH_TINT, k);
            mat.emissive = flash.emissive.mix(&FLASH_GLOW, k);
        }
        if flash.timer == 0.0 {
            slot.0 = flash.original.clone();
            commands.entity(entity).remove::<HitFlash>();
        }
    }
}

pub(crate) fn flinch(
    time: Res<Time>,
    mut commands: Commands,
    mut bodies: Query<(Entity, &Enemy, &mut Transform, &mut HitFlinch)>,
) {
    let dt = time.delta_secs();
    for (entity, enemy, mut tf, mut flinch) in &mut bodies {
        if !enemy.alive {
            commands.entity(entity).remove::<HitFlinch>();
            continue;
        }
        let (offset, vel) = spring_step(flinch.offset, flinch.vel, dt);
        flinch.offset = offset;
        flinch.vel = vel;
        if settled(offset, vel) {
            tf.rotation = Quat::from_rotation_y(enemy.yaw);
            commands.entity(entity).remove::<HitFlinch>();
            continue;
        }
        tf.rotation = posed(enemy.yaw, flinch.axis, offset);
    }
}

pub(crate) fn debris(
    time: Res<Time>,
    mut commands: Commands,
    mut parts: Query<(
        Entity,
        &mut Transform,
        &mut Velocity,
        &Gravity,
        &mut Lifetime,
        Option<&Spin>,
    ), With<Debris>>,
) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut tf, mut vel, gravity, mut life, spin) in &mut parts {
        life.0 -= dt;
        if life.0 <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let (pos, next) = step_motion(tf.translation, vel.0, gravity.0, dt);
        vel.0 = next;
        tf.translation = pos;
        if let Some(spin) = spin {
            tf.rotation *= Quat::from_scaled_axis(spin.0 * dt);
        }
    }
}

fn burst(
    commands: &mut Commands,
    bank: &DebrisBank,
    point: Vec3,
    normal: Vec3,
    is_mech: bool,
    rng: &mut u32,
) {
    let count = burst_count(is_mech);
    let (life_lo, life_hi) = life_span(is_mech);
    let origin = point + normal.normalize_or_zero() * 8.0;
    for i in 0..count {
        let dir = cone(normal, rng);
        let speed = if is_mech {
            180.0 + unit(rng) * 200.0
        } else {
            240.0 + unit(rng) * 240.0
        };
        let mut vel = dir * speed;
        vel.y += 160.0 + unit(rng) * 180.0;
        let life = life_lo + (life_hi - life_lo) * unit(rng);
        let scale = if is_mech {
            10.0 + unit(rng) * 12.0
        } else {
            5.0 + unit(rng) * 5.0
        };
        let (mesh, mat) = if is_mech {
            if i % 3 == 2 {
                (bank.sliver.clone(), bank.hot.clone())
            } else if i % 2 == 0 {
                (bank.tetra.clone(), bank.steel.clone())
            } else {
                (bank.tetra.clone(), bank.iron.clone())
            }
        } else if i % 2 == 0 {
            (bank.quad.clone(), bank.blood.clone())
        } else {
            (bank.quad.clone(), bank.spark.clone())
        };
        let spin = Vec3::new(
            (unit(rng) - 0.5) * 14.0,
            (unit(rng) - 0.5) * 14.0,
            (unit(rng) - 0.5) * 14.0,
        );
        commands.spawn((
            LevelEntity,
            Debris,
            Velocity(vel),
            Gravity(if is_mech { 1_100.0 } else { 900.0 }),
            Lifetime(life),
            Spin(spin),
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(origin)
                .with_scale(Vec3::splat(scale))
                .with_rotation(Quat::from_scaled_axis(spin)),
        ));
    }
}

fn burst_count(is_mech: bool) -> usize {
    if is_mech { MECH_BITS } else { FOOT_BITS }
}

fn life_span(is_mech: bool) -> (f32, f32) {
    if is_mech { (1.2, 1.95) } else { (1.05, 1.75) }
}

fn cone(normal: Vec3, rng: &mut u32) -> Vec3 {
    let n = normal.normalize_or(Vec3::Y);
    let tangent = n.cross(Vec3::Y).normalize_or(Vec3::X);
    let bitangent = n.cross(tangent);
    let spread = 0.7;
    (n + tangent * (unit(rng) * 2.0 - 1.0) * spread
        + bitangent * (unit(rng) * 2.0 - 1.0) * spread)
        .normalize_or(n)
}

fn unit(rng: &mut u32) -> f32 {
    *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
    ((*rng >> 8) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

fn absorb(flinch: &mut HitFlinch, axis: Vec3) {
    let axis = axis.normalize_or(Vec3::X);
    if flinch.axis.dot(axis) < 0.0 {
        flinch.offset = -flinch.offset;
        flinch.vel = -flinch.vel;
    }
    flinch.axis = axis;
    flinch.vel = (flinch.vel + FLINCH_KICK).clamp(-6.0, 6.0);
}

/// Outward sphere normal faces the shooter. The head moves the other way.
pub(crate) fn lean_axis(normal: Vec3, yaw: f32) -> Vec3 {
    let mut push = Vec3::new(-normal.x, 0.0, -normal.z);
    if push.length_squared() < 1e-6 {
        push = Quat::from_rotation_y(yaw) * Vec3::Z;
    }
    let local = Quat::from_rotation_y(yaw).inverse() * push.normalize_or(Vec3::NEG_Z);
    Vec3::Y.cross(local).normalize_or(Vec3::X)
}

pub(crate) fn posed(yaw: f32, axis: Vec3, offset: f32) -> Quat {
    Quat::from_rotation_y(yaw) * Quat::from_axis_angle(axis.normalize_or(Vec3::X), offset)
}

pub(crate) fn spring_step(offset: f32, vel: f32, dt: f32) -> (f32, f32) {
    let dt = dt.clamp(0.0, 0.05);
    let accel = -SPRING_K * offset - SPRING_D * vel;
    let mut vel = vel + accel * dt;
    let mut offset = offset + vel * dt;
    if offset > FLINCH_MAX {
        offset = FLINCH_MAX;
        if vel > 0.0 {
            vel = 0.0;
        }
    } else if offset < -FLINCH_MAX {
        offset = -FLINCH_MAX;
        if vel < 0.0 {
            vel = 0.0;
        }
    }
    (offset, vel)
}

fn settled(offset: f32, vel: f32) -> bool {
    offset.abs() < 0.004 && vel.abs() < 0.05
}

pub(crate) fn step_motion(pos: Vec3, vel: Vec3, gravity: f32, dt: f32) -> (Vec3, Vec3) {
    let mut vel = vel;
    vel.y -= gravity * dt;
    (pos + vel * dt, vel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AiState, EnemyKind};
    use std::time::Duration;

    fn bare_enemy(kind: EnemyKind, yaw: f32) -> Enemy {
        Enemy {
            kind,
            ally: false,
            health: 80.0,
            fire_cd: 0.0,
            alive: true,
            state: AiState::Idle,
            patrol: [Vec3::ZERO, Vec3::X],
            patrol_i: 0,
            idle: 0.0,
            lost: 0.0,
            last_seen: Vec3::ZERO,
            path: Vec::new(),
            path_cd: 0.0,
            yaw,
            dead_t: 0.0,
            fall_v: 0.0,
            label: String::new(),
            route: Vec::new(),
            route_i: 0,
            run: false,
            authored: false,
        }
    }

    #[test]
    fn mca_kinds_are_mechs_and_on_foot_are_not() {
        assert!(EnemyKind::Andra5.is_mech());
        assert!(EnemyKind::Andra10.is_mech());
        assert!(EnemyKind::Raksha.is_mech());
        assert!(EnemyKind::Uhlan.is_mech());
        assert!(!EnemyKind::Trooper.is_mech());
        assert!(!EnemyKind::Shock.is_mech());
        assert!(!EnemyKind::Ruin.is_mech());
        assert_eq!(burst_count(false), FOOT_BITS);
        assert_eq!(burst_count(true), MECH_BITS);
        for mech in [false, true] {
            let (lo, hi) = life_span(mech);
            assert!(lo >= 1.0 && hi <= 2.0 && lo < hi);
        }
    }

    #[test]
    fn zero_offset_is_the_yaw_baseline() {
        let yaw = 0.7;
        let axis = lean_axis(Vec3::new(0.0, 0.2, 1.0), yaw);
        let posed = posed(yaw, axis, 0.0);
        let base = Quat::from_rotation_y(yaw);
        assert!(posed.dot(base).abs() > 0.9999);
    }

    #[test]
    fn flinch_stays_small_and_returns_to_yaw() {
        let yaw = 0.7;
        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .add_systems(Update, flinch);
        let id = app
            .world_mut()
            .spawn((
                bare_enemy(EnemyKind::Trooper, yaw),
                Transform::from_rotation(Quat::from_rotation_y(yaw)),
                HitFlinch {
                    axis: Vec3::X,
                    offset: 0.0,
                    vel: FLINCH_KICK,
                },
            ))
            .id();

        let mut offset = 0.0;
        let mut vel = FLINCH_KICK;
        for _ in 0..240 {
            (offset, vel) = spring_step(offset, vel, 1.0 / 60.0);
            assert!(offset.abs() <= FLINCH_MAX + 1e-4);
        }
        assert!(settled(offset, vel));

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(16));
        app.update();
        let mid = app.world().get::<Transform>(id).unwrap().rotation;
        let base = Quat::from_rotation_y(yaw);
        assert!(mid.angle_between(base) > 0.01);
        assert!((app.world().get::<Enemy>(id).unwrap().yaw - yaw).abs() < 1e-6);

        for _ in 0..240 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(16));
            app.update();
        }
        assert!(app.world().get::<HitFlinch>(id).is_none());
        let end = app.world().get::<Transform>(id).unwrap().rotation;
        assert!(end.angle_between(base) < 1e-3);
        assert!((app.world().get::<Enemy>(id).unwrap().yaw - yaw).abs() < 1e-6);
    }

    #[test]
    fn debris_arcs_then_despawns() {
        let mut pos = Vec3::ZERO;
        let mut vel = Vec3::new(0.0, 200.0, 0.0);
        let mut peak: f32 = 0.0;
        for _ in 0..60 {
            (pos, vel) = step_motion(pos, vel, 900.0, 1.0 / 60.0);
            peak = peak.max(pos.y);
        }
        assert!(peak > 15.0);
        assert!(pos.y < peak);

        let mut app = App::new();
        app.insert_resource(Time::<()>::default())
            .add_systems(Update, debris);
        let id = app
            .world_mut()
            .spawn((
                Debris,
                Velocity(Vec3::new(0.0, 30.0, 0.0)),
                Gravity(40.0),
                Lifetime(0.05),
                Transform::IDENTITY,
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(16));
        app.update();
        let y = app.world().get::<Transform>(id).unwrap().translation.y;
        assert!(y > 0.2);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(80));
        app.update();
        assert!(app.world().get::<Transform>(id).is_none());
    }

    #[test]
    fn flash_clones_a_material_and_restores_the_shared_handle() {
        let mut app = App::new();
        app.insert_resource(Assets::<StandardMaterial>::default())
            .insert_resource(Time::<()>::default())
            .add_systems(Update, flash);
        let original_color = Color::srgb(0.2, 0.45, 0.7);
        let shared = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: original_color,
                emissive: LinearRgba::rgb(0.1, 0.0, 0.0),
                ..default()
            });
        let flash = {
            let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            open_flash(&mut materials, &shared).expect("shared material")
        };
        let owned = flash.owned.clone();
        assert_ne!(owned, shared);
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&shared)
                .unwrap()
                .base_color,
            original_color
        );

        let child = app
            .world_mut()
            .spawn((MeshMaterial3d(owned.clone()), flash))
            .id();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(50));
        app.update();
        let tinted = app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&owned)
            .unwrap()
            .base_color;
        assert_ne!(tinted, original_color);
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&shared)
                .unwrap()
                .base_color,
            original_color
        );

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(200));
        app.update();
        let slot = app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(child)
            .unwrap();
        assert_eq!(slot.0, shared);
        assert!(app.world().get::<HitFlash>(child).is_none());
        assert!(app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&owned)
            .is_none());
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&shared)
                .unwrap()
                .base_color,
            original_color
        );

        let doomed = {
            let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            open_flash(&mut materials, &shared).expect("shared material")
        };
        let doomed_owned = doomed.owned.clone();
        let body = app.world_mut().spawn(doomed).id();
        app.world_mut().entity_mut(body).despawn();
        assert!(app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(&doomed_owned)
            .is_none());
        assert_eq!(
            app.world()
                .resource::<Assets<StandardMaterial>>()
                .get(&shared)
                .unwrap()
                .base_color,
            original_color
        );
    }
}
