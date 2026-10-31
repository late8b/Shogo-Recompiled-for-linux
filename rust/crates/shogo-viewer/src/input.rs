//! One frame of movement and combat input.
//!
//! The stick path is tuned for a wireless DualSense: a wide radial deadzone
//! so Bluetooth drift does not walk or look, a short look smoother so a late
//! packet does not spike the camera, and a trigger threshold above resting
//! noise. Keyboard and mouse fill the same actions when the sticks are idle.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::pad::{self, Pads};

/// Inner radial deadzone. Wireless DualSense sticks sit noisier than a wired pad.
pub const STICK_INNER: f32 = 0.18;
/// Deflection treated as a full throw, then rescaled so the cap still reaches 1.
pub const STICK_OUTER: f32 = 0.96;
pub const MOVE_CURVE: f32 = 1.15;
pub const LOOK_CURVE: f32 = 1.55;
/// R2 / L2 must pass this before a shot or aim starts.
pub const TRIGGER: f32 = 0.38;
const YAW_RATE: f32 = 2.45;
const PITCH_RATE: f32 = 1.85;
const LOOK_TAU: f32 = 0.035;
/// Radians per mouse pixel. 0.003 whipped the view; this is about one turn
/// across two 1080p screen-widths.
const MOUSE_LOOK: f32 = 0.0022;

#[derive(Resource)]
pub struct Actions {
    /// x = strafe right, y = forward. Length is at most 1.
    pub wish: Vec2,
    /// Radians this frame. +x turns right, +y looks up.
    pub look: Vec2,
    pub sprint: bool,
    pub jump: bool,
    pub fire: bool,
    pub aim: bool,
    pub reload: bool,
    pub cycle: bool,
    pub toggle_scale: bool,
    pub respawn: bool,
    pub weapon_slot: Option<u8>,
    stick_look: Vec2,
}

impl Default for Actions {
    fn default() -> Self {
        Self {
            wish: Vec2::ZERO,
            look: Vec2::ZERO,
            sprint: false,
            jump: false,
            fire: false,
            aim: false,
            reload: false,
            cycle: false,
            toggle_scale: false,
            respawn: false,
            weapon_slot: None,
            stick_look: Vec2::ZERO,
        }
    }
}

pub fn gather(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut windows: Query<&mut CursorOptions>,
    grab: Option<ResMut<crate::LookGrabbed>>,
    mut pads: ResMut<Pads>,
    mut actions: ResMut<Actions>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.05);
    pads.poll(dt);

    // Esc is the pause key. The front end owns it and clears the grab.
    let locked = grab.as_ref().is_some_and(|g| g.0);
    for mut cursor in &mut windows {
        cursor.grab_mode = if locked {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        cursor.visible = !locked;
    }

    let mut key = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        key.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        key.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        key.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        key.x -= 1.0;
    }
    if key.length_squared() > 1.0 {
        key = key.normalize();
    }

    let stick = radial(
        pads.left_stick().x,
        pads.left_stick().y,
        STICK_INNER,
        STICK_OUTER,
        MOVE_CURVE,
    );
    actions.wish = if stick.length_squared() > 0.0 { stick } else { key };

    let mut look_x = 0.0;
    let mut look_y = 0.0;
    if locked {
        for ev in motion.read() {
            look_x += ev.delta.x * MOUSE_LOOK;
            look_y -= ev.delta.y * MOUSE_LOOK;
        }
    } else {
        motion.clear();
    }
    let look_stick = radial(
        pads.right_stick().x,
        pads.right_stick().y,
        STICK_INNER,
        STICK_OUTER,
        LOOK_CURVE,
    );
    let raw_rate = Vec2::new(look_stick.x * YAW_RATE, look_stick.y * PITCH_RATE);
    let blend = 1.0 - (-dt / LOOK_TAU).exp();
    actions.stick_look = actions.stick_look.lerp(raw_rate, blend);
    // Cap only the stick, so a fast mouse swipe is not flattened.
    let stick_step = (actions.stick_look * dt).clamp(Vec2::splat(-0.2), Vec2::splat(0.2));
    actions.look = Vec2::new(look_x, look_y) + stick_step;

    let r2 = pads.r2().max(if pads.down(pad::R2_DIGITAL) { 1.0 } else { 0.0 });
    let l2 = pads.l2().max(if pads.down(pad::L2_DIGITAL) { 1.0 } else { 0.0 });

    actions.sprint = keys.pressed(KeyCode::ShiftLeft)
        || keys.pressed(KeyCode::ShiftRight)
        || pads.down(pad::L3);
    actions.jump = keys.just_pressed(KeyCode::Space) || pads.just_down(pad::CROSS);
    actions.fire = mouse.pressed(MouseButton::Left) || r2 >= TRIGGER;
    actions.aim = mouse.pressed(MouseButton::Right) || l2 >= TRIGGER;
    actions.reload = keys.just_pressed(KeyCode::KeyR) || pads.just_down(pad::SQUARE);
    actions.cycle = keys.just_pressed(KeyCode::KeyQ) || pads.just_down(pad::TRIANGLE);
    actions.toggle_scale = keys.just_pressed(KeyCode::KeyF) || pads.just_down(pad::DPAD_UP);
    actions.respawn = mouse.just_pressed(MouseButton::Left)
        || pads.just_down(pad::CROSS)
        || keys.just_pressed(KeyCode::Enter);
    actions.weapon_slot = if keys.just_pressed(KeyCode::Digit1) {
        Some(0)
    } else if keys.just_pressed(KeyCode::Digit2) {
        Some(1)
    } else {
        None
    };
}

/// Radial deadzone with an outer remap and a power curve.
/// `curve` above 1 softens the first travel, which hides stick noise.
pub fn radial(x: f32, y: f32, inner: f32, outer: f32, curve: f32) -> Vec2 {
    let v = Vec2::new(x, y);
    let m = v.length();
    if !m.is_finite() || m <= inner || outer <= inner {
        return Vec2::ZERO;
    }
    let t = ((m - inner) / (outer - inner)).clamp(0.0, 1.0);
    let shaped = t.powf(curve.max(0.01));
    v / m * shaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzone_kills_bluetooth_drift_and_full_throw_reaches_one() {
        assert_eq!(radial(0.12, -0.08, STICK_INNER, STICK_OUTER, MOVE_CURVE), Vec2::ZERO);
        let full = radial(1.0, 0.0, STICK_INNER, STICK_OUTER, 1.0);
        assert!((full.x - 1.0).abs() < 1e-3, "{full:?}");
        let mid = radial(0.18 + (0.96 - 0.18) * 0.5, 0.0, STICK_INNER, STICK_OUTER, 1.0);
        assert!((mid.x - 0.5).abs() < 1e-3, "{mid:?}");
        let curved = radial(0.5, 0.0, STICK_INNER, STICK_OUTER, LOOK_CURVE);
        let linear = radial(0.5, 0.0, STICK_INNER, STICK_OUTER, 1.0);
        assert!(curved.x < linear.x);
    }
}
