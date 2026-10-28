//! One-shot audio when a sliding door enters Opening or Closing, and when a
//! barricade breaks.
//!
//! Retail `FUN_10028d40` plays `OpenStartSound` as the phase becomes opening.
//! `FUN_10029030` plays `CloseStartSound`, or the open-start sound when close
//! was left blank. Breakable shells play `DestroySound` (`FUN_100264c0`).
//! Paths are resolved under `extracted/SOUND`. A missing `sounds\doors\tech2.wav`
//! falls back to the same file name elsewhere (that one lives in `SWITCHES`).
//! Doors with no authored sting use `DOORS/DOOROPEN1.WAV` and
//! `DOORS/DOORCLOSE.WAV`. A barricade with no `DestroySound` uses
//! `PROPS/CRATE.WAV`. There is no close phase on a breakable barricade.
//!
//! Playback is `paplay` via [`crate::tune::Mixer`]. `bevy_audio` stays off
//! (this machine has no ALSA headers). Volume falls to zero at `SoundRadius`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use shogo_assets::door_sounds::{self, DoorCue};

use crate::script::DoorPhase;
use crate::tune::Mixer;
use crate::{resolve_ci, Player};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Opening,
    Closing,
}

#[derive(Clone)]
struct Cue {
    pos: Vec3,
    open_start: String,
    close_start: String,
    destroy: String,
    radius: f32,
}

#[derive(Resource)]
pub(crate) struct DoorCues {
    by_name: HashMap<String, Cue>,
    placed: Vec<(Vec3, Cue)>,
    wavs: HashMap<String, Vec<PathBuf>>,
    root: PathBuf,
    open_fallback: Option<PathBuf>,
    close_fallback: Option<PathBuf>,
    break_fallback: Option<PathBuf>,
}

impl DoorCues {
    pub(crate) fn from_dat(data: &[u8]) -> Self {
        let list = door_sounds::read_door_cues(data).unwrap_or_default();
        Self::from_list(list)
    }

    fn from_list(list: Vec<DoorCue>) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../extracted/SOUND");
        let wavs = index_wavs(&root);
        let mut by_name = HashMap::new();
        let mut placed = Vec::new();
        for cue in list {
            let stored = Cue {
                pos: Vec3::new(cue.pos[0], cue.pos[1], cue.pos[2]),
                open_start: cue.open_start,
                close_start: cue.close_start,
                destroy: cue.destroy,
                radius: cue.radius,
            };
            placed.push((stored.pos, stored.clone()));
            by_name.insert(cue.name.to_ascii_lowercase(), stored);
        }
        let open_fallback = resolve_ci(&root, "DOORS/DOOROPEN1.WAV");
        let close_fallback = resolve_ci(&root, "DOORS/DOORCLOSE.WAV");
        let break_fallback = resolve_ci(&root, "PROPS/CRATE.WAV");
        Self {
            by_name,
            placed,
            wavs,
            root,
            open_fallback,
            close_fallback,
            break_fallback,
        }
    }

    fn resolve(&self, spec: &str) -> Option<PathBuf> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }
        let norm = spec.replace('\\', "/");
        let rel = strip_sounds(&norm);
        if let Some(hit) = resolve_ci(&self.root, rel) {
            return Some(hit);
        }
        let base = Path::new(rel)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.to_ascii_lowercase())?;
        let hits = self.wavs.get(&base)?;
        let authored = Path::new(rel)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        hits.iter()
            .min_by_key(|path| wav_rank(path, &authored))
            .cloned()
    }
}

/// Fired from the shot that drops a barricade to zero, before it is despawned.
#[derive(Event, Clone)]
pub(crate) struct BarricadeBroke {
    pub at: Vec3,
}

pub(crate) fn slide(
    tune: &Mixer,
    cues: Option<&DoorCues>,
    name: &str,
    was: DoorPhase,
    now: DoorPhase,
    at: Vec3,
    ear: Option<Vec3>,
) {
    if was == now {
        return;
    }
    let edge = match now {
        DoorPhase::Opening => Edge::Opening,
        DoorPhase::Closing => Edge::Closing,
        _ => return,
    };
    let Some(cues) = cues else {
        return;
    };
    let cue = cues.by_name.get(&name.to_ascii_lowercase());
    let (spec, fallback) = match edge {
        Edge::Opening => (
            cue.map(|c| c.open_start.as_str()).unwrap_or(""),
            cues.open_fallback.clone(),
        ),
        Edge::Closing => {
            // Retail uses the open-start handle when close was not set.
            let close = cue.map(|c| c.close_start.as_str()).unwrap_or("");
            let open = cue.map(|c| c.open_start.as_str()).unwrap_or("");
            let spec = if !close.is_empty() { close } else { open };
            (spec, cues.close_fallback.clone())
        }
    };
    let path = cues.resolve(spec).or(fallback);
    let Some(path) = path else {
        return;
    };
    let radius = cue.map(|c| c.radius).unwrap_or(0.0);
    play(tune, &path, at, ear, radius);
}

pub(crate) fn on_barricade(
    event: On<BarricadeBroke>,
    tune: Res<Mixer>,
    cues: Option<Res<DoorCues>>,
    players: Query<&Transform, With<Player>>,
) {
    let ear = players.single().ok().map(|tf| tf.translation);
    let Some(cues) = cues else {
        return;
    };
    let near = nearest(&cues.placed, event.at);
    let spec = near.map(|c| c.destroy.as_str()).unwrap_or("");
    let path = cues.resolve(spec).or_else(|| cues.break_fallback.clone());
    let Some(path) = path else {
        return;
    };
    let radius = near.map(|c| c.radius).unwrap_or(0.0);
    play(&tune, &path, event.at, ear, radius);
}

fn nearest(placed: &[(Vec3, Cue)], at: Vec3) -> Option<&Cue> {
    placed
        .iter()
        .filter(|(pos, _)| pos.distance_squared(at) <= 64.0)
        .min_by(|a, b| {
            a.0.distance_squared(at)
                .partial_cmp(&b.0.distance_squared(at))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, cue)| cue)
}

fn play(tune: &Mixer, path: &Path, at: Vec3, ear: Option<Vec3>, radius: f32) {
    let gain = match ear {
        Some(ear) => distance_gain(ear, at, radius),
        None => 1.0,
    };
    tune.sting_at(path, gain);
}

/// Linear falloff. A radius under a unit (unset in the DAT) uses 1500, the
/// common authored `SoundRadius` on breakable shells.
pub(crate) fn distance_gain(ear: Vec3, at: Vec3, radius: f32) -> f32 {
    let radius = if radius > 1.0 { radius } else { 1500.0 };
    let dist = ear.distance(at);
    if dist >= radius {
        0.0
    } else {
        1.0 - dist / radius
    }
}

fn strip_sounds(norm: &str) -> &str {
    let mut parts = norm.split('/');
    if parts.next().is_some_and(|p| p.eq_ignore_ascii_case("sounds")) {
        let rest = norm.split_once('/').map(|(_, rest)| rest).unwrap_or("");
        if rest.is_empty() { norm } else { rest }
    } else {
        norm
    }
}

fn wav_rank(path: &Path, authored_parent: &str) -> (u8, usize) {
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    let class = if !authored_parent.is_empty() && parent == authored_parent {
        0
    } else if parent == "DOORS" {
        1
    } else if parent == "SWITCHES" {
        2
    } else if parent == "PROPS" {
        3
    } else if parent == "AMBIENT" {
        8
    } else {
        4
    };
    (class, path.as_os_str().len())
}

fn index_wavs(root: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut out: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in read.flatten() {
            let path = ent.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let wav = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            if !wav {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            out.entry(name.to_ascii_lowercase())
                .or_default()
                .push(path);
        }
    }
    for paths in out.values_mut() {
        paths.sort();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_is_full_at_the_door_and_silent_past_the_radius() {
        let at = Vec3::new(100.0, 0.0, 0.0);
        assert!((distance_gain(at, at, 1000.0) - 1.0).abs() < 1e-4);
        assert!((distance_gain(at + Vec3::X * 500.0, at, 1000.0) - 0.5).abs() < 1e-4);
        assert_eq!(distance_gain(at + Vec3::X * 1000.0, at, 1000.0), 0.0);
        assert_eq!(distance_gain(at + Vec3::X * 2000.0, at, 0.0), 0.0);
    }

    #[test]
    fn quarters_lift_and_tech_door_bind_real_wavs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/02_QUARTERS.DAT");
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let cues = DoorCues::from_dat(&data);
        let lift = cues.by_name.get("mainlift").expect("mainlift");
        let open = cues.resolve(&lift.open_start).expect("openstart05");
        assert!(open
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .eq_ignore_ascii_case("OPENSTART05.WAV"));
        assert!(open.to_string_lossy().to_ascii_uppercase().contains("DOORS"));
        let door = cues.by_name.get("door2").expect("door2");
        let tech = cues.resolve(&door.open_start).expect("tech2");
        let tech_s = tech.to_string_lossy().to_ascii_uppercase();
        assert!(tech_s.contains("SWITCHES"));
        assert!(tech
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .eq_ignore_ascii_case("TECH2.WAV"));
    }

    #[test]
    fn silent_barricade_uses_the_crate_break() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        let Ok(data) = std::fs::read(path) else {
            return;
        };
        let cues = DoorCues::from_dat(&data);
        let barb = cues.by_name.get("barb1").expect("barb1");
        assert!(cues.resolve(&barb.destroy).is_none());
        let fallback = cues.break_fallback.as_ref().expect("crate");
        assert!(fallback
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .eq_ignore_ascii_case("CRATE.WAV"));
    }
}
