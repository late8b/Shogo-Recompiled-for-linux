//! On-foot and MCA motion.
//!
//! Horizontal wish comes from `FUN_10012ac0` in `decompiled/mod2_CShellRef.c`
//! (the client move update). The engine step under it is `FUN_00436b50` in
//! `decompiled/Client.exe.c`: constant acceleration, `v += a dt`, and
//! `x += v dt + ½ a dt²`. The ½ is `_DAT_0047b61c`, the same factor
//! `FUN_004376c0` uses to turn a full extent into a half extent. LithTech
//! units, Y-up (the 1998 integrator is Z-up; vertical lives on Y here).
//!
//! `FUN_1003cde0` in `decompiled/mod3_Object.c` registers the player console
//! variables. Two of those calls pass an IEEE-754 immediate:
//!
//! - `TractorBeamSpeed` = `0x408f400000000000` = 1000
//! - `LeashLen` = `0x407c200000000000` = 450
//!
//! `RunSpeed` and `PlayerJumpVelMult` are read back from the engine cvar table
//! (`FUN_10040cb0`) with no immediate beside the call. `FUN_100121b0` does
//! write a pre-message run slot of 400 (`0x43c80000` at move+0x84) and a walk
//! slot of 0; `FUN_10013fa0` (message bit 0x10) overwrites both from the
//! server. World gravity is the vector `FUN_00436b50` adds when object flag
//! `0x20000` is set (`FUN_1001f880` / the `Gravity` property). No magnitude
//! immediate sits on that path. `FUN_100128e0`'s `-2000` (`_DAT_1006bbc0`) is
//! the steep-slope slide, not that vector. The on-foot walk, sprint, gravity,
//! and jump below stay the values the native runtime already uses.

use bevy::prelude::*;

/// `TractorBeamSpeed` immediate in `FUN_1003cde0`.
pub const TRACTOR_BEAM_SPEED: f64 = f64::from_bits(0x408f400000000000);
/// `LeashLen` immediate in `FUN_1003cde0`.
pub const LEASH_LEN: f64 = f64::from_bits(0x407c200000000000);

/// Walk cap. The move-mgr walk slot (move+0x80) is 0 in `FUN_100121b0` and
/// the `RunSpeed` cvar has no immediate. Loaded at runtime.
pub const WALK: f32 = 280.0;
/// Sprint cap. Client default 400 is only the pre-message value in
/// `FUN_100121b0`; the server overwrites it. The cvar has no immediate.
pub const RUN: f32 = 560.0;
/// World gravity magnitude. `FUN_00436b50` consumes a vector supplied by the
/// caller. No immediate in the dump. Loaded at runtime.
pub const GRAVITY: f32 = 1600.0;
/// Jump takeoff. `PlayerJumpVelMult` (`FUN_1003cde0` / `FUN_10040cb0`) has no
/// immediate. `FUN_10012ac0` adds the runtime accel on the vertical axis, and
/// multiplies by `_DAT_1006bbd8` (1.5) when command bit 0x20 is set. The
/// accel slot itself (move+100) is 0 until that message. Loaded at runtime.
pub const JUMP: f32 = 420.0;
pub const BODY: f32 = 32.0;

/// `FUN_10012ac0`: on-foot run (`FUN_10012a80` false) does `accel + accel`.
pub const FOOT_RUN_ACCEL: f32 = f32::from_bits(0x40000000);
/// `FUN_10012ac0`: mech run (`FUN_10012a80` true) multiplies by `_DAT_1006bbe0`.
pub const MECH_RUN_ACCEL: f32 = f32::from_bits(0x40200000);
/// `FUN_10012ac0` scales velocity by `_DAT_1006bbd4` when character+0x91 is set.
pub const GROUND_DAMP: f32 = f32::from_bits(0x3f666666);
/// Speeds under `_DAT_1006bbd0` after that damp are cleared.
pub const STOP_SPEED: f32 = f32::from_bits(0x3dcccccd);
/// Crouch / swim-walk / ladder-walk scale. `_DAT_1006b500` in `FUN_10012ac0`.
pub const HALF_RATE: f32 = f32::from_bits(0x3f000000);
/// Command bit 0x20 vertical scale. `_DAT_1006bbd8` in `FUN_10012ac0`.
pub const JUMP_BIT20_SCALE: f32 = f32::from_bits(0x3fc00000);
/// Steep-slope slide in `FUN_100128e0`. `_DAT_1006bbc0`. Not world gravity.
pub const SLOPE_SLIDE: f32 = f32::from_bits(0xc4fa0000);
/// Container drag scale in `FUN_10012750`. `_DAT_1006bbbc`.
pub const CONTAINER_DRAG: f32 = f32::from_bits(0x40a00000);

/// `log2(e)` constant `FUN_00436b50` feeds to `f2xm1` / `fscale`.
const LOG2_E: f64 = 1.4426950408889634;

/// Blended infantry / MCA body. `scale` 0 is on foot.
#[derive(Clone, Copy, Debug)]
pub struct BodyTune {
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

#[derive(Clone, Copy, Debug)]
pub struct Kinematics {
    pub yaw: f32,
    pub pitch: f32,
    pub vel: Vec3,
    pub grounded: bool,
    pub scale: f32,
    pub want_mca: bool,
    pub scale_lock: f32,
    pub ads: f32,
    pub radius: f32,
}

impl Default for Kinematics {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            vel: Vec3::ZERO,
            grounded: true,
            scale: 0.0,
            want_mca: false,
            scale_lock: 0.0,
            ads: 0.0,
            radius: BODY,
        }
    }
}

/// One frame of stick / key wish. `look` is radians this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveWish {
    pub wish: Vec2,
    pub look: Vec2,
    /// 1 is free look. 0 while a lock owns the camera. Friction sits in between.
    pub look_mul: f32,
    pub sprint: bool,
    pub jump: bool,
    pub aim: bool,
    pub toggle_body: bool,
}

/// Standing-contact integral from `FUN_00436b50`.
///
/// `v' = -friction v + accel`, integrated with `exp2(-friction dt log2(e))`.
/// Returns `(new_vel, delta_pos)`. `friction` is object+0x8c; the dump stores
/// no immediate there, so the caller passes the coefficient.
pub fn friction_step(vel: f32, accel: f32, friction: f32, dt: f32) -> (f32, f32) {
    if friction.abs() < 1e-6 {
        let new_vel = vel + accel * dt;
        let delta = vel * dt + 0.5 * accel * dt * dt;
        return (new_vel, delta);
    }
    let decay = libm_exp2(-friction as f64 * dt as f64 * LOG2_E) as f32;
    let new_vel = (vel - accel / friction) * decay + accel / friction;
    let delta = (accel / friction) * dt
        + ((1.0 - decay) / (friction * friction)) * (friction * vel - accel);
    (new_vel, delta)
}

fn libm_exp2(x: f64) -> f64 {
    // Same identity as the x87 sequence: exp2(x) = exp(x * ln 2).
    (x * std::f64::consts::LN_2).exp()
}

/// Integrate one tick and return the new sphere center.
/// `floor_y` is the ground plane.
pub fn integrate(pos: Vec3, body: &mut Kinematics, wish: &MoveWish, dt: f32, floor_y: f32) -> Vec3 {
    let dt = dt.clamp(0.0, 0.05);
    body.scale_lock = (body.scale_lock - dt).max(0.0);
    if wish.toggle_body && body.scale_lock <= 0.0 {
        body.want_mca = !body.want_mca;
        body.scale_lock = 0.4;
    }
    let target = if body.want_mca { 1.0 } else { 0.0 };
    body.scale = approach(body.scale, target, dt / 0.45);
    let tune = BodyTune::at(body.scale);
    body.radius = tune.radius;

    let ads_target = if wish.aim { 1.0 } else { 0.0 };
    body.ads = approach(body.ads, ads_target, dt * 7.0);

    let look_scale = (1.0 - 0.45 * body.ads) * wish.look_mul;
    body.yaw -= wish.look.x * look_scale;
    body.pitch = (body.pitch + wish.look.y * look_scale).clamp(-1.45, 1.45);

    let flat = flat_forward(body.yaw);
    let right = flat.cross(Vec3::Y);
    let wish_dir = right * wish.wish.x + flat * wish.wish.y;
    let mag = wish_dir.length().min(1.0);
    let mut dir = wish_dir.normalize_or_zero();
    let ads_slow = 1.0 - 0.4 * body.ads;
    let speed = if wish.sprint { tune.sprint } else { tune.walk } * mag * ads_slow;
    // `FUN_10012ac0` doubles on-foot run accel and scales mech run by 2.5.
    let accel_mag = tune.accel
        * if wish.sprint {
            if body.scale <= 0.001 {
                FOOT_RUN_ACCEL
            } else {
                MECH_RUN_ACCEL
            }
        } else {
            1.0
        };

    let old = body.vel;
    let mut start_x = old.x;
    let mut start_z = old.z;
    let mut ax = 0.0;
    let mut az = 0.0;

    if body.scale <= 0.001 {
        if mag < 0.05 {
            if body.grounded {
                let mut horiz = Vec3::new(start_x, 0.0, start_z) * GROUND_DAMP;
                if horiz.length() < STOP_SPEED {
                    horiz = Vec3::ZERO;
                }
                start_x = horiz.x;
                start_z = horiz.z;
            }
        } else {
            let mut horiz = Vec3::new(start_x, 0.0, start_z);
            let hlen = horiz.length();
            if speed < hlen && hlen > 1e-6 {
                horiz *= speed / hlen;
                start_x = horiz.x;
                start_z = horiz.z;
            }
            ax = dir.x * accel_mag * mag;
            az = dir.z * accel_mag * mag;
        }
    } else if mag < 0.05 {
        let stopped = approach_vec(
            Vec3::new(start_x, 0.0, start_z),
            Vec3::ZERO,
            tune.brake * dt,
        );
        start_x = stopped.x;
        start_z = stopped.z;
    } else {
        let current = Vec3::new(start_x, 0.0, start_z);
        if current.length() > 80.0 {
            dir = rotate_toward(current.normalize_or_zero(), dir, tune.yaw_rate * dt);
        }
        let steered = approach_vec(current, dir * speed, accel_mag * dt);
        start_x = steered.x;
        start_z = steered.z;
    }

    let mut vx = start_x + ax * dt;
    let mut vz = start_z + az * dt;
    if body.scale <= 0.001 && mag >= 0.05 {
        let hlen = (vx * vx + vz * vz).sqrt();
        if speed < hlen && hlen > 1e-6 && dt > 0.0 {
            let s = speed / hlen;
            vx *= s;
            vz *= s;
            ax = (vx - start_x) / dt;
            az = (vz - start_z) / dt;
        }
    }

    let mut start_y = old.y;
    if body.grounded && wish.jump {
        start_y = tune.jump;
        body.grounded = false;
    }
    let ay = if body.grounded {
        if start_y < 0.0 {
            start_y = 0.0;
        }
        0.0
    } else {
        -tune.gravity
    };
    let vy = start_y + ay * dt;

    body.vel = Vec3::new(vx, vy, vz);
    let mut center = pos
        + Vec3::new(
            start_x * dt + 0.5 * ax * dt * dt,
            start_y * dt + 0.5 * ay * dt * dt,
            start_z * dt + 0.5 * az * dt * dt,
        );
    let feet = center.y - body.radius;
    if feet <= floor_y && body.vel.y <= 0.0 {
        center.y = floor_y + body.radius;
        body.vel.y = 0.0;
        body.grounded = true;
    } else if feet > floor_y + 0.5 {
        body.grounded = false;
    }
    center
}

pub fn facing(yaw: f32, pitch: f32) -> Vec3 {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0) * Vec3::NEG_Z
}

pub fn flat_forward(yaw: f32) -> Vec3 {
    let v = facing(yaw, 0.0);
    Vec3::new(v.x, 0.0, v.z).normalize_or_zero()
}

pub fn yaw_pitch_toward(dir: Vec3) -> (f32, f32) {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return (0.0, 0.0);
    }
    let yaw = (-dir.x).atan2(-dir.z);
    let pitch = dir.y.clamp(-1.0, 1.0).asin().clamp(-1.2, 1.2);
    (yaw, pitch)
}

pub fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let mut d = to - from;
    let pi = std::f32::consts::PI;
    while d > pi {
        d -= pi * 2.0;
    }
    while d < -pi {
        d += pi * 2.0;
    }
    from + d * t.clamp(0.0, 1.0)
}

pub fn camera_rotation(yaw: f32, pitch: f32) -> Quat {
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)
}

fn approach(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = target - current;
    if delta.abs() <= max_step {
        target
    } else {
        current + delta.signum() * max_step
    }
}

fn approach_vec(current: Vec3, target: Vec3, max_step: f32) -> Vec3 {
    let delta = target - current;
    let len = delta.length();
    if len <= max_step || len < 1e-4 {
        target
    } else {
        current + delta / len * max_step
    }
}

pub fn rotate_toward(from: Vec3, to: Vec3, max_ang: f32) -> Vec3 {
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

/// Ballistic apex of a jump that leaves the ground at `jump` under constant gravity.
pub fn jump_apex(jump: f32, gravity: f32) -> f32 {
    jump * jump / (2.0 * gravity.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvar_immediates_match_fun_1003cde0() {
        assert!((TRACTOR_BEAM_SPEED - 1000.0).abs() < 1e-9);
        assert!((LEASH_LEN - 450.0).abs() < 1e-9);
    }

    #[test]
    fn move_mgr_immediates_match_the_client() {
        assert_eq!(FOOT_RUN_ACCEL.to_bits(), 0x40000000);
        assert_eq!(MECH_RUN_ACCEL.to_bits(), 0x40200000);
        assert_eq!(GROUND_DAMP.to_bits(), 0x3f666666);
        assert_eq!(STOP_SPEED.to_bits(), 0x3dcccccd);
        assert_eq!(HALF_RATE.to_bits(), 0x3f000000);
        assert_eq!(JUMP_BIT20_SCALE.to_bits(), 0x3fc00000);
        assert_eq!(SLOPE_SLIDE.to_bits(), 0xc4fa0000);
        assert_eq!(CONTAINER_DRAG.to_bits(), 0x40a00000);
        assert!((GROUND_DAMP - 0.9).abs() < 1e-6);
        assert!((MECH_RUN_ACCEL - 2.5).abs() < 1e-6);
        assert!((SLOPE_SLIDE + 2000.0).abs() < 1e-3);
        assert!((HALF_RATE - 0.5).abs() < 1e-6);
        assert!((JUMP_BIT20_SCALE - 1.5).abs() < 1e-6);
        assert!((CONTAINER_DRAG - 5.0).abs() < 1e-6);
    }

    #[test]
    fn on_foot_is_the_runtime_body_and_mca_is_heavier() {
        let foot = BodyTune::at(0.0);
        let mca = BodyTune::at(1.0);
        assert!((foot.walk - WALK).abs() < 1e-3);
        assert!((foot.sprint - RUN).abs() < 1e-3);
        assert!((foot.gravity - GRAVITY).abs() < 1e-3);
        assert!((foot.jump - JUMP).abs() < 1e-3);
        assert!((foot.radius - BODY).abs() < 1e-3);
        assert!(mca.radius > foot.radius * 2.0);
        assert!(mca.sprint > foot.sprint);
        assert!(mca.accel < foot.accel * 0.1);
        assert!(mca.jump > foot.jump);
        assert!(mca.gravity < foot.gravity);
        let mid = BodyTune::at(0.5);
        assert!(mid.radius > foot.radius && mid.radius < mca.radius);
    }

    #[test]
    fn jump_height_matches_the_ballistic_apex() {
        let tune = BodyTune::at(0.0);
        let mut body = Kinematics::default();
        let mut pos = Vec3::new(0.0, tune.radius, 0.0);
        let mut wish = MoveWish {
            look_mul: 1.0,
            jump: true,
            ..default()
        };
        let dt = 1.0 / 60.0;
        let mut peak = 0.0_f32;
        for frame in 0..600 {
            if frame > 0 {
                wish.jump = false;
            }
            pos = integrate(pos, &mut body, &wish, dt, 0.0);
            peak = peak.max(pos.y - tune.radius);
            if frame > 2 && body.grounded {
                break;
            }
        }
        let expected = jump_apex(tune.jump, tune.gravity);
        assert!(
            (peak - expected).abs() < 1.5,
            "peak {peak} expected {expected}"
        );
    }

    #[test]
    fn jump_takeoff_sets_vertical_then_gravity() {
        let mut body = Kinematics::default();
        let wish = MoveWish {
            look_mul: 1.0,
            jump: true,
            ..default()
        };
        let dt = 1.0 / 60.0;
        let _ = integrate(Vec3::new(0.0, BODY, 0.0), &mut body, &wish, dt, 0.0);
        let expected = JUMP - GRAVITY * dt;
        assert!(
            (body.vel.y - expected).abs() < 1e-2,
            "vy {} expected {expected}",
            body.vel.y
        );
        assert!(!body.grounded);
    }

    #[test]
    fn airborne_gravity_is_the_engine_step() {
        let mut body = Kinematics {
            grounded: false,
            vel: Vec3::new(40.0, 0.0, 0.0),
            ..default()
        };
        let wish = MoveWish {
            look_mul: 1.0,
            ..default()
        };
        let dt = 1.0 / 60.0;
        let _ = integrate(Vec3::new(0.0, 200.0, 0.0), &mut body, &wish, dt, 0.0);
        assert!((body.vel.y + GRAVITY * dt).abs() < 1e-2, "{}", body.vel.y);
        assert!(
            (body.vel.x - 40.0).abs() < 1e-3,
            "airborne coast keeps horizontal {}",
            body.vel.x
        );
        assert!(!body.grounded);
    }

    #[test]
    fn grounded_coast_uses_the_client_damp() {
        let mut body = Kinematics {
            vel: Vec3::new(200.0, 0.0, 0.0),
            ..default()
        };
        let wish = MoveWish {
            look_mul: 1.0,
            ..default()
        };
        let _ = integrate(
            Vec3::new(0.0, BODY, 0.0),
            &mut body,
            &wish,
            1.0 / 60.0,
            0.0,
        );
        assert!(
            (body.vel.x - 200.0 * GROUND_DAMP).abs() < 1e-3,
            "{}",
            body.vel.x
        );
        assert!(body.grounded);

        let mut slow = Kinematics {
            vel: Vec3::new(0.05, 0.0, 0.0),
            ..default()
        };
        let _ = integrate(
            Vec3::new(0.0, BODY, 0.0),
            &mut slow,
            &wish,
            1.0 / 60.0,
            0.0,
        );
        assert!(
            slow.vel.x.abs() < 1e-6 && slow.vel.z.abs() < 1e-6,
            "{:?}",
            slow.vel
        );
    }

    #[test]
    fn infantry_accel_ramps_and_sprint_doubles_it() {
        let dt = 1.0 / 60.0;
        let wish = MoveWish {
            wish: Vec2::new(0.0, 1.0),
            look_mul: 1.0,
            ..default()
        };
        let mut walk = Kinematics::default();
        let _ = integrate(Vec3::new(0.0, BODY, 0.0), &mut walk, &wish, dt, 0.0);
        let walk_speed = Vec2::new(walk.vel.x, walk.vel.z).length();
        let step = BodyTune::at(0.0).accel * dt;
        assert!(
            (walk_speed - step).abs() < 1.0,
            "walk tick {walk_speed} step {step}"
        );
        assert!(walk_speed < WALK);

        let mut sprint_body = Kinematics::default();
        let sprint = MoveWish {
            sprint: true,
            ..wish
        };
        let _ = integrate(
            Vec3::new(0.0, BODY, 0.0),
            &mut sprint_body,
            &sprint,
            dt,
            0.0,
        );
        let sprint_speed = Vec2::new(sprint_body.vel.x, sprint_body.vel.z).length();
        assert!(
            (sprint_speed / walk_speed - FOOT_RUN_ACCEL).abs() < 0.02,
            "sprint {sprint_speed} walk {walk_speed}"
        );

        let mut held = walk;
        let mut pos = Vec3::new(0.0, BODY, 0.0);
        for _ in 0..180 {
            pos = integrate(pos, &mut held, &wish, dt, 0.0);
        }
        let capped = Vec2::new(held.vel.x, held.vel.z).length();
        assert!(
            (capped - WALK).abs() < 1.0,
            "capped {capped} at {pos:?}"
        );
    }

    #[test]
    fn standing_friction_matches_fun_00436b50() {
        let (vel, delta) = friction_step(10.0, 0.0, 4.0, 0.25);
        assert!((vel - 3.6787944).abs() < 1e-4, "{vel}");
        assert!((delta - 1.5803014).abs() < 1e-4, "{delta}");
        let (vel, delta) = friction_step(10.0, 8.0, 4.0, 0.25);
        assert!((vel - 4.9430355).abs() < 1e-4, "{vel}");
        assert!((delta - 1.7642411).abs() < 1e-3, "{delta}");
        let (vel, delta) = friction_step(2.0, 4.0, 0.0, 0.5);
        assert!((vel - 4.0).abs() < 1e-5, "{vel}");
        assert!((delta - 1.5).abs() < 1e-5, "{delta}");
    }

    #[test]
    fn yaw_zero_looks_down_negative_z() {
        let f = facing(0.0, 0.0);
        assert!(f.z < -0.99, "{f:?}");
        assert!(f.x.abs() < 1e-3);
        let (yaw, pitch) = yaw_pitch_toward(Vec3::NEG_Z);
        assert!(yaw.abs() < 1e-3 && pitch.abs() < 1e-3, "{yaw} {pitch}");
    }
}
