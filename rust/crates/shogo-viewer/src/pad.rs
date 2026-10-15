//! Linux evdev reader for a wireless DualSense and any other HID gamepad.
//!
//! The kernel `hid-playstation` driver exposes a Bluetooth or USB DualSense as
//! `/dev/input/event*`. Face buttons use the standard evdev gamepad codes
//! (Cross = South / XInput A, Square = West / XInput X). gilrs is not used:
//! it links libudev, and the headers are not installed here. Axis ranges come
//! from `EVIOCGABS`, so trigger rest and stick center are normalized per pad.

use bevy::prelude::*;

pub const CROSS: u32 = 1 << 0;
pub const SQUARE: u32 = 1 << 1;
pub const TRIANGLE: u32 = 1 << 2;
pub const L3: u32 = 1 << 3;
pub const DPAD_UP: u32 = 1 << 4;
pub const L2_DIGITAL: u32 = 1 << 5;
pub const R2_DIGITAL: u32 = 1 << 6;

#[derive(Resource)]
pub struct Pads {
    left: Vec2,
    right: Vec2,
    l2: f32,
    r2: f32,
    buttons: u32,
    just_down: u32,
    prev: u32,
    scan: f32,
    logged_miss: bool,
    #[cfg(target_os = "linux")]
    dev: Option<LinuxPad>,
}

impl Default for Pads {
    fn default() -> Self {
        Self {
            left: Vec2::ZERO,
            right: Vec2::ZERO,
            l2: 0.0,
            r2: 0.0,
            buttons: 0,
            just_down: 0,
            prev: 0,
            scan: 0.0,
            logged_miss: false,
            #[cfg(target_os = "linux")]
            dev: None,
        }
    }
}

impl Pads {
    pub fn poll(&mut self, dt: f32) {
        self.just_down = 0;
        #[cfg(target_os = "linux")]
        {
            if self.dev.is_none() {
                self.scan -= dt;
                if self.scan <= 0.0 {
                    self.scan = 1.5;
                    match LinuxPad::open_best() {
                        Some(pad) => {
                            let link = if pad.bluetooth { "bluetooth" } else { "usb/hid" };
                            info!(
                                "gamepad '{}' ({link}, vid {:04x} pid {:04x})",
                                pad.name, pad.vendor, pad.product
                            );
                            self.left = pad.stick(ABS_X, ABS_Y, true);
                            self.right = pad.stick(ABS_RX, ABS_RY, true);
                            self.l2 = pad.trigger(ABS_Z);
                            self.r2 = pad.trigger(ABS_RZ);
                            self.dev = Some(pad);
                        }
                        None => {
                            if !self.logged_miss {
                                self.logged_miss = true;
                                info!(
                                    "no gamepad on /dev/input yet — keyboard and mouse stay active (user needs the input group for a wireless DualSense)"
                                );
                            }
                        }
                    }
                }
            }
            let mut lost = false;
            if let Some(pad) = self.dev.as_mut() {
                match pad.drain() {
                    Ok(()) => {
                        self.left = pad.stick(ABS_X, ABS_Y, true);
                        self.right = pad.stick(ABS_RX, ABS_RY, true);
                        self.l2 = pad.trigger(ABS_Z);
                        self.r2 = pad.trigger(ABS_RZ);
                        self.buttons = pad.buttons;
                    }
                    Err(()) => lost = true,
                }
            }
            if lost {
                info!("gamepad disconnected");
                self.dev = None;
                self.left = Vec2::ZERO;
                self.right = Vec2::ZERO;
                self.l2 = 0.0;
                self.r2 = 0.0;
                self.buttons = 0;
                self.scan = 0.4;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = dt;
        }
        self.just_down = self.buttons & !self.prev;
        self.prev = self.buttons;
    }

    pub fn left_stick(&self) -> Vec2 {
        self.left
    }

    pub fn right_stick(&self) -> Vec2 {
        self.right
    }

    pub fn l2(&self) -> f32 {
        self.l2
    }

    pub fn r2(&self) -> f32 {
        self.r2
    }

    pub fn down(&self, bit: u32) -> bool {
        self.buttons & bit != 0
    }

    pub fn just_down(&self, bit: u32) -> bool {
        self.just_down & bit != 0
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
    bluetooth: bool,
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
        let ok = unsafe {
            libc::ioctl(
                raw,
                ev_ioc(0x06, name_buf.len() as u32),
                name_buf.as_mut_ptr(),
            )
        };
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
        let bustype = u16::from_ne_bytes([id[0], id[1]]);
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
            bluetooth: bustype == BUS_BLUETOOTH,
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
            let down = value != 0;
            let bit = match code {
                BTN_SOUTH => Some(CROSS),
                BTN_WEST => Some(SQUARE),
                BTN_NORTH => Some(TRIANGLE),
                BTN_THUMBL => Some(L3),
                BTN_DPAD_UP => Some(DPAD_UP),
                BTN_TL2 => Some(L2_DIGITAL),
                BTN_TR2 => Some(R2_DIGITAL),
                _ => None,
            };
            if let Some(bit) = bit {
                if down {
                    self.buttons |= bit;
                } else {
                    self.buttons &= !bit;
                }
            }
        }
    }

    fn apply_hat(&mut self) {
        let hy = self.axis_value(ABS_HAT0Y);
        self.buttons &= !DPAD_UP;
        if hy < 0 {
            self.buttons |= DPAD_UP;
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
const BTN_WEST: u16 = 0x134;
#[cfg(target_os = "linux")]
const BTN_NORTH: u16 = 0x133;
#[cfg(target_os = "linux")]
const BTN_TL2: u16 = 0x138;
#[cfg(target_os = "linux")]
const BTN_TR2: u16 = 0x139;
#[cfg(target_os = "linux")]
const BTN_THUMBL: u16 = 0x13d;
#[cfg(target_os = "linux")]
const BTN_DPAD_UP: u16 = 0x220;
#[cfg(target_os = "linux")]
const BUS_BLUETOOTH: u16 = 0x05;
#[cfg(target_os = "linux")]
const SONY_VID: u16 = 0x054c;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_rests_at_zero_and_full_press_is_one() {
        assert!((norm01(0, 0, 255) - 0.0).abs() < 1e-4);
        assert!((norm01(255, 0, 255) - 1.0).abs() < 1e-4);
        assert!((norm01(128, 0, 255) - 0.5).abs() < 0.01);
    }

    #[test]
    fn stick_center_is_near_zero() {
        assert!(norm11(0, -32767, 32767).abs() < 0.02);
        assert!((norm11(32767, -32767, 32767) - 1.0).abs() < 0.02);
        assert!((norm11(-32767, -32767, 32767) + 1.0).abs() < 0.02);
    }
}
