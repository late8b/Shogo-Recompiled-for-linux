//! Two rifles from the playable runtime.
//!
//! The decompiled weapon classes (`CProjectile`, `CPulseRifleProjectile`) do
//! not leave a fire-rate immediate next to the constructor. These intervals,
//! magazine sizes, and damages are the ones `shogo-viewer` already plays:
//! assault every 0.14 s, pulse every 0.5 s.

use bevy::prelude::*;

use crate::input::Actions;
use crate::physics::BODY;
use crate::player::{Player, Sim};
use crate::targeting::{Hostile, LockOn};

pub const PLAYER_HP: f32 = 500.0;
pub const ARMOR_MAX: f32 = 150.0;

#[derive(Clone, Copy, Debug)]
pub struct WeaponKind {
    pub name: &'static str,
    pub cooldown: f32,
    pub reload: f32,
    pub mag: u16,
    pub reserve: u16,
    pub damage: f32,
    pub hitscan: bool,
    pub bolt_speed: f32,
}

pub const WEAPONS: [WeaponKind; 2] = [
    WeaponKind {
        name: "Assault Rifle",
        cooldown: 0.14,
        reload: 1.25,
        mag: 30,
        reserve: 90,
        damage: 15.0,
        hitscan: true,
        bolt_speed: 0.0,
    },
    WeaponKind {
        name: "Pulse Rifle",
        cooldown: 0.5,
        reload: 1.7,
        mag: 8,
        reserve: 24,
        damage: 40.0,
        hitscan: false,
        bolt_speed: 3_200.0,
    },
];

pub fn weapon(slot: u8) -> &'static WeaponKind {
    &WEAPONS[(slot as usize).min(WEAPONS.len() - 1)]
}

/// Armor takes the hit first. What is left reaches health.
pub fn apply_damage(health: &mut f32, armor: &mut f32, amount: f32) {
    let amount = amount.max(0.0);
    let soak = amount.min((*armor).max(0.0));
    *armor = (*armor - soak).max(0.0);
    *health = (*health - (amount - soak)).max(0.0);
}

pub fn can_fire(cooldown: f32, reloading: f32, ammo: u16) -> bool {
    cooldown <= 0.0 && reloading <= 0.0 && ammo > 0
}

pub fn finish_reload(slot: u8, ammo: &mut [u16; 2], reserve: &mut [u16; 2]) {
    let w = (slot as usize).min(1);
    let room = WEAPONS[w].mag.saturating_sub(ammo[w]);
    let take = room.min(reserve[w]);
    ammo[w] = ammo[w].saturating_add(take);
    reserve[w] = reserve[w].saturating_sub(take);
}

/// Closest sphere in front of a normalized ray.
pub fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let near = -b - s;
    if near >= 0.0 {
        return Some(near);
    }
    let far = -b + s;
    (far >= 0.0).then_some(far)
}

#[derive(Component)]
pub struct Bolt {
    pub vel: Vec3,
    pub damage: f32,
    pub life: f32,
    pub friendly: bool,
}

pub fn tick(
    time: Res<Time>,
    sim: Res<Sim>,
    actions: Res<Actions>,
    lock: Res<LockOn>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut players: Query<(&mut Player, &Transform)>,
    mut hostiles: Query<(Entity, &mut Hostile, &Transform, &mut Visibility), Without<Player>>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Transform), Without<Player>>,
) {
    let dt = time.delta_secs().min(0.05);
    let mut origin = Vec3::ZERO;
    let mut alive = false;
    if let Ok((mut player, tf)) = players.single_mut() {
        player.fire_cd = (player.fire_cd - dt).max(0.0);
        if player.reloading > 0.0 {
            player.reloading = (player.reloading - dt).max(0.0);
            if player.reloading <= 0.0 {
                let slot = player.weapon;
                let mut ammo = player.ammo;
                let mut reserve = player.reserve;
                finish_reload(slot, &mut ammo, &mut reserve);
                player.ammo = ammo;
                player.reserve = reserve;
            }
        }
        origin = tf.translation;
        alive = player.health > 0.0;
        if alive {
            let slot = player.weapon as usize;
            let kind = *weapon(player.weapon);
            let wheel_blocks = !sim.run;
            if !wheel_blocks
                && actions.reload
                && player.reloading <= 0.0
                && can_reload(player.weapon, player.ammo, player.reserve)
            {
                player.reloading = kind.reload;
            } else if !wheel_blocks && actions.fire && player.reloading <= 0.0 {
                try_fire(&mut player, &tf, slot, kind, lock.target, &actions, &mut commands, &mut meshes, &mut materials, &mut hostiles);
            }
        }
    }
    if alive {
        hostile_shots(dt, &origin, &mut commands, &mut meshes, &mut materials, &mut hostiles);
    }
    step_bolts(dt, &mut commands, &mut players, &mut hostiles, &mut bolts);
}

fn try_fire(
    player: &mut Player,
    tf: &Transform,
    slot: usize,
    kind: WeaponKind,
    locked: Option<Entity>,
    actions: &Actions,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    hostiles: &mut Query<(Entity, &mut Hostile, &Transform, &mut Visibility), Without<Player>>,
) {
    if player.ammo[slot] == 0 {
        if can_reload(player.weapon, player.ammo, player.reserve) {
            player.reloading = kind.reload;
        }
        return;
    }
    if player.fire_cd > 0.0 || !actions.fire {
        return;
    }
    let origin = tf.translation;
    let forward = tf.forward().as_vec3();
    let aim = shot_dir(origin, forward, locked, hostiles);
    if kind.hitscan {
        if let Some((hit, _)) = closest_hostile(origin, aim, hostiles) {
            if let Ok((_, mut hostile, _, mut vis)) = hostiles.get_mut(hit) {
                hostile.health = (hostile.health - kind.damage).max(0.0);
                if hostile.health <= 0.0 {
                    *vis = Visibility::Hidden;
                }
            }
        }
    } else {
        spawn_bolt(
            commands,
            meshes,
            materials,
            origin + aim * 8.0,
            aim * kind.bolt_speed,
            kind.damage,
            true,
        );
    }
    player.fire_cd = kind.cooldown;
    player.ammo[slot] = player.ammo[slot].saturating_sub(1);
}

fn shot_dir(
    origin: Vec3,
    forward: Vec3,
    locked: Option<Entity>,
    hostiles: &Query<(Entity, &mut Hostile, &Transform, &mut Visibility), Without<Player>>,
) -> Vec3 {
    if let Some(entity) = locked {
        if let Ok((_, hostile, tf, _)) = hostiles.get(entity) {
            if hostile.health > 0.0 {
                let com = tf.translation + Vec3::Y * hostile.chest;
                return (com - origin).normalize_or_zero();
            }
        }
    }
    forward.normalize_or_zero()
}

fn closest_hostile(
    origin: Vec3,
    dir: Vec3,
    hostiles: &Query<(Entity, &mut Hostile, &Transform, &mut Visibility), Without<Player>>,
) -> Option<(Entity, f32)> {
    let mut best: Option<(Entity, f32)> = None;
    for (entity, hostile, tf, _) in hostiles.iter() {
        if hostile.health <= 0.0 {
            continue;
        }
        let center = tf.translation + Vec3::Y * hostile.chest;
        let Some(dist) = ray_sphere(origin, dir, center, hostile.radius) else {
            continue;
        };
        if best.map(|(_, d)| dist < d).unwrap_or(true) {
            best = Some((entity, dist));
        }
    }
    best
}

fn hostile_shots(
    dt: f32,
    player_pos: &Vec3,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    hostiles: &mut Query<(Entity, &mut Hostile, &Transform, &mut Visibility), Without<Player>>,
) {
    for (_, mut hostile, tf, _) in hostiles.iter_mut() {
        hostile.shot = (hostile.shot - dt).max(0.0);
        if hostile.health <= 0.0 || hostile.shot > 0.0 {
            continue;
        }
        let com = tf.translation + Vec3::Y * hostile.chest;
        let to = *player_pos - com;
        let dist = to.length();
        if dist > 680.0 || dist < 40.0 {
            continue;
        }
        hostile.shot = 1.6;
        let dir = to.normalize_or_zero();
        spawn_bolt(commands, meshes, materials, com + dir * 10.0, dir * 420.0, 12.0, false);
    }
}

fn step_bolts(
    dt: f32,
    commands: &mut Commands,
    players: &mut Query<(&mut Player, &Transform)>,
    hostiles: &mut Query<(Entity, &mut Hostile, &Transform, &mut Visibility), Without<Player>>,
    bolts: &mut Query<(Entity, &mut Bolt, &mut Transform), Without<Player>>,
) {
    let mut dead = Vec::new();
    for (entity, mut bolt, mut tf) in bolts.iter_mut() {
        bolt.life -= dt;
        let next = tf.translation + bolt.vel * dt;
        if bolt.life <= 0.0 || next.y < 0.0 {
            dead.push(entity);
            continue;
        }
        if bolt.friendly {
            let mut hit = false;
            for (_, mut hostile, htf, mut vis) in hostiles.iter_mut() {
                if hostile.health <= 0.0 {
                    continue;
                }
                let center = htf.translation + Vec3::Y * hostile.chest;
                if next.distance(center) <= hostile.radius + 6.0 {
                    hostile.health = (hostile.health - bolt.damage).max(0.0);
                    if hostile.health <= 0.0 {
                        *vis = Visibility::Hidden;
                    }
                    hit = true;
                    break;
                }
            }
            if hit {
                dead.push(entity);
                continue;
            }
        } else if let Ok((mut player, ptf)) = players.single_mut() {
            let radius = player.radius.max(BODY);
            let alive = player.health > 0.0;
            let close = next.distance(ptf.translation) <= radius;
            if alive && close {
                let hit = bolt.damage;
                let mut health = player.health;
                let mut armor = player.armor;
                apply_damage(&mut health, &mut armor, hit);
                player.health = health;
                player.armor = armor;
                dead.push(entity);
                continue;
            }
        }
        tf.translation = next;
    }
    for entity in dead {
        commands.entity(entity).despawn();
    }
}

fn spawn_bolt(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
    vel: Vec3,
    damage: f32,
    friendly: bool,
) {
    let color = if friendly {
        Color::srgb(0.95, 0.72, 0.25)
    } else {
        Color::srgb(0.95, 0.25, 0.2)
    };
    commands.spawn((
        Bolt {
            vel,
            damage,
            life: 2.4,
            friendly,
        },
        Mesh3d(meshes.add(Sphere::new(4.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * 2.0,
            unlit: true,
            ..default()
        })),
        Transform::from_translation(pos),
    ));
}

fn can_reload(slot: u8, ammo: [u16; 2], reserve: [u16; 2]) -> bool {
    let w = (slot as usize).min(1);
    ammo[w] < WEAPONS[w].mag && reserve[w] > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_in_the_chamber_can_fire_and_an_empty_gun_cannot() {
        assert!(can_fire(0.0, 0.0, 1));
        assert!(!can_fire(0.0, 0.0, 0));
        assert!(!can_fire(0.14, 0.0, 30));
        assert!(!can_fire(0.0, 1.25, 30));
    }

    #[test]
    fn assault_is_faster_than_pulse_and_both_match_the_runtime() {
        assert!((WEAPONS[0].cooldown - 0.14).abs() < 1e-6);
        assert!((WEAPONS[1].cooldown - 0.5).abs() < 1e-6);
        assert_eq!(WEAPONS[0].mag, 30);
        assert_eq!(WEAPONS[1].mag, 8);
        assert!(WEAPONS[0].cooldown < WEAPONS[1].cooldown);
    }

    #[test]
    fn armor_absorbs_before_health() {
        let mut health = 100.0;
        let mut armor = 40.0;
        apply_damage(&mut health, &mut armor, 25.0);
        assert!((armor - 15.0).abs() < 1e-3);
        assert!((health - 100.0).abs() < 1e-3);
        apply_damage(&mut health, &mut armor, 20.0);
        assert!(armor.abs() < 1e-3);
        assert!((health - 95.0).abs() < 1e-3);
    }

    #[test]
    fn reload_fills_from_reserve() {
        let mut ammo = [10, 8];
        let mut reserve = [90, 24];
        finish_reload(0, &mut ammo, &mut reserve);
        assert_eq!(ammo[0], 30);
        assert_eq!(reserve[0], 70);
    }

    #[test]
    fn ray_hits_a_sphere_in_front_and_misses_a_side_step() {
        let origin = Vec3::ZERO;
        let dir = Vec3::NEG_Z;
        let hit = ray_sphere(origin, dir, Vec3::new(0.0, 0.0, -50.0), 10.0).unwrap();
        assert!((hit - 40.0).abs() < 0.2, "{hit}");
        assert!(ray_sphere(origin, dir, Vec3::new(30.0, 0.0, -50.0), 8.0).is_none());
    }
}
