//! One action set for the keyboard, the mouse, and a gamepad.
//!
//! Sticks go through the same radial curve either way. The last device that
//! actually moved swaps the glyph set the menus draw. Bevy [`Gamepad`]
//! entities are preferred; the evdev reader fills the same [`PadFrame`] when
//! gilrs is not linked.

use bevy::input::gamepad::{AxisSettings, Gamepad, GamepadSettings};
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::pad::{self, PadFrame, Pads};
use crate::ui::{Phase, Wheel};

pub const STICK_INNER: f32 = 0.18;
pub const STICK_OUTER: f32 = 0.96;
pub const MOVE_CURVE: f32 = 1.15;
pub const LOOK_CURVE: f32 = 1.55;
pub const TRIGGER: f32 = 0.38;
const YAW_RATE: f32 = 2.45;
const PITCH_RATE: f32 = 1.85;
const LOOK_TAU: f32 = 0.035;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    Keyboard,
    Gamepad,
}

#[derive(Clone, Copy, Debug)]
pub struct Glyphs {
    pub confirm: &'static str,
    pub back: &'static str,
    pub lock: &'static str,
    pub fire: &'static str,
    pub jump: &'static str,
    pub wheel: &'static str,
    pub movement: &'static str,
    pub aim: &'static str,
}

impl Glyphs {
    pub fn for_device(device: Device) -> Self {
        match device {
            Device::Keyboard => Self {
                confirm: "Enter",
                back: "Esc",
                lock: "Z",
                fire: "LMB",
                jump: "Space",
                wheel: "Tab",
                movement: "WASD",
                aim: "RMB",
            },
            Device::Gamepad => Self {
                confirm: "Cross",
                back: "Circle",
                lock: "L2",
                fire: "R2",
                jump: "Cross",
                wheel: "Triangle",
                movement: "Left stick",
                aim: "L1",
            },
        }
    }
}

#[derive(Resource)]
pub struct LookConfig {
    /// Multiplier on the stick look rates. The settings screen edits this.
    pub sens: f32,
    pub assist: bool,
}

impl Default for LookConfig {
    fn default() -> Self {
        Self {
            sens: 1.0,
            assist: true,
        }
    }
}

#[derive(Resource)]
pub struct Actions {
    pub wish: Vec2,
    pub look: Vec2,
    pub sprint: bool,
    pub jump: bool,
    pub fire: bool,
    pub aim: bool,
    pub reload: bool,
    pub cycle: bool,
    pub toggle_body: bool,
    pub respawn: bool,
    pub confirm: bool,
    /// Mouse click this frame. Menus use this; fire stays held for the rifle.
    pub pointer: bool,
    pub back: bool,
    pub pause: bool,
    pub lock: bool,
    /// Signed flick this frame. Negative is left, positive is right.
    pub flick: f32,
    pub weapon_slot: Option<u8>,
    pub ui_step: IVec2,
    pub device: Device,
    stick_look: Vec2,
    prev_rx: f32,
    nav_dir: IVec2,
    nav_hold: f32,
    wheel_hold: f32,
    wheel_down: bool,
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
            toggle_body: false,
            respawn: false,
            confirm: false,
            pointer: false,
            back: false,
            pause: false,
            lock: false,
            flick: 0.0,
            weapon_slot: None,
            ui_step: IVec2::ZERO,
            device: Device::Keyboard,
            stick_look: Vec2::ZERO,
            prev_rx: 0.0,
            nav_dir: IVec2::ZERO,
            nav_hold: 0.0,
            wheel_hold: 0.0,
            wheel_down: false,
        }
    }
}

pub fn gather(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    phase: Res<State<Phase>>,
    look_cfg: Res<LookConfig>,
    mut gamepads: Query<(&Gamepad, &mut GamepadSettings)>,
    mut pads: ResMut<Pads>,
    mut actions: ResMut<Actions>,
    mut wheel: ResMut<Wheel>,
) {
    let dt = time.delta_secs().clamp(0.0, 0.05);
    let sample = if let Some((pad, mut settings)) = gamepads.iter_mut().next() {
        // Bevy's axis filter is a second deadzone. Sticks stay raw so `radial` is the only curve.
        if settings.default_axis_settings.deadzone_upperbound() != 0.0 {
            if let Ok(raw) = AxisSettings::new(-1.0, 0.0, 0.0, 1.0, 0.01) {
                settings.default_axis_settings = raw;
            }
        }
        PadFrame::from_gamepad(pad)
    } else {
        pads.poll(dt);
        pads.frame()
    };

    let menu = *phase.get() != Phase::Play || wheel.open;
    let locked_mouse = !menu && std::env::var("SHOGO_GRAB").ok().as_deref() != Some("0");

    let mut key = Vec2::ZERO;
    if !menu {
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
    }
    let stick = radial(sample.left.x, sample.left.y, STICK_INNER, STICK_OUTER, MOVE_CURVE);
    actions.wish = if menu || stick.length_squared() == 0.0 { key } else { stick };

    let mut look_x = 0.0;
    let mut look_y = 0.0;
    let mut mouse_moved = false;
    if locked_mouse {
        for ev in motion.read() {
            look_x += ev.delta.x * 0.003;
            look_y -= ev.delta.y * 0.003;
            mouse_moved = true;
        }
    } else {
        motion.clear();
    }
    let look_stick = radial(sample.right.x, sample.right.y, STICK_INNER, STICK_OUTER, LOOK_CURVE);
    let sens = look_cfg.sens.clamp(0.4, 2.0);
    let raw_rate = Vec2::new(look_stick.x * YAW_RATE * sens, look_stick.y * PITCH_RATE * sens);
    let blend = 1.0 - (-dt / LOOK_TAU).exp();
    actions.stick_look = actions.stick_look.lerp(raw_rate, blend);
    let stick_step = (actions.stick_look * dt).clamp(Vec2::splat(-0.2), Vec2::splat(0.2));
    actions.look = Vec2::new(look_x, look_y) + stick_step;

    let r2 = sample.r2.max(if sample.down(pad::R2) { 1.0 } else { 0.0 });
    let l2 = sample.l2.max(if sample.down(pad::L2) { 1.0 } else { 0.0 });
    actions.lock = l2 >= TRIGGER
        || keys.pressed(KeyCode::KeyZ)
        || mouse.pressed(MouseButton::Middle);
    actions.sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) || sample.down(pad::L3);
    actions.jump = keys.just_pressed(KeyCode::Space) || sample.just(pad::SOUTH);
    actions.fire = mouse.pressed(MouseButton::Left) || r2 >= TRIGGER;
    actions.aim = mouse.pressed(MouseButton::Right) || (sample.down(pad::L1) && !actions.lock);
    actions.reload = keys.just_pressed(KeyCode::KeyR) || sample.just(pad::WEST);
    actions.toggle_body = keys.just_pressed(KeyCode::KeyF) || sample.just(pad::DPAD_UP);
    actions.respawn = keys.just_pressed(KeyCode::Enter) || sample.just(pad::SOUTH) || mouse.just_pressed(MouseButton::Left);
    actions.confirm = keys.just_pressed(KeyCode::Enter) || sample.just(pad::SOUTH);
    actions.pointer = mouse.just_pressed(MouseButton::Left);
    actions.back = keys.just_pressed(KeyCode::Escape) || sample.just(pad::EAST);
    actions.pause = keys.just_pressed(KeyCode::Escape) || sample.just(pad::START);
    actions.weapon_slot = if keys.just_pressed(KeyCode::Digit1) {
        Some(0)
    } else if keys.just_pressed(KeyCode::Digit2) {
        Some(1)
    } else {
        None
    };

    let rx = look_stick.x;
    let mut flick = if actions.prev_rx.abs() < 0.45 && rx.abs() >= 0.72 {
        rx
    } else {
        0.0
    };
    if actions.lock && sample.just(pad::R1) {
        flick = 1.0;
    }
    if actions.lock && (sample.just(pad::L1) || keys.just_pressed(KeyCode::KeyQ)) {
        flick = -1.0;
    }
    if actions.lock && keys.just_pressed(KeyCode::KeyE) {
        flick = 1.0;
    }
    actions.flick = flick;
    actions.prev_rx = rx;

    let pad_active = stick.length_squared() > 0.0
        || look_stick.length_squared() > 0.0
        || sample.down != 0
        || sample.l2 > 0.2
        || sample.r2 > 0.2;
    let key_active = key.length_squared() > 0.0
        || mouse_moved
        || mouse.get_just_pressed().next().is_some()
        || keys.get_just_pressed().next().is_some();
    if pad_active {
        actions.device = Device::Gamepad;
    } else if key_active {
        actions.device = Device::Keyboard;
    }

    let mut nav_dir = actions.nav_dir;
    let mut nav_hold = actions.nav_hold;
    let step = if menu {
        nav_step(&keys, &sample, stick, dt, &mut nav_dir, &mut nav_hold)
    } else {
        nav_dir = IVec2::ZERO;
        nav_hold = 0.0;
        IVec2::ZERO
    };
    actions.nav_dir = nav_dir;
    actions.nav_hold = nav_hold;
    actions.ui_step = step;

    let wheel_key = keys.pressed(KeyCode::Tab) || sample.down(pad::NORTH);
    let wheel_edge = keys.just_pressed(KeyCode::Tab) || sample.just(pad::NORTH);
    if wheel_edge {
        actions.wheel_hold = 0.0;
        actions.wheel_down = true;
    }
    actions.cycle = false;
    if actions.wheel_down && wheel_key {
        actions.wheel_hold += dt;
        if actions.wheel_hold > 0.16 && *phase.get() == Phase::Play {
            wheel.open = true;
        }
    }
    if actions.wheel_down && !wheel_key {
        if !wheel.open && *phase.get() == Phase::Play {
            actions.cycle = true;
        }
        wheel.open = false;
        actions.wheel_down = false;
        actions.wheel_hold = 0.0;
    }
    if wheel.open && actions.ui_step.x != 0 {
        wheel.slot = if wheel.slot == 0 { 1 } else { 0 };
    }
}

fn nav_step(
    keys: &ButtonInput<KeyCode>,
    sample: &PadFrame,
    stick: Vec2,
    dt: f32,
    nav_dir: &mut IVec2,
    nav_hold: &mut f32,
) -> IVec2 {
    let mut analog = stick;
    if sample.down(pad::DPAD_UP) {
        analog.y = 1.0;
    }
    if sample.down(pad::DPAD_DOWN) {
        analog.y = -1.0;
    }
    if sample.down(pad::DPAD_RIGHT) {
        analog.x = 1.0;
    }
    if sample.down(pad::DPAD_LEFT) {
        analog.x = -1.0;
    }
    if keys.pressed(KeyCode::ArrowUp) || keys.pressed(KeyCode::KeyW) {
        analog.y = 1.0;
    }
    if keys.pressed(KeyCode::ArrowDown) || keys.pressed(KeyCode::KeyS) {
        analog.y = -1.0;
    }
    if keys.pressed(KeyCode::ArrowRight) || keys.pressed(KeyCode::KeyD) {
        analog.x = 1.0;
    }
    if keys.pressed(KeyCode::ArrowLeft) || keys.pressed(KeyCode::KeyA) {
        analog.x = -1.0;
    }
    let dir = dominant(analog);
    if dir == IVec2::ZERO {
        *nav_dir = IVec2::ZERO;
        *nav_hold = 0.0;
        return IVec2::ZERO;
    }
    if dir != *nav_dir {
        *nav_dir = dir;
        *nav_hold = 0.0;
        return dir;
    }
    *nav_hold += dt;
    if *nav_hold > 0.38 {
        *nav_hold = 0.22;
        dir
    } else {
        IVec2::ZERO
    }
}

fn dominant(v: Vec2) -> IVec2 {
    if v.length_squared() < 0.55 * 0.55 {
        return IVec2::ZERO;
    }
    if v.x.abs() > v.y.abs() {
        IVec2::new(if v.x > 0.0 { 1 } else { -1 }, 0)
    } else {
        IVec2::new(0, if v.y > 0.0 { 1 } else { -1 })
    }
}

pub fn grab_cursor(
    phase: Res<State<Phase>>,
    wheel: Res<Wheel>,
    mut windows: Query<&mut CursorOptions>,
) {
    let grab = *phase.get() == Phase::Play
        && !wheel.open
        && std::env::var("SHOGO_GRAB").ok().as_deref() != Some("0");
    for mut cursor in &mut windows {
        cursor.grab_mode = if grab {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        cursor.visible = !grab;
    }
}

/// Radial deadzone, then a power curve. `curve` above 1 softens the first travel.
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
    fn deadzone_kills_drift_and_full_throw_reaches_one() {
        assert_eq!(radial(0.12, -0.08, STICK_INNER, STICK_OUTER, MOVE_CURVE), Vec2::ZERO);
        let full = radial(1.0, 0.0, STICK_INNER, STICK_OUTER, 1.0);
        assert!((full.x - 1.0).abs() < 1e-3, "{full:?}");
        let mid = radial(
            STICK_INNER + (STICK_OUTER - STICK_INNER) * 0.5,
            0.0,
            STICK_INNER,
            STICK_OUTER,
            1.0,
        );
        assert!((mid.x - 0.5).abs() < 1e-3, "{mid:?}");
        let curved = radial(0.5, 0.0, STICK_INNER, STICK_OUTER, LOOK_CURVE);
        let linear = radial(0.5, 0.0, STICK_INNER, STICK_OUTER, 1.0);
        assert!(curved.length() > 0.0 && curved.length() < linear.length());
        let diag = radial(0.7, -0.7, STICK_INNER, STICK_OUTER, MOVE_CURVE);
        assert!((diag.x + diag.y).abs() < 1e-4, "{diag:?}");
    }

    #[test]
    fn glyphs_follow_the_last_device() {
        let keys = Glyphs::for_device(Device::Keyboard);
        let pad = Glyphs::for_device(Device::Gamepad);
        assert_eq!(keys.lock, "Z");
        assert_eq!(pad.lock, "L2");
        assert_ne!(keys.confirm, pad.confirm);
    }
}
