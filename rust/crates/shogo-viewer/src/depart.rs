//! How long until the Ambush dropship locks the exit.
//!
//! The DAT has no Counter and no TimedTrigger on the leave. `KeyFramer`
//! `savior` flies dropship `drop2`. Key `s5` waits **30s** on the pad and
//! sends `bailing`, then `s6` (TimeStamp 0) sends `outtahere` `Lock`.
//! The ship is called by `trans1` SendDelay **14s** and `rescue` SendDelay
//! **50s**, then keys s1–s4 take 3+4+6+5 seconds to land.
//! 14+50+18+30 = **112 seconds** from objective 1040. `s6`'s zero span is
//! the viewer's existing 0.05s key clamp, not a second clock.
//! This walks that live schedule. It does not fail the mission on its own.

use shogo_assets::dat::NodeClass;

/// Fall-back objective. Retail string 1040 is "Fall back to Drop Point Renegade".
pub(crate) const EXTRACTION_OBJECTIVE: i32 = 1040;

struct SnapNode {
    class: NodeClass,
    name: String,
    send_delay: f32,
    locked: bool,
    spent: bool,
    messages: Vec<(String, String)>,
}

struct SnapFrame {
    name: String,
    active: bool,
    cursor: usize,
    wait: f32,
    keys: Vec<usize>,
}

struct SnapKey {
    time: f32,
    target: String,
    message: String,
}

pub(crate) struct Schedule {
    nodes: Vec<SnapNode>,
    pending: Vec<(f32, String, String)>,
    framers: Vec<SnapFrame>,
    keys: Vec<SnapKey>,
}

impl Schedule {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            pending: Vec::new(),
            framers: Vec::new(),
            keys: Vec::new(),
        }
    }

    pub(crate) fn node(
        &mut self,
        class: NodeClass,
        name: &str,
        send_delay: f32,
        locked: bool,
        spent: bool,
        messages: Vec<(String, String)>,
    ) {
        self.nodes.push(SnapNode {
            class,
            name: name.to_string(),
            send_delay,
            locked,
            spent,
            messages,
        });
    }

    pub(crate) fn pending(&mut self, time: f32, target: &str, body: &str) {
        self.pending
            .push((time, target.to_string(), body.to_string()));
    }

    pub(crate) fn framer(
        &mut self,
        name: &str,
        active: bool,
        cursor: usize,
        wait: f32,
        keys: Vec<usize>,
    ) {
        self.framers.push(SnapFrame {
            name: name.to_string(),
            active,
            cursor,
            wait,
            keys,
        });
    }

    pub(crate) fn key(&mut self, time: f32, target: &str, message: &str) {
        self.keys.push(SnapKey {
            time,
            target: target.to_string(),
            message: message.to_string(),
        });
    }
}

/// `m:ss`, rounding up so the last instant still shows `0:01`.
pub(crate) fn format_depart(seconds: f32) -> String {
    let whole = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Seconds from this schedule until an open exit is locked again.
/// `None` when the graph never locks an exit that was opened.
pub(crate) fn departure_eta(schedule: &Schedule) -> Option<f32> {
    let mut nodes = schedule
        .nodes
        .iter()
        .map(|node| SnapNode {
            class: node.class,
            name: node.name.clone(),
            send_delay: node.send_delay,
            locked: node.locked,
            spent: node.spent,
            messages: node.messages.clone(),
        })
        .collect::<Vec<_>>();
    let mut pending = schedule.pending.clone();
    let mut framers = schedule
        .framers
        .iter()
        .map(|frame| SnapFrame {
            name: frame.name.clone(),
            active: frame.active,
            cursor: frame.cursor,
            wait: frame.wait,
            keys: frame.keys.clone(),
        })
        .collect::<Vec<_>>();
    let keys = &schedule.keys;
    let mut t = 0.0;
    for _ in 0..512 {
        if t > 600.0 {
            return None;
        }
        if drain(&mut nodes, &mut pending, &mut framers, keys) {
            return Some(t);
        }
        if arrive(&mut nodes, &mut pending, &mut framers, keys) {
            return Some(t);
        }
        let step = next_step(&pending, &framers)?;
        t += step;
        for item in &mut pending {
            item.0 -= step;
        }
        for frame in &mut framers {
            if frame.active {
                frame.wait -= step;
            }
        }
    }
    None
}

fn drain(
    nodes: &mut [SnapNode],
    pending: &mut Vec<(f32, String, String)>,
    framers: &mut [SnapFrame],
    keys: &[SnapKey],
) -> bool {
    for _ in 0..64 {
        let due = pending
            .iter()
            .enumerate()
            .filter(|(_, item)| item.0 <= 0.0)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if due.is_empty() {
            return false;
        }
        let batch = due
            .into_iter()
            .rev()
            .map(|i| pending.remove(i))
            .collect::<Vec<_>>();
        for (_, target, body) in batch {
            if deliver(nodes, pending, framers, keys, &target, &body) {
                return true;
            }
        }
    }
    false
}

fn arrive(
    nodes: &mut [SnapNode],
    pending: &mut Vec<(f32, String, String)>,
    framers: &mut [SnapFrame],
    keys: &[SnapKey],
) -> bool {
    let mut messages = Vec::new();
    for frame in framers.iter_mut() {
        if !frame.active || frame.cursor >= frame.keys.len() || frame.wait > 0.0 {
            continue;
        }
        let key = &keys[frame.keys[frame.cursor]];
        if !key.target.is_empty() {
            messages.push((key.target.clone(), key.message.clone()));
        }
        frame.cursor += 1;
        if frame.cursor < frame.keys.len() {
            let upcoming = &keys[frame.keys[frame.cursor]];
            frame.wait = upcoming.time.max(0.05);
        } else {
            frame.active = false;
        }
    }
    for (target, body) in messages {
        if deliver(nodes, pending, framers, keys, &target, &body) {
            return true;
        }
    }
    false
}

fn next_step(pending: &[(f32, String, String)], framers: &[SnapFrame]) -> Option<f32> {
    let pending_at = pending
        .iter()
        .map(|item| item.0)
        .filter(|time| *time > 0.0)
        .fold(f32::MAX, f32::min);
    let frame_at = framers
        .iter()
        .filter(|frame| frame.active && frame.cursor < frame.keys.len())
        .map(|frame| frame.wait)
        .filter(|wait| *wait > 0.0)
        .fold(f32::MAX, f32::min);
    let step = pending_at.min(frame_at);
    (step.is_finite() && step > 0.0).then_some(step)
}

fn deliver(
    nodes: &mut [SnapNode],
    pending: &mut Vec<(f32, String, String)>,
    framers: &mut [SnapFrame],
    keys: &[SnapKey],
    target: &str,
    body: &str,
) -> bool {
    if target.is_empty() {
        return false;
    }
    let lower = body.trim().to_ascii_lowercase();
    if lower == "unlock" || lower == "lock" {
        let lock = lower == "lock";
        let mut leaving = false;
        for node in nodes.iter_mut() {
            if !same(&node.name, target) {
                continue;
            }
            if node.class == NodeClass::Exit && lock && !node.locked {
                leaving = true;
            }
            node.locked = lock;
        }
        return leaving;
    }
    if lower == "on" {
        start_framer(framers, keys, target);
        return false;
    }
    if lower == "trigger" {
        let ids = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| same(&node.name, target))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        for id in ids {
            fire_node(nodes, pending, id);
        }
    }
    false
}

fn fire_node(nodes: &mut [SnapNode], pending: &mut Vec<(f32, String, String)>, id: usize) {
    if nodes[id].spent || nodes[id].locked {
        return;
    }
    nodes[id].spent = true;
    let delay = nodes[id].send_delay.max(0.0);
    for (target, body) in nodes[id].messages.clone() {
        pending.push((delay, target, body));
    }
}

fn start_framer(framers: &mut [SnapFrame], keys: &[SnapKey], name: &str) {
    let Some(frame) = framers.iter_mut().find(|frame| same(&frame.name, name)) else {
        return;
    };
    if frame.active || frame.keys.len() < 2 {
        return;
    }
    let span = keys[frame.keys[1]].time.max(0.05);
    frame.active = true;
    frame.cursor = 1;
    frame.wait = span;
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}
