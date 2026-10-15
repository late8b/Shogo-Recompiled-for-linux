//! Lock-on, target swap, and stick friction.
//!
//! Hold the lock bind and the camera eases onto the target with the highest
//! forward dot inside the acquire cone. A right-stick flick, or a bumper,
//! takes the next target on that side of the lens. The score is
//! `dot(camera forward, normalize(target - origin))`. Soft friction slows a
//! free look while the stick crosses an enemy volume.

use bevy::prelude::*;

use crate::input::Actions;
use crate::physics::{facing, flat_forward, yaw_pitch_toward};
use crate::player::{Player, Sim};
use crate::ui::Wheel;

/// World units. Past this distance a target is out of the lock, whatever its
/// angle. Both cones below are only tested inside this range.
pub const LOCK_RANGE: f32 = 2_800.0;

/// Acquire cone. `LOCK_DOT` is `cos(θ)` for `θ = acos(0.35) ≈ 69.5°`, so a
/// new lock only accepts targets inside a cone of that half-angle around the
/// camera forward. Anything wider is ignored.
pub const LOCK_DOT: f32 = 0.35;

/// Keep cone. `KEEP_DOT` is `cos(θ)` for `θ = acos(0.05) ≈ 87.1°`. A lock
/// already held stays until the target slips outside this wider cone, or
/// past [`LOCK_RANGE`].
pub const KEEP_DOT: f32 = 0.05;

/// Free-look friction, in radians. `0.18` rad is about `10.3°` added beyond
/// the target's own angular radius; inside that neighborhood the look rate drops.
pub const FRICTION_CONE: f32 = 0.18;

/// Absolute flick below this is noise, not a side swap. Real flicks from the
/// stick or a bumper are larger. This is a stick magnitude, not a view cone.
const FLICK_DEADZONE: f32 = 0.2;

/// Friction ignores a body closer than this. Inside about one player radius
/// the volume fills the lens and is not a tracking aid. World units, not a cone.
const FRICTION_NEAR: f32 = 48.0;

/// Friction ignores a body past this distance, in world units.
const FRICTION_FAR: f32 = 2_400.0;

/// Look-rate scale at the center of a target volume.
const FRICTION_MIN: f32 = 0.42;

/// Added to [`FRICTION_MIN`] at the rim of the friction cone, so the rate is
/// back to 1 just outside that cone.
const FRICTION_EDGE: f32 = 0.58;

#[derive(Component)]
pub struct Hostile {
    pub health: f32,
    pub max_health: f32,
    pub radius: f32,
    /// Local Y of the chest, added to the entity translation.
    pub chest: f32,
    pub shot: f32,
    pub home: Vec3,
}

#[derive(Resource, Default)]
pub struct LockOn {
    pub target: Option<Entity>,
    /// 1 right after a switch, so the camera takes the fast blend.
    pub snap: f32,
}

#[derive(Resource, Default)]
pub struct Reticle {
    /// Normalized device coordinates. +x is right, +y is up. Zero is the center.
    pub ndc: Vec2,
    pub locked: bool,
}

pub fn acquire(
    time: Res<Time>,
    sim: Res<Sim>,
    actions: Res<Actions>,
    look: Res<crate::input::LookConfig>,
    wheel: Res<Wheel>,
    mut lock: ResMut<LockOn>,
    players: Query<(&Player, &Transform)>,
    hostiles: Query<(Entity, &Hostile, &Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    lock.snap = (lock.snap - dt * 3.0).max(0.0);
    if !sim.run || wheel.open {
        lock.target = None;
        return;
    }
    let Ok((player, tf)) = players.single() else {
        return;
    };
    if player.health <= 0.0 || !look.assist {
        lock.target = None;
        return;
    }
    let origin = tf.translation;
    let forward = facing(player.yaw, player.pitch);
    let right = flat_forward(player.yaw).cross(Vec3::Y);
    let targets = live_targets(&hostiles);
    let next = update_lock(
        actions.lock,
        actions.flick,
        lock.target,
        origin,
        forward,
        right,
        &targets,
        LOCK_RANGE,
        LOCK_DOT,
    );
    if next != lock.target && next.is_some() {
        lock.snap = 1.0;
    }
    lock.target = next;
}

fn live_targets(hostiles: &Query<(Entity, &Hostile, &Transform)>) -> Vec<(Entity, Vec3)> {
    hostiles
        .iter()
        .filter(|(_, hostile, _)| hostile.health > 0.0)
        .map(|(entity, hostile, tf)| (entity, tf.translation + Vec3::Y * hostile.chest))
        .collect()
}

/// `dot(forward, normalize(target - origin))`.
///
/// `None` when the offset is zero, the target is past `range`, or the dot
/// falls below `min_dot`.
pub fn forward_score(
    origin: Vec3,
    forward: Vec3,
    target: Vec3,
    range: f32,
    min_dot: f32,
) -> Option<f32> {
    let offset = target - origin;
    let dist = offset.length();
    if dist == 0.0 || dist > range {
        return None;
    }
    let score = forward.dot(offset / dist);
    if score < min_dot {
        return None;
    }
    Some(score)
}

/// Pick, keep, or switch a lock. `flick` is negative for left, positive for right.
pub fn update_lock<T: Copy + PartialEq>(
    held: bool,
    flick: f32,
    current: Option<T>,
    origin: Vec3,
    forward: Vec3,
    right: Vec3,
    targets: &[(T, Vec3)],
    range: f32,
    min_dot: f32,
) -> Option<T> {
    if !held {
        return None;
    }
    if flick.abs() > FLICK_DEADZONE {
        if let Some(switched) =
            next_target(origin, forward, right, flick, current, targets, range, min_dot)
        {
            return Some(switched);
        }
    }
    if let Some(current) = current {
        if still_valid(current, origin, forward, targets, range, KEEP_DOT) {
            return Some(current);
        }
    }
    best_target(origin, forward, targets, range, min_dot)
}

/// Highest forward dot inside the acquire cone.
pub fn best_target<T: Copy>(
    origin: Vec3,
    forward: Vec3,
    targets: &[(T, Vec3)],
    range: f32,
    min_dot: f32,
) -> Option<T> {
    let mut best: Option<(T, f32)> = None;
    for (id, pos) in targets {
        let Some(score) = forward_score(origin, forward, *pos, range, min_dot) else {
            continue;
        };
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((*id, score));
        }
    }
    best.map(|(id, _)| id)
}

/// Next target on the flick side.
///
/// Side is the sign of `dot(target - origin, camera right)`. The candidate
/// still has to clear the same forward-dot cone as a fresh lock.
pub fn next_target<T: Copy + PartialEq>(
    origin: Vec3,
    forward: Vec3,
    right: Vec3,
    flick: f32,
    current: Option<T>,
    targets: &[(T, Vec3)],
    range: f32,
    min_dot: f32,
) -> Option<T> {
    let mut best: Option<(T, f32)> = None;
    for (id, pos) in targets {
        if current.is_some_and(|cur| cur == *id) {
            continue;
        }
        let offset = *pos - origin;
        // Same sign only. A zero dot sits on the lens axis and is neither side.
        // `signum` is wrong here: Rust maps `+0.0` to `+1.0`.
        if offset.dot(right) * flick <= 0.0 {
            continue;
        }
        let Some(score) = forward_score(origin, forward, *pos, range, min_dot) else {
            continue;
        };
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some((*id, score));
        }
    }
    best.map(|(id, _)| id)
}

fn still_valid<T: Copy + PartialEq>(
    id: T,
    origin: Vec3,
    forward: Vec3,
    targets: &[(T, Vec3)],
    range: f32,
    min_dot: f32,
) -> bool {
    targets.iter().any(|(cand, pos)| {
        *cand == id && forward_score(origin, forward, *pos, range, min_dot).is_some()
    })
}

/// Look-speed multiplier while a free aim sweeps an enemy. 1 means no slowdown.
pub fn aim_friction(origin: Vec3, forward: Vec3, targets: &[(Vec3, f32)], cone: f32) -> f32 {
    let mut best = 1.0_f32;
    for (pos, radius) in targets {
        let to = *pos - origin;
        let dist = to.length();
        if dist < FRICTION_NEAR || dist > FRICTION_FAR {
            continue;
        }
        let dir = to / dist;
        let aim = forward.dot(dir).clamp(-1.0, 1.0).acos();
        let body = (*radius / dist).atan();
        let window = body + cone;
        if aim < window {
            let t = (aim / window).clamp(0.0, 1.0);
            best = best.min(FRICTION_MIN + FRICTION_EDGE * t);
        }
    }
    best
}

/// Project `point` into normalized device coordinates for a YXZ camera.
pub fn project(
    yaw: f32,
    pitch: f32,
    fov_y: f32,
    aspect: f32,
    origin: Vec3,
    point: Vec3,
) -> Option<Vec2> {
    let rot = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    let local = rot.inverse() * (point - origin);
    if local.z >= -0.25 {
        return None;
    }
    let depth = -local.z;
    let half_h = (fov_y * 0.5).tan().max(0.05);
    let half_w = half_h * aspect.max(0.05);
    Some(Vec2::new(local.x / depth / half_w, local.y / depth / half_h))
}

pub fn desired_lock(origin: Vec3, com: Vec3) -> (f32, f32) {
    yaw_pitch_toward(com - origin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> Vec<(u32, Vec3)> {
        vec![
            (1, Vec3::new(0.0, 1.0, -10.0)),
            (2, Vec3::new(-4.0, 1.0, -12.0)),
            (3, Vec3::new(4.0, 1.0, -12.0)),
        ]
    }

    #[test]
    fn in_front_beats_beside() {
        // The beside target is much closer. A distance-biased score would
        // prefer it; the forward dot still prefers the one on the lens axis.
        let targets = vec![
            (1, Vec3::new(0.0, 0.0, -2_000.0)),
            (2, Vec3::new(30.0, 0.0, -40.0)),
        ];
        let best = best_target(Vec3::ZERO, Vec3::NEG_Z, &targets, LOCK_RANGE, LOCK_DOT);
        assert_eq!(best, Some(1));
    }

    #[test]
    fn behind_is_ignored() {
        let behind = vec![(9, Vec3::new(0.0, 0.0, 40.0))];
        let none = best_target(Vec3::ZERO, Vec3::NEG_Z, &behind, LOCK_RANGE, LOCK_DOT);
        assert_eq!(none, None);

        let mut mixed = row();
        mixed.push((9, Vec3::new(0.0, 1.0, 30.0)));
        let best = best_target(Vec3::ZERO, Vec3::NEG_Z, &mixed, LOCK_RANGE, LOCK_DOT);
        assert_eq!(best, Some(1));
    }

    #[test]
    fn right_flick_selects_the_right_hand_target() {
        let targets = row();
        let next = next_target(
            Vec3::ZERO,
            Vec3::NEG_Z,
            Vec3::X,
            1.0,
            Some(1),
            &targets,
            LOCK_RANGE,
            LOCK_DOT,
        );
        assert_eq!(next, Some(3));
    }

    #[test]
    fn target_under_the_keep_dot_is_released() {
        // Almost on the camera's right: forward dot is about 0.02, under KEEP_DOT.
        let weak = vec![(4, Vec3::new(100.0, 0.0, -2.0))];
        let got = update_lock(
            true,
            0.0,
            Some(4),
            Vec3::ZERO,
            Vec3::NEG_Z,
            Vec3::X,
            &weak,
            LOCK_RANGE,
            LOCK_DOT,
        );
        assert_eq!(got, None);
    }

    #[test]
    fn releasing_the_bind_clears_the_lock() {
        let targets = row();
        let got = update_lock(
            false,
            0.0,
            Some(1),
            Vec3::ZERO,
            Vec3::NEG_Z,
            Vec3::X,
            &targets,
            LOCK_RANGE,
            LOCK_DOT,
        );
        assert_eq!(got, None);
    }

    #[test]
    fn friction_slows_on_a_target_and_stays_open_off_to_the_side() {
        let on = aim_friction(
            Vec3::ZERO,
            Vec3::NEG_Z,
            &[(Vec3::new(0.0, 0.0, -80.0), 4.0)],
            FRICTION_CONE,
        );
        let off = aim_friction(
            Vec3::ZERO,
            Vec3::NEG_Z,
            &[(Vec3::new(80.0, 0.0, -30.0), 2.0)],
            FRICTION_CONE,
        );
        assert!(on < 0.7, "{on}");
        assert!((off - 1.0).abs() < 1e-3, "{off}");
    }

    #[test]
    fn a_point_on_the_boresight_projects_to_the_center() {
        let origin = Vec3::new(0.0, 32.0, 0.0);
        let ndc = project(
            0.0,
            0.0,
            75.0_f32.to_radians(),
            16.0 / 9.0,
            origin,
            Vec3::new(0.0, 32.0, -100.0),
        )
        .unwrap();
        assert!(ndc.length() < 0.02, "{ndc:?}");
        let right = project(
            0.0,
            0.0,
            75.0_f32.to_radians(),
            16.0 / 9.0,
            origin,
            Vec3::new(30.0, 32.0, -100.0),
        )
        .unwrap();
        assert!(right.x > 0.15, "{right:?}");
    }
}
