//! Playback for the retail 16-bit WAV files.
//!
//! Bevy's audio feature needs ALSA headers, which this machine does not have.
//! `paplay` is already installed (PipeWire), so a background thread feeds it
//! raw PCM. The game keeps running if the player cannot be started.

use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use bevy::prelude::*;

#[derive(Clone)]
pub struct Clip {
    pub rate: u32,
    pub channels: u16,
    pub pcm: Vec<i16>,
}

enum Cmd {
    /// Play `sting` once, then loop `bed`. A missing sting just loops.
    Bed { sting: Option<Clip>, bed: Clip },
    Stop,
}

#[derive(Resource)]
pub struct Mixer {
    tx: Option<Sender<Cmd>>,
}

impl Mixer {
    pub fn start() -> Self {
        if std::env::var("SHOGO_MUTE").ok().as_deref() == Some("1") {
            return Self { tx: None };
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || play_thread(rx));
        Self { tx: Some(tx) }
    }

    pub fn bed(&self, sting: Option<&Path>, bed: &Path) {
        let Some(tx) = &self.tx else {
            return;
        };
        let Some(bed) = bed.and_then_clip() else {
            return;
        };
        let sting = sting.and_then(|path| path.and_then_clip());
        let _ = tx.send(Cmd::Bed { sting, bed });
    }

    pub fn sting(&self, path: &Path) {
        if self.tx.is_none() || !path.is_file() {
            return;
        }
        let _ = Command::new("paplay")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Stop);
        }
    }
}

trait ClipFile {
    fn and_then_clip(&self) -> Option<Clip>;
}

impl ClipFile for &Path {
    fn and_then_clip(&self) -> Option<Clip> {
        let bytes = std::fs::read(self).ok()?;
        decode_wav(&bytes)
    }
}

fn play_thread(rx: mpsc::Receiver<Cmd>) {
    let mut child: Option<Child> = None;
    let mut bed = Clip {
        rate: 22_050,
        channels: 1,
        pcm: Vec::new(),
    };
    let mut cursor = 0usize;
    let mut lead: Option<Clip> = None;
    loop {
        let cmd = if lead.is_none() && bed.pcm.is_empty() {
            match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            }
        } else {
            match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(cmd) => Some(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        if let Some(cmd) = cmd {
            match cmd {
                Cmd::Stop => {
                    stop(&mut child);
                    break;
                }
                Cmd::Bed { sting, bed: next } => {
                    stop(&mut child);
                    bed = next;
                    lead = sting;
                    cursor = 0;
                    child = open_play(bed.rate, bed.channels);
                }
            }
        }
        let (len, chunk) = {
            let playing = lead.as_ref().unwrap_or(&bed);
            if playing.pcm.is_empty() {
                continue;
            }
            let end = (cursor + 2048).min(playing.pcm.len());
            let mut bytes = Vec::with_capacity((end - cursor) * 2);
            for sample in &playing.pcm[cursor..end] {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            (playing.pcm.len(), bytes)
        };
        if child.is_none() {
            let rate = lead.as_ref().unwrap_or(&bed).rate;
            let channels = lead.as_ref().unwrap_or(&bed).channels;
            child = open_play(rate, channels);
        }
        let Some(proc) = child.as_mut() else {
            continue;
        };
        let Some(stdin) = proc.stdin.as_mut() else {
            continue;
        };
        if stdin.write_all(&chunk).is_err() {
            stop(&mut child);
            continue;
        }
        cursor += chunk.len() / 2;
        if cursor >= len {
            lead = None;
            cursor = 0;
        }
    }
    stop(&mut child);
}

fn stop(child: &mut Option<Child>) {
    if let Some(mut proc) = child.take() {
        let _ = proc.kill();
        let _ = proc.wait();
    }
}

fn open_play(rate: u32, channels: u16) -> Option<Child> {
    Command::new("paplay")
        .args([
            "--raw",
            "--format=s16le",
            &format!("--rate={rate}"),
            &format!("--channels={channels}"),
            "--latency-msec=80",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

pub fn decode_wav(data: &[u8]) -> Option<Clip> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut rate = 0u32;
    let mut channels = 0u16;
    let mut bits;
    let mut pcm = None;
    let mut i = 12usize;
    while i + 8 <= data.len() {
        let id = &data[i..i + 4];
        let size = u32::from_le_bytes(data[i + 4..i + 8].try_into().ok()?) as usize;
        let body = data.get(i + 8..i + 8 + size)?;
        if id == b"fmt " && body.len() >= 16 {
            let format = u16::from_le_bytes(body[0..2].try_into().ok()?);
            channels = u16::from_le_bytes(body[2..4].try_into().ok()?);
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes(body[14..16].try_into().ok()?);
            if format != 1 || bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
        } else if id == b"data" {
            let mut samples = Vec::with_capacity(body.len() / 2);
            for frame in body.chunks_exact(2) {
                samples.push(i16::from_le_bytes([frame[0], frame[1]]));
            }
            pcm = Some(samples);
        }
        i += 8 + size + (size & 1);
    }
    let pcm = pcm?;
    // Menu and mission beds sit under the gunshots. Leave a little headroom.
    let pcm = pcm
        .into_iter()
        .map(|s| ((s as i32) * 3 / 4) as i16)
        .collect();
    Some(Clip { rate, channels, pcm })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_loop_is_a_pcm_bed() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SOUND/INTERFACE/LOOP3.WAV");
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(_) => return,
        };
        let clip = decode_wav(&data).expect("loop");
        assert_eq!(clip.rate, 22_050);
        assert_eq!(clip.channels, 1);
        assert!(clip.pcm.len() > 22_050 * 10);
    }
}
