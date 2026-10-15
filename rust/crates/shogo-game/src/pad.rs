//! Gamepad sample shared by Bevy's [`Gamepad`] component and a Linux evdev reader.
//!
//! Bevy's gamepad backend links `libudev`. This machine has no udev headers, so
//! the Linux path reads `/dev/input/event*` directly and exposes the same
//! buttons the Bevy API names (`South`, `LeftTrigger2`, sticks). When a
//! [`Gamepad`] entity does exist, that component wins and evdev stays idle.

use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;

pub const SOUTH: u32 = 1 << 0;
pub const EAST: u32 = 1 << 1;
pub const WEST: u32 = 1 << 2;
pub const NORTH: u32 = 1 << 3;
pub const L3: u32 = 1 << 4;
pub const R3: u32 = 1 << 5;
pub const DPAD_UP: u32 = 1 << 6;
pub const DPAD_DOWN: u32 = 1 << 7;
pub const DPAD_LEFT: u32 = 1 << 8;
pub const DPAD_RIGHT: u32 = 1 << 9;
pub const L1: u32 = 1 << 10;
pub const R1: u32 = 1 << 11;
pub const L2: u32 = 1 << 12;
pub const R2: u32 = 1 << 13;
pub const START: u32 = 1 << 14;

const BUTTONS: [(GamepadButton, u32); 14] = [
    (GamepadButton::South, SOUTH),
    (GamepadButton::East, EAST),
    (GamepadButton::West, WEST),
    (GamepadButton::North, NORTH),
    (GamepadButton::LeftThumb, L3),
    (GamepadButton::RightThumb, R3),
    (GamepadButton::DPadUp, DPAD_UP),
    (GamepadButton::DPadDown, DPAD_DOWN),
    (GamepadButton::DPadLeft, DPAD_LEFT),
    (GamepadButton::DPadRight, DPAD_RIGHT),
    (GamepadButton::LeftTrigger, L1),
    (GamepadButton::RightTrigger, R1),
    (GamepadButton::LeftTrigger2, L2),
    (GamepadButton::RightTrigger2, R2),
];

#[derive(Clone, Copy, Debug, Default)]
pub struct PadFrame {
    pub left: Vec2,
    pub right: Vec2,
    pub l2: f32,
    pub r2: f32,
    pub down: u32,
    pub just: u32,
}

impl PadFrame {
    /// Read a Bevy [`Gamepad`] through the public button and stick API.
    pub fn from_gamepad(pad: &Gamepad) -> Self {
        let mut down = 0;
        let mut just = 0;
        for (button, bit) in BUTTONS {
            if pad.pressed(button) {
                down |= bit;
            }
            if pad.just_pressed(button) {
                just |= bit;
            }
        }
        if pad.pressed(GamepadButton::Start) {
            down |= START;
        }
        if pad.just_pressed(GamepadButton::Start) {
            just |= START;
        }
        let l2 = pad
            .get(GamepadButton::LeftTrigger2)
            .unwrap_or(0.0)
            .max(if down & L2 != 0 { 1.0 } else { 0.0 });
        let r2 = pad
            .get(GamepadButton::RightTrigger2)
            .unwrap_or(0.0)
            .max(if down & R2 != 0 { 1.0 } else { 0.0 });
        Self {
            left: pad.left_stick(),
            right: pad.right_stick(),
            l2,
            r2,
            down,
            just,
        }
    }

    pub fn down(&self, bit: u32) -> bool {
        self.down & bit != 0
    }

    pub fn just(&self, bit: u32) -> bool {
        self.just & bit != 0
    }
}

#[derive(Resource)]
pub struct Pads {
    frame: PadFrame,
    prev: u32,
    scan: f32,
    logged_miss: bool,
    #[cfg(target_os = "linux")]
    dev: Option<LinuxPad>,
}

impl Default for Pads {
    fn default() -> Self {
        Self {
            frame: PadFrame::default(),
            prev: 0,
            scan: 0.0,
            logged_miss: false,
            #[cfg(target_os = "linux")]
            dev: None,
        }
    }
}

impl Pads {
    pub fn frame(&self) -> PadFrame {
        self.frame
    }

    /// Drain evdev. Callers that already have a Bevy [`Gamepad`] should skip this.
    pub fn poll(&mut self, dt: f32) {
        self.frame.just = 0;
        #[cfg(target_os = "linux")]
        {
            if self.dev.is_none() {
                self.scan -= dt;
                if self.scan <= 0.0 {
                    self.scan = 1.5;
                    match LinuxPad::open_best() {
                        Some(pad) => {
                            info!("gamepad '{}' (vid {:04x} pid {:04x})", pad.name, pad.vendor, pad.product);
                            self.dev = Some(pad);
                        }
                        None => {
                            if !self.logged_miss {
                                self.logged_miss = true;
                                info!("no gamepad on /dev/input — keyboard and mouse stay active");
                            }
                        }
                    }
                }
            }
            let mut lost = false;
            if let Some(pad) = self.dev.as_mut() {
                match pad.drain() {
                    Ok(()) => {
                        self.frame.left = pad.stick(ABS_X, ABS_Y, true);
                        self.frame.right = pad.stick(ABS_RX, ABS_RY, true);
                        self.frame.l2 = pad.trigger(ABS_Z);
                        self.frame.r2 = pad.trigger(ABS_RZ);
                        self.frame.down = pad.buttons;
                    }
                    Err(()) => lost = true,
                }
            }
            if lost {
                info!("gamepad disconnected");
                self.dev = None;
                self.frame = PadFrame::default();
                self.scan = 0.4;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = dt;
        }
        self.frame.just = self.frame.down & !self.prev;
        self.prev = self.frame.down;
    }
}

pub fn norm01(value: i32, min: i32, max: i32) -> f32 {
    if max <= min {
        return 0.0;
    }
    ((value - min) as f32 / (max - min) as f32).clamp(0.0, 1.0)
}

pub fn norm11(value: i32, min: i32, max: i32) -> f32 {
    norm01(value, min, max) * 2.0 - 1.0
}

#[cfg(target_os = "linux")]
struct Axis {
    min: i32,
    max: i32,
    value: i32,
    present: bool,
}

#[cfg(target_os = "linux")]
impl Default for Axis {
    fn default() -> Self {
        Self {
            min: -1,
            max: 1,
            value: 0,
            present: false,
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxPad {
    fd: std::os::fd::OwnedFd,
    name: String,
    vendor: u16,
    product: u16,
    axes: [Axis; 8],
    buttons: u32,
}

#[cfg(target_os = "linux")]
const AXIS_CODES: [u16; 8] = [ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ, ABS_HAT0X, ABS_HAT0Y];

#[cfg(target_os = "linux")]
impl LinuxPad {
    fn open_best() -> Option<Self> {
        let mut best: Option<(i32, Self)> = None;
        for i in 0..48 {
            let path = format!("/dev/input/event{i}");
            if !std::path::Path::new(&path).exists() {
                continue;
            }
            let Some(pad) = Self::open(&path) else {
                continue;
            };
            let mut score = 10;
            let name = pad.name.to_ascii_lowercase();
            if name.contains("motion") || name.contains("touchpad") || name.contains("keyboard") {
                continue;
            }
            if !pad.axes[0].present {
                continue;
            }
            if name.contains("dualsense") {
                score += 100;
            } else if name.contains("wireless controller") || name.contains("sony") {
                score += 60;
            }
            if pad.vendor == SONY_VID {
                score += 40;
            }
            if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                best = Some((score, pad));
            }
        }
        best.map(|(_, pad)| pad)
    }

    fn open(path: &str) -> Option<Self> {
        use std::os::fd::{FromRawFd, OwnedFd};
        let c = std::ffi::CString::new(path).ok()?;
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd < 0 {
            return None;
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let raw = std::os::fd::AsRawFd::as_raw_fd(&fd);
        let mut name_buf = [0u8; 256];
        let ok = unsafe { libc::ioctl(raw, ev_ioc(0x06, name_buf.len() as u32), name_buf.as_mut_ptr()) };
        if ok < 0 {
            return None;
        }
        let name = cstr(&name_buf);
        let mut id = [0u8; 8];
        let ok = unsafe { libc::ioctl(raw, ev_ioc(0x02, 8), id.as_mut_ptr()) };
        if ok < 0 {
            return None;
        }
        let vendor = u16::from_ne_bytes([id[2], id[3]]);
        let product = u16::from_ne_bytes([id[4], id[5]]);
        let mut key_bits = [0u8; 96];
        let ok = unsafe {
            libc::ioctl(
                raw,
                ev_ioc(0x20 + EV_KEY as u32, key_bits.len() as u32),
                key_bits.as_mut_ptr(),
            )
        };
        if ok < 0 || !bit_set(&key_bits, BTN_SOUTH) {
            return None;
        }
        let mut axes = std::array::from_fn(|_| Axis::default());
        for (slot, code) in AXIS_CODES.into_iter().enumerate() {
            let mut info = [0u8; 24];
            let req = ev_ioc(0x40 + code as u32, 24);
            let ok = unsafe { libc::ioctl(raw, req, info.as_mut_ptr()) };
            if ok < 0 {
                continue;
            }
            let value = i32::from_ne_bytes(info[0..4].try_into().unwrap());
            let min = i32::from_ne_bytes(info[4..8].try_into().unwrap());
            let max = i32::from_ne_bytes(info[8..12].try_into().unwrap());
            if max > min {
                axes[slot] = Axis {
                    min,
                    max,
                    value,
                    present: true,
                };
            }
        }
        if !axes[0].present {
            return None;
        }
        Some(Self {
            fd,
            name,
            vendor,
            product,
            axes,
            buttons: 0,
        })
    }

    fn drain(&mut self) -> Result<(), ()> {
        let raw = std::os::fd::AsRawFd::as_raw_fd(&self.fd);
        loop {
            let mut buf = [0u8; 24];
            let n = unsafe { libc::read(raw, buf.as_mut_ptr().cast(), buf.len()) };
            if n == 24 {
                self.apply(&buf);
                continue;
            }
            if n == 0 {
                return Err(());
            }
            if n < 0 {
                let err = std::io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(e) if e == libc::EINTR => continue,
                    Some(e) if e == libc::EAGAIN || e == libc::EWOULDBLOCK => return Ok(()),
                    _ => return Err(()),
                }
            }
            return Ok(());
        }
    }

    fn apply(&mut self, buf: &[u8; 24]) {
        let kind = u16::from_ne_bytes([buf[16], buf[17]]);
        let code = u16::from_ne_bytes([buf[18], buf[19]]);
        let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
        if kind == EV_ABS {
            if let Some(slot) = AXIS_CODES.iter().position(|c| *c == code) {
                self.axes[slot].value = value;
            }
            if code == ABS_HAT0X || code == ABS_HAT0Y {
                self.apply_hat();
            }
        } else if kind == EV_KEY {
            let bit = match code {
                BTN_SOUTH => Some(SOUTH),
                BTN_EAST => Some(EAST),
                BTN_WEST => Some(WEST),
                BTN_NORTH => Some(NORTH),
                BTN_THUMBL => Some(L3),
                BTN_THUMBR => Some(R3),
                BTN_DPAD_UP => Some(DPAD_UP),
                BTN_DPAD_DOWN => Some(DPAD_DOWN),
                BTN_DPAD_LEFT => Some(DPAD_LEFT),
                BTN_DPAD_RIGHT => Some(DPAD_RIGHT),
                BTN_TL => Some(L1),
                BTN_TR => Some(R1),
                BTN_TL2 => Some(L2),
                BTN_TR2 => Some(R2),
                BTN_START => Some(START),
                _ => None,
            };
            if let Some(bit) = bit {
                if value != 0 {
                    self.buttons |= bit;
                } else {
                    self.buttons &= !bit;
                }
            }
        }
    }

    fn apply_hat(&mut self) {
        let hx = self.axis_value(ABS_HAT0X);
        let hy = self.axis_value(ABS_HAT0Y);
        self.buttons &= !(DPAD_UP | DPAD_DOWN | DPAD_LEFT | DPAD_RIGHT);
        if hy < 0 {
            self.buttons |= DPAD_UP;
        } else if hy > 0 {
            self.buttons |= DPAD_DOWN;
        }
        if hx < 0 {
            self.buttons |= DPAD_LEFT;
        } else if hx > 0 {
            self.buttons |= DPAD_RIGHT;
        }
    }

    fn axis_value(&self, code: u16) -> i32 {
        AXIS_CODES
            .iter()
            .position(|c| *c == code)
            .map(|i| self.axes[i].value)
            .unwrap_or(0)
    }

    fn stick(&self, x: u16, y: u16, flip_y: bool) -> Vec2 {
        let sx = self.norm_stick(x);
        let mut sy = self.norm_stick(y);
        if flip_y {
            sy = -sy;
        }
        Vec2::new(sx, sy)
    }

    fn norm_stick(&self, code: u16) -> f32 {
        let Some(slot) = AXIS_CODES.iter().position(|c| *c == code) else {
            return 0.0;
        };
        let axis = &self.axes[slot];
        if !axis.present {
            return 0.0;
        }
        norm11(axis.value, axis.min, axis.max)
    }

    fn trigger(&self, code: u16) -> f32 {
        let Some(slot) = AXIS_CODES.iter().position(|c| *c == code) else {
            return 0.0;
        };
        let axis = &self.axes[slot];
        if !axis.present {
            return 0.0;
        }
        norm01(axis.value, axis.min, axis.max)
    }
}

#[cfg(target_os = "linux")]
fn cstr(buf: &[u8]) -> String {
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

#[cfg(target_os = "linux")]
fn bit_set(buf: &[u8], code: u16) -> bool {
    let i = code as usize;
    buf.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0)
}

#[cfg(target_os = "linux")]
fn ev_ioc(nr: u32, size: u32) -> libc::Ioctl {
    const READ: u32 = 2;
    ((READ << 30) | (size << 16) | ((b'E' as u32) << 8) | nr) as libc::Ioctl
}

#[cfg(target_os = "linux")]
const EV_KEY: u16 = 0x01;
#[cfg(target_os = "linux")]
const EV_ABS: u16 = 0x03;
#[cfg(target_os = "linux")]
const ABS_X: u16 = 0x00;
#[cfg(target_os = "linux")]
const ABS_Y: u16 = 0x01;
#[cfg(target_os = "linux")]
const ABS_Z: u16 = 0x02;
#[cfg(target_os = "linux")]
const ABS_RX: u16 = 0x03;
#[cfg(target_os = "linux")]
const ABS_RY: u16 = 0x04;
#[cfg(target_os = "linux")]
const ABS_RZ: u16 = 0x05;
#[cfg(target_os = "linux")]
const ABS_HAT0X: u16 = 0x10;
#[cfg(target_os = "linux")]
const ABS_HAT0Y: u16 = 0x11;
#[cfg(target_os = "linux")]
const BTN_SOUTH: u16 = 0x130;
#[cfg(target_os = "linux")]
const BTN_EAST: u16 = 0x131;
#[cfg(target_os = "linux")]
const BTN_NORTH: u16 = 0x133;
#[cfg(target_os = "linux")]
const BTN_WEST: u16 = 0x134;
#[cfg(target_os = "linux")]
const BTN_TL: u16 = 0x136;
#[cfg(target_os = "linux")]
const BTN_TR: u16 = 0x137;
#[cfg(target_os = "linux")]
const BTN_TL2: u16 = 0x138;
#[cfg(target_os = "linux")]
const BTN_TR2: u16 = 0x139;
#[cfg(target_os = "linux")]
const BTN_START: u16 = 0x13b;
#[cfg(target_os = "linux")]
const BTN_THUMBL: u16 = 0x13d;
#[cfg(target_os = "linux")]
const BTN_THUMBR: u16 = 0x13e;
#[cfg(target_os = "linux")]
const BTN_DPAD_UP: u16 = 0x220;
#[cfg(target_os = "linux")]
const BTN_DPAD_DOWN: u16 = 0x221;
#[cfg(target_os = "linux")]
const BTN_DPAD_LEFT: u16 = 0x222;
#[cfg(target_os = "linux")]
const BTN_DPAD_RIGHT: u16 = 0x223;
#[cfg(target_os = "linux")]
const SONY_VID: u16 = 0x054c;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_rests_at_zero_and_full_press_is_one() {
        assert!((norm01(0, 0, 255) - 0.0).abs() < 1e-4);
        assert!((norm01(255, 0, 255) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn stick_center_is_near_zero() {
        assert!(norm11(0, -32767, 32767).abs() < 0.02);
        assert!((norm11(32767, -32767, 32767) - 1.0).abs() < 0.02);
    }

    #[test]
    fn left_trigger_is_the_lock_button_in_bevys_names() {
        assert_eq!(BUTTONS[12].0, GamepadButton::LeftTrigger2);
        assert_eq!(BUTTONS[12].1, L2);
        assert_eq!(BUTTONS[10].0, GamepadButton::LeftTrigger);
    }
}
