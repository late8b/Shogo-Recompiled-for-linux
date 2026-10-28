//! Ambush mission script.
//!
//! The DAT already names every trigger, AI key, keyframer, barricade, and the
//! exit. This runs that graph: `Trigger` fans out by object name, `FollowPath`
//! walks `AIKey`s, `On` plays a keyframer, and the locked `ExitTrigger` ends
//! the mission once something unlocks it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::text::FontSize;
use shogo_assets::dat::{DatWorld, MissionData, MissionMessage, NodeClass};

use crate::combat::Enemy;
use crate::{
    find_asset_root, is_sky_portal, resolve_ci, rgba_image, world_path, world_pos, LevelEntity,
    Player,
};

/// Fired once when an `ExitTrigger` records a destination. The viewer listens
/// and swaps the loaded `.DAT` for that world.
#[derive(Event, Clone, Debug)]
pub(crate) struct LevelTransitionEvent {
    pub next_world: String,
    pub spawn_point: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ActorOrder {
    Follow {
        name: String,
        points: Vec<[f32; 3]>,
        run: bool,
    },
    Remove(String),
    Destroy(String),
}

#[derive(Clone, Debug)]
struct LiveNode {
    class: NodeClass,
    name: String,
    pos: [f32; 3],
    dims: [f32; 3],
    send_delay: f32,
    touch: bool,
    player: bool,
    ai: bool,
    ai_name: String,
    locked: bool,
    spent: bool,
    messages: Vec<MissionMessage>,
    destination: String,
    start_name: String,
    add_objectives: String,
    remove_objectives: String,
    complete_objectives: String,
    string_id: i32,
}

#[derive(Clone, Debug)]
struct LiveFramer {
    name: String,
    object: String,
    keys: Vec<usize>,
    cursor: usize,
    wait: f32,
    span: f32,
    active: bool,
}

#[derive(Clone, Debug)]
struct Pending {
    time: f32,
    target: String,
    body: String,
}

/// Pure script machine. The Bevy systems below only apply its output.
#[derive(Clone, Debug)]
pub(crate) struct Machine {
    nodes: Vec<LiveNode>,
    marker_names: Vec<String>,
    marker_pos: Vec<[f32; 3]>,
    keys: Vec<shogo_assets::dat::MotionKey>,
    framers: Vec<LiveFramer>,
    pending: Vec<Pending>,
    orders: Vec<ActorOrder>,
    objectives: Vec<i32>,
    done: Vec<i32>,
    line: String,
    line_t: f32,
    exit_world: String,
    exit_start: String,
    exit_sent: bool,
    finished: bool,
    places: Vec<(String, [f32; 3])>,
    door_orders: Vec<(String, DoorVerb)>,
}

impl Machine {
    pub(crate) fn boot(data: MissionData) -> Self {
        let nodes = data
            .nodes
            .into_iter()
            .map(|n| LiveNode {
                class: n.class,
                name: n.name,
                pos: n.pos,
                dims: n.dims,
                send_delay: n.send_delay,
                touch: n.touch,
                player: n.player,
                ai: n.ai,
                ai_name: n.ai_name,
                locked: n.locked,
                spent: false,
                messages: n.messages,
                destination: n.destination,
                start_name: n.start_name,
                add_objectives: n.add_objectives,
                remove_objectives: n.remove_objectives,
                complete_objectives: n.complete_objectives,
                string_id: n.string_id,
            })
            .collect::<Vec<_>>();
        let marker_names = data.markers.iter().map(|m| m.name.clone()).collect();
        let marker_pos = data.markers.iter().map(|m| m.pos).collect();
        let framers = data
            .framers
            .iter()
            .map(|f| {
                let keys = path_indices(&f.base, data.keys.iter().map(|k| k.name.as_str()));
                LiveFramer {
                    name: f.name.clone(),
                    object: f.object.clone(),
                    keys,
                    cursor: 0,
                    wait: 0.0,
                    span: 0.0,
                    active: false,
                }
            })
            .collect();
        let mut machine = Self {
            nodes,
            marker_names,
            marker_pos,
            keys: data.keys,
            framers,
            pending: Vec::new(),
            orders: Vec::new(),
            objectives: Vec::new(),
            done: Vec::new(),
            line: String::new(),
            line_t: 0.0,
            exit_world: String::new(),
            exit_start: String::new(),
            exit_sent: false,
            finished: false,
            places: Vec::new(),
            door_orders: Vec::new(),
        };
        machine.deliver("start", "Trigger");
        machine
    }

    pub(crate) fn tick(&mut self, dt: f32, player: [f32; 3], actors: &[(&str, [f32; 3])]) {
        self.line_t = (self.line_t - dt).max(0.0);
        if self.line_t == 0.0 && !self.finished {
            self.line = self.objective_line();
        }
        for pending in &mut self.pending {
            pending.time -= dt;
        }
        for _ in 0..64 {
            let due = self
                .pending
                .iter()
                .enumerate()
                .filter(|(_, p)| p.time <= 0.0)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if due.is_empty() {
                break;
            }
            let batch: Vec<Pending> = due.into_iter().rev().map(|i| self.pending.remove(i)).collect();
            for item in batch {
                self.deliver(&item.target, &item.body);
            }
        }
        self.advance_framers(dt);
        self.touch_player(player);
        self.touch_ai(actors);
        if self.finished {
            self.line = format!("Mission complete. Next: {}", self.exit_world);
        }
    }

    pub(crate) fn take_orders(&mut self) -> Vec<ActorOrder> {
        std::mem::take(&mut self.orders)
    }

    pub(crate) fn places(&self) -> &[(String, [f32; 3])] {
        &self.places
    }

    pub(crate) fn line(&self) -> &str {
        &self.line
    }

    pub(crate) fn objectives(&self) -> (&[i32], &[i32]) {
        (&self.objectives, &self.done)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn finished(&self) -> Option<(&str, &str)> {
        self.finished
            .then_some((self.exit_world.as_str(), self.exit_start.as_str()))
    }

    /// One destination per exit. `finished` stays readable; this is what the
    /// level loader consumes so the same exit cannot reload the world every tick.
    pub(crate) fn take_transition(&mut self) -> Option<(String, String)> {
        if self.exit_sent || !self.finished {
            return None;
        }
        self.exit_sent = true;
        Some((self.exit_world.clone(), self.exit_start.clone()))
    }

    pub(crate) fn take_door_orders(&mut self) -> Vec<(String, DoorVerb)> {
        std::mem::take(&mut self.door_orders)
    }

    pub(crate) fn deliver(&mut self, target: &str, body: &str) {
        if target.is_empty() {
            return;
        }
        let verb = body.trim();
        let lower = verb.to_ascii_lowercase();
        if lower == "unlock" || lower == "lock" {
            let lock = lower == "lock";
            self.door_orders.push((
                target.to_string(),
                if lock { DoorVerb::Lock } else { DoorVerb::Unlock },
            ));
            for node in &mut self.nodes {
                if same(node.name.as_str(), target) {
                    node.locked = lock;
                }
            }
            return;
        }
        if lower == "on" {
            self.start_framer(target);
            return;
        }
        if lower == "trigger" {
            self.door_orders.push((target.to_string(), DoorVerb::Trigger));
            let ids = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| same(n.name.as_str(), target))
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            for id in ids {
                self.fire_node(id);
            }
            return;
        }
        if let Some(order) = actor_order(target, verb, &self.marker_names, &self.marker_pos) {
            self.orders.push(order);
        }
    }

    fn fire_node(&mut self, id: usize) {
        if self.nodes[id].spent || self.nodes[id].locked {
            return;
        }
        self.nodes[id].spent = true;
        match self.nodes[id].class {
            NodeClass::Transmission => {
                let idn = self.nodes[id].string_id;
                if idn != 0 {
                    self.line = format!("Transmission {idn}");
                    self.line_t = 6.0;
                }
            }
            NodeClass::Objective => {
                apply_ids(&mut self.objectives, &self.nodes[id].add_objectives, true);
                let remove = self.nodes[id].remove_objectives.clone();
                let complete = self.nodes[id].complete_objectives.clone();
                apply_ids(&mut self.objectives, &remove, false);
                for idn in split_ids(&complete) {
                    self.objectives.retain(|x| *x != idn);
                    if !self.done.contains(&idn) {
                        self.done.push(idn);
                    }
                }
                if self.line_t == 0.0 {
                    self.line = self.objective_line();
                }
            }
            NodeClass::Exit => {
                self.exit_world = self.nodes[id].destination.clone();
                self.exit_start = self.nodes[id].start_name.clone();
                self.finished = !self.exit_world.is_empty();
            }
            NodeClass::Trigger => {}
        }
        let delay = self.nodes[id].send_delay.max(0.0);
        let messages = self.nodes[id].messages.clone();
        for message in messages {
            self.pending.push(Pending {
                time: delay,
                target: message.target,
                body: message.body,
            });
        }
    }

    fn start_framer(&mut self, name: &str) {
        let Some(idx) = self.framers.iter().position(|f| same(f.name.as_str(), name)) else {
            return;
        };
        if self.framers[idx].active || self.framers[idx].keys.len() < 2 {
            return;
        }
        let key0 = self.framers[idx].keys[0];
        let key1 = self.framers[idx].keys[1];
        let span = self.keys[key1].time.max(0.05);
        let object = self.framers[idx].object.clone();
        let pos = self.keys[key0].pos;
        self.framers[idx].active = true;
        self.framers[idx].cursor = 1;
        self.framers[idx].span = span;
        self.framers[idx].wait = span;
        self.set_place(object, pos);
    }

    fn advance_framers(&mut self, dt: f32) {
        let mut arrived: Vec<(String, String)> = Vec::new();
        let mut moved: Vec<(String, [f32; 3])> = Vec::new();
        for fi in 0..self.framers.len() {
            if !self.framers[fi].active || self.framers[fi].cursor >= self.framers[fi].keys.len() {
                continue;
            }
            self.framers[fi].wait -= dt;
            let prev_i = self.framers[fi].keys[self.framers[fi].cursor - 1];
            let next_i = self.framers[fi].keys[self.framers[fi].cursor];
            let prev = self.keys[prev_i].pos;
            let next = self.keys[next_i].pos;
            let span = self.framers[fi].span;
            let wait = self.framers[fi].wait;
            let t = (1.0 - wait / span).clamp(0.0, 1.0);
            let pos = [
                prev[0] + (next[0] - prev[0]) * t,
                prev[1] + (next[1] - prev[1]) * t,
                prev[2] + (next[2] - prev[2]) * t,
            ];
            if wait <= 0.0 {
                let target = self.keys[next_i].target.clone();
                let message = self.keys[next_i].message.clone();
                if !target.is_empty() {
                    arrived.push((target, message));
                }
                self.framers[fi].cursor += 1;
                if self.framers[fi].cursor < self.framers[fi].keys.len() {
                    let upcoming = self.framers[fi].keys[self.framers[fi].cursor];
                    let span = self.keys[upcoming].time.max(0.05);
                    self.framers[fi].span = span;
                    self.framers[fi].wait = span;
                } else {
                    self.framers[fi].active = false;
                }
                moved.push((self.framers[fi].object.clone(), next));
            } else {
                moved.push((self.framers[fi].object.clone(), pos));
            }
        }
        for (name, pos) in moved {
            self.set_place(name, pos);
        }
        for (target, body) in arrived {
            self.deliver(&target, &body);
        }
    }

    fn set_place(&mut self, name: String, pos: [f32; 3]) {
        if let Some(slot) = self.places.iter_mut().find(|(n, _)| same(n, &name)) {
            slot.1 = pos;
        } else {
            self.places.push((name, pos));
        }
    }

    fn touch_player(&mut self, player: [f32; 3]) {
        let mut fire = Vec::new();
        let mut locked_exit = false;
        for (i, node) in self.nodes.iter().enumerate() {
            if node.spent || !overlaps(player, node.pos, node.dims) {
                continue;
            }
            let exit = node.class == NodeClass::Exit && node.player;
            let touch = node.touch && node.player;
            if node.locked && exit {
                locked_exit = true;
                continue;
            }
            if exit || touch {
                fire.push(i);
            }
        }
        for id in fire {
            self.fire_node(id);
        }
        if locked_exit && self.line_t == 0.0 && !self.finished {
            self.line = "The way out is locked".to_string();
        }
    }

    fn touch_ai(&mut self, actors: &[(&str, [f32; 3])]) {
        let mut fire = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if node.spent || node.locked || !node.ai {
                continue;
            }
            let hit = actors.iter().any(|(name, pos)| {
                (node.ai_name.is_empty() || same(node.ai_name.as_str(), name))
                    && overlaps(*pos, node.pos, node.dims)
            });
            if hit {
                fire.push(i);
            }
        }
        for id in fire {
            self.fire_node(id);
        }
    }

    fn objective_line(&self) -> String {
        self.objectives
            .iter()
            .find(|id| !self.done.contains(id))
            .map(|id| format!("Objective {id}"))
            .unwrap_or_default()
    }
}

fn actor_order(
    target: &str,
    body: &str,
    names: &[String],
    pos: &[[f32; 3]],
) -> Option<ActorOrder> {
    let lower = body.to_ascii_lowercase();
    if lower == "destroy" {
        return Some(ActorOrder::Destroy(target.to_string()));
    }
    if !lower.contains(';') && lower != "remove" {
        return None;
    }
    let mut run = false;
    let mut path = None;
    let mut remove = false;
    for part in body.split(';') {
        let part = part.trim();
        let word = part.to_ascii_lowercase();
        if word.starts_with("setmovement") {
            run = word.contains("run");
        } else if let Some(rest) = word.strip_prefix("followpath") {
            path = Some(rest.trim().to_string());
        } else if word == "remove" {
            remove = true;
        }
    }
    if remove {
        return Some(ActorOrder::Remove(target.to_string()));
    }
    let path = path?;
    let points = follow_points(&path, names, pos);
    Some(ActorOrder::Follow {
        name: target.to_string(),
        points,
        run,
    })
}

/// Keys whose name is `prefix` plus a number, in numeric order.
pub(crate) fn follow_points(prefix: &str, names: &[String], pos: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut indexed = Vec::new();
    for (name, p) in names.iter().zip(pos.iter()) {
        let Some(rest) = name
            .get(prefix.len()..)
            .filter(|_| name.len() > prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix))
        else {
            continue;
        };
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(n) = rest.parse::<u32>() {
            indexed.push((n, *p));
        }
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed.into_iter().map(|(_, p)| p).collect()
}

fn path_indices<'a>(prefix: &str, names: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut indexed = Vec::new();
    for (i, name) in names.enumerate() {
        let Some(rest) = name.get(prefix.len()..).filter(|_| {
            name.len() > prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
        }) else {
            continue;
        };
        if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(n) = rest.parse::<u32>() {
            indexed.push((n, i));
        }
    }
    indexed.sort_by_key(|(n, _)| *n);
    indexed.into_iter().map(|(_, i)| i).collect()
}

fn overlaps(pos: [f32; 3], center: [f32; 3], dims: [f32; 3]) -> bool {
    let half = [
        if dims[0] < 1.0 { 16.0 } else { dims[0] },
        if dims[1] < 1.0 { 16.0 } else { dims[1] },
        if dims[2] < 1.0 { 16.0 } else { dims[2] },
    ];
    (pos[0] - center[0]).abs() <= half[0]
        && (pos[1] - center[1]).abs() <= half[1]
        && (pos[2] - center[2]).abs() <= half[2]
}

fn split_ids(text: &str) -> Vec<i32> {
    text.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

fn apply_ids(ids: &mut Vec<i32>, text: &str, add: bool) {
    for id in split_ids(text) {
        if add {
            if !ids.contains(&id) {
                ids.push(id);
            }
        } else {
            ids.retain(|x| *x != id);
        }
    }
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DoorVerb {
    Trigger,
    Lock,
    Unlock,
}

/// Retail door phase. 1 closed, 4 opening, 3 open, 2 closing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DoorPhase {
    Closed,
    Delay,
    Opening,
    Open,
    Closing,
}

/// A sliding world door. `along` is 0 at the closed pose and 1 fully open.
#[derive(Component)]
pub(crate) struct SlidingDoor {
    pub name: String,
    pub rest: Vec3,
    pub travel: Vec3,
    pub speed: f32,
    pub move_delay: f32,
    pub open_wait: f32,
    pub close_wait: f32,
    pub waveform: u8,
    /// Bit 1 touch-open, bit 2 start open, bit 4 auto-close.
    pub flags: u32,
    pub phase: DoorPhase,
    pub along: f32,
    pub wait: f32,
    pub locked: bool,
    pub half: Vec3,
    pub touch_half: Vec3,
    /// Authored object position. The brush center in `rest` can sit off this.
    pub touch: Vec3,
}

/// One authored trigger, exit, objective, or transmission volume.
#[derive(Component)]
pub(crate) struct TriggerVolume {
    pub name: String,
    pub half: Vec3,
    pub class: NodeClass,
    pub touch: bool,
    pub player: bool,
    pub occupied: bool,
}

/// Active and finished objective ids, copied out of the script each tick.
#[derive(Resource, Default)]
pub(crate) struct ObjectiveState {
    pub active: Vec<i32>,
    pub done: Vec<i32>,
}

#[derive(Resource)]
pub(crate) struct Script {
    machine: Machine,
}

impl Script {
    pub(crate) fn line(&self) -> &str {
        self.machine.line()
    }
}

#[derive(Component)]
pub(crate) struct ScriptProp(pub String);

#[derive(Component)]
pub(crate) struct Barricade {
    pub hp: f32,
    pub half: Vec3,
}

#[derive(Component)]
pub(crate) struct MissionReadout;

const DOOR_HALF: Vec3 = Vec3::new(72.0, 96.0, 36.0);

pub(crate) fn place(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(path) = world_path() else {
        return;
    };
    let Ok(data) = std::fs::read(&path) else {
        return;
    };
    commands.insert_resource(crate::door_audio::DoorCues::from_dat(&data));
    let Ok(mission) = shogo_assets::dat::read_mission(&data) else {
        return;
    };
    if mission.nodes.is_empty() && mission.doors.is_empty() {
        return;
    }
    let models = shogo_assets::dat::world_models(&data).unwrap_or_default();
    let asset_root = find_asset_root(std::path::Path::new(&path));
    let mut sliding = 0u32;
    let mut barricades = 0u32;
    let mut brushed = 0u32;
    for door in &mission.doors {
        let pos = world_pos(door.pos);
        let brush = find_model(&models, &door.name).and_then(|model| {
            door_brush(model, asset_root.as_deref(), images.as_mut())
        });
        if door.hit_points > 0.0 && door.move_dist.abs() < 1.0 {
            barricades += 1;
            let (center, half) = brush
                .as_ref()
                .map(|b| (b.center, b.half))
                .unwrap_or((pos, DOOR_HALF));
            let mut entity = commands.spawn((
                LevelEntity,
                Barricade {
                    hp: door.hit_points,
                    half,
                },
                Visibility::default(),
                Transform::from_translation(center),
            ));
            if let Some(brush) = brush {
                brushed += 1;
                attach_brush(&mut entity, &mut meshes, &mut materials, brush);
            }
        } else if door.move_dist.abs() >= 1.0 {
            sliding += 1;
            let travel = world_pos(door.move_dir) * door.move_dist;
            let (center, half) = brush
                .as_ref()
                .map(|b| (b.center, b.half))
                .unwrap_or((pos, panel_half(travel)));
            let touch_half = trigger_half(door.trigger_dims, half);
            let starts_open = door.state_flags & 2 != 0;
            let along = if starts_open { 1.0 } else { 0.0 };
            let phase = if starts_open {
                DoorPhase::Open
            } else {
                DoorPhase::Closed
            };
            let mut entity = commands.spawn((
                LevelEntity,
                SlidingDoor {
                    name: door.name.clone(),
                    rest: center,
                    travel,
                    speed: if door.speed > 1.0 { door.speed } else { 200.0 },
                    move_delay: door.move_delay.max(0.0),
                    open_wait: door.open_wait.max(0.0),
                    close_wait: door.close_wait.max(0.0),
                    waveform: door.waveform,
                    flags: door.state_flags,
                    phase,
                    along,
                    wait: 0.0,
                    locked: false,
                    half,
                    touch_half,
                    touch: pos,
                },
                Visibility::default(),
                Transform::from_translation(center + travel * wave(door.waveform, along)),
            ));
            if let Some(brush) = brush {
                brushed += 1;
                attach_brush(&mut entity, &mut meshes, &mut materials, brush);
            }
        }
    }
    for node in &mission.nodes {
        let half = Vec3::new(
            if node.dims[0] < 1.0 { 16.0 } else { node.dims[0] },
            if node.dims[1] < 1.0 { 16.0 } else { node.dims[1] },
            if node.dims[2] < 1.0 { 16.0 } else { node.dims[2] },
        );
        commands.spawn((
            LevelEntity,
            TriggerVolume {
                name: node.name.clone(),
                half,
                class: node.class,
                touch: node.touch,
                player: node.player,
                occupied: false,
            },
            Transform::from_translation(world_pos(node.pos)),
        ));
    }
    info!(
        "world interaction: {sliding} sliding doors, {barricades} barricades, {brushed} world-model brushes, {} trigger volumes",
        mission.nodes.len()
    );
    if !mission.nodes.is_empty() {
        commands.insert_resource(crate::waypoint::Guide::compile(&mission));
        commands.insert_resource(Script {
            machine: Machine::boot(mission),
        });
        commands.insert_resource(ObjectiveState::default());
    }
}

pub(crate) fn drive(
    time: Res<Time>,
    mut commands: Commands,
    mut script: Option<ResMut<Script>>,
    players: Query<&Transform, With<Player>>,
    mut actors: Query<(Entity, &mut Enemy, &Transform)>,
    mut props: Query<(Entity, &ScriptProp, &mut Transform), (Without<Enemy>, Without<Player>)>,
) {
    let Some(script) = script.as_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    let player = players
        .single()
        .map(|tf| [tf.translation.x, tf.translation.y, tf.translation.z])
        .unwrap_or([0.0, 0.0, 0.0]);
    let cast = actors
        .iter()
        .map(|(_, enemy, tf)| {
            (
                enemy.label.clone(),
                [tf.translation.x, tf.translation.y, tf.translation.z],
            )
        })
        .collect::<Vec<_>>();
    let cast_ref = cast
        .iter()
        .map(|(name, pos)| (name.as_str(), *pos))
        .collect::<Vec<_>>();
    script.machine.tick(dt, player, &cast_ref);
    let orders = script.machine.take_orders();
    for order in orders {
        match order {
            ActorOrder::Follow { name, points, run } => {
                for (_, mut enemy, _) in &mut actors {
                    if same(&enemy.label, &name) {
                        enemy.route = points.iter().copied().map(world_pos).collect();
                        enemy.route_i = 0;
                        enemy.run = run;
                    }
                }
            }
            ActorOrder::Remove(name) | ActorOrder::Destroy(name) => {
                let gone: Vec<Entity> = actors
                    .iter()
                    .filter(|(_, enemy, _)| same(&enemy.label, &name))
                    .map(|(ent, _, _)| ent)
                    .collect();
                for ent in gone {
                    commands.entity(ent).despawn();
                }
                let props_gone: Vec<Entity> = props
                    .iter()
                    .filter(|(_, prop, _)| same(&prop.0, &name))
                    .map(|(ent, _, _)| ent)
                    .collect();
                for ent in props_gone {
                    commands.entity(ent).despawn();
                }
            }
        }
    }
    for (name, pos) in script.machine.places().to_vec() {
        let pos = world_pos(pos);
        for (_, prop, mut tf) in &mut props {
            if same(&prop.0, &name) {
                tf.translation = pos;
            }
        }
    }
    publish_exit(script, &mut commands);
}

fn publish_exit(script: &mut Script, commands: &mut Commands) {
    if let Some((next_world, spawn_point)) = script.machine.take_transition() {
        commands.trigger(LevelTransitionEvent {
            next_world,
            spawn_point,
        });
    }
}

pub(crate) fn readout(
    dialogue: Res<crate::transmission::Dialogue>,
    script: Option<Res<Script>>,
    mut commands: Commands,
    mut lines: Query<&mut Text, With<MissionReadout>>,
) {
    let Some(script) = script else {
        return;
    };
    let line = dialogue.line(script.line());
    if lines.is_empty() {
        commands.spawn((
            LevelEntity,
            MissionReadout,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(28.0),
                top: Val::Px(28.0),
                ..default()
            },
            Text::new(line),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(Color::srgb(0.96, 0.93, 0.84)),
        ));
        return;
    }
    if let Ok(mut text) = lines.single_mut() {
        *text = Text::new(line);
    }
}

/// Player sphere against each trigger volume. The script still fires the node;
/// this records the overlap on the component.
pub(crate) fn touch_volumes(
    mut commands: Commands,
    mut script: Option<ResMut<Script>>,
    objectives: Option<ResMut<ObjectiveState>>,
    guide: Option<Res<crate::waypoint::Guide>>,
    players: Query<(&Player, &Transform)>,
    mut volumes: Query<(Entity, &mut TriggerVolume, &Transform)>,
    mut markers: Query<(Entity, &mut crate::waypoint::MissionMarker)>,
) {
    let Some((body, ptf)) = players.single().ok() else {
        return;
    };
    for (_ent, mut volume, tf) in &mut volumes {
        let was = volume.occupied;
        volume.occupied = sphere_hits(ptf.translation, body.radius, tf.translation, volume.half);
        let armed = volume.player && (volume.touch || volume.class == NodeClass::Exit);
        if volume.occupied && !was && armed {
            if let Some(script) = script.as_mut() {
                script.machine.deliver(&volume.name, "Trigger");
            }
        }
    }
    if let Some(script) = script.as_mut() {
        if let Some(mut objectives) = objectives {
            let (active, done) = script.machine.objectives();
            objectives.active = active.to_vec();
            objectives.done = done.to_vec();
            if let Some(guide) = guide.as_deref() {
                crate::waypoint::retarget(&mut commands, guide, &objectives, &volumes, &mut markers);
            }
        }
        publish_exit(script, &mut commands);
    }
}

/// Slide doors. A `Trigger` message, or the player in the touch box, starts them.
pub(crate) fn doors(
    time: Res<Time>,
    tune: Res<crate::tune::Mixer>,
    cues: Option<Res<crate::door_audio::DoorCues>>,
    mut script: Option<ResMut<Script>>,
    players: Query<&Transform, With<Player>>,
    mut doors: Query<(&mut SlidingDoor, &mut Transform), Without<Player>>,
) {
    let dt = time.delta_secs().min(0.05);
    let orders = script
        .as_mut()
        .map(|script| script.machine.take_door_orders())
        .unwrap_or_default();
    let player = players.single().ok().map(|tf| tf.translation);
    for (mut door, mut tf) in &mut doors {
        let was = door.phase;
        for (name, verb) in &orders {
            if !same(&door.name, name) {
                continue;
            }
            match verb {
                DoorVerb::Lock => door.locked = true,
                DoorVerb::Unlock => door.locked = false,
                DoorVerb::Trigger => ask_open(&mut door),
            }
        }
        if door.flags & 1 != 0 {
            if let Some(player) = player {
                if !door.locked && sphere_hits(player, 32.0, door.touch, door.touch_half) {
                    ask_open(&mut door);
                }
            }
        }
        step_door(&mut door, dt);
        let shaped = wave(door.waveform, door.along);
        tf.translation = door.rest + door.travel * shaped;
        crate::door_audio::slide(
            &tune,
            cues.as_deref(),
            &door.name,
            was,
            door.phase,
            tf.translation,
            player,
        );
    }
}

fn ask_open(door: &mut SlidingDoor) {
    if door.locked || door.wait > 0.0 && door.phase == DoorPhase::Closed {
        return;
    }
    match door.phase {
        DoorPhase::Closed => {
            if door.move_delay > 0.0 {
                door.phase = DoorPhase::Delay;
                door.wait = door.move_delay;
            } else {
                door.phase = DoorPhase::Opening;
            }
        }
        DoorPhase::Closing => door.phase = DoorPhase::Opening,
        DoorPhase::Delay | DoorPhase::Opening | DoorPhase::Open => {}
    }
}

fn step_door(door: &mut SlidingDoor, dt: f32) {
    let span = (door.travel.length() / door.speed).max(0.05);
    match door.phase {
        DoorPhase::Delay => {
            door.wait -= dt;
            if door.wait <= 0.0 {
                door.wait = 0.0;
                door.phase = DoorPhase::Opening;
            }
        }
        DoorPhase::Opening => {
            door.along = (door.along + dt / span).min(1.0);
            if door.along >= 1.0 {
                door.phase = DoorPhase::Open;
                door.wait = door.open_wait;
            }
        }
        DoorPhase::Open => {
            if door.flags & 4 != 0 {
                door.wait -= dt;
                if door.wait <= 0.0 {
                    door.wait = 0.0;
                    door.phase = DoorPhase::Closing;
                }
            }
        }
        DoorPhase::Closing => {
            door.along = (door.along - dt / span).max(0.0);
            if door.along <= 0.0 {
                door.phase = DoorPhase::Closed;
                door.wait = door.close_wait;
            }
        }
        DoorPhase::Closed => {
            if door.wait > 0.0 {
                door.wait = (door.wait - dt).max(0.0);
            }
        }
    }
}

/// LithTech wave, pinned so 0 is shut and 1 is fully open.
pub(crate) fn wave(kind: u8, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match kind {
        // Ease through the travel and finish open. A full-period sine would
        // land back on the closed pose.
        1 => t * t * (3.0 - 2.0 * t),
        2 => (t * std::f32::consts::FRAC_PI_2).sin(),
        3 => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
        _ => t,
    }
}

struct DoorBrush {
    center: Vec3,
    half: Vec3,
    parts: Vec<(Mesh, StandardMaterial)>,
}

fn find_model<'a>(models: &'a [DatWorld], name: &str) -> Option<&'a DatWorld> {
    models
        .iter()
        .find(|model| model.name == name)
        .or_else(|| models.iter().find(|model| model.name.eq_ignore_ascii_case(name)))
}

/// WorldModel brush recentered on its point cloud so the entity origin is the
/// box the collision queries already assume.
fn door_brush(
    model: &DatWorld,
    asset_root: Option<&std::path::Path>,
    images: &mut Assets<Image>,
) -> Option<DoorBrush> {
    if model.groups.is_empty() {
        return None;
    }
    let center = world_pos([
        (model.min[0] + model.max[0]) * 0.5,
        (model.min[1] + model.max[1]) * 0.5,
        (model.min[2] + model.max[2]) * 0.5,
    ]);
    let span = world_pos(model.max) - world_pos(model.min);
    let half = Vec3::new(
        (span.x.abs() * 0.5).max(2.0),
        (span.y.abs() * 0.5).max(2.0),
        (span.z.abs() * 0.5).max(2.0),
    );
    let mut parts = Vec::new();
    for group in &model.groups {
        let tex_name = model
            .textures
            .get(group.texture as usize)
            .map(|s| s.as_str())
            .unwrap_or("");
        if is_sky_portal(tex_name) {
            continue;
        }
        let decoded = asset_root
            .and_then(|root| resolve_ci(root, tex_name))
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| shogo_assets::dtx::decode(&bytes).ok());
        let (tw, th) = decoded
            .as_ref()
            .map(|tex| (tex.width as f32, tex.height as f32))
            .filter(|(w, h)| *w > 0.0 && *h > 0.0)
            .unwrap_or((64.0, 64.0));
        let positions: Vec<[f32; 3]> = group
            .positions
            .iter()
            .map(|p| {
                let g = world_pos(*p) - center;
                [g.x, g.y, g.z]
            })
            .collect();
        let uvs: Vec<[f32; 2]> = group.uvs.iter().map(|[u, v]| [u / tw, 1.0 - v / th]).collect();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(group.indices.clone()));
        mesh.compute_area_weighted_normals();
        let base_color = if decoded.is_some() {
            Color::WHITE
        } else {
            Color::srgb(0.35, 0.35, 0.35)
        };
        parts.push((
            mesh,
            StandardMaterial {
                base_color,
                base_color_texture: decoded.map(|tex| {
                    images.add(rgba_image(tex.width, tex.height, tex.rgba))
                }),
                perceptual_roughness: 1.0,
                metallic: 0.0,
                reflectance: 0.0,
                unlit: false,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(DoorBrush { center, half, parts })
    }
}

fn attach_brush(
    entity: &mut EntityCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    brush: DoorBrush,
) {
    let handles: Vec<_> = brush
        .parts
        .into_iter()
        .map(|(mesh, material)| (meshes.add(mesh), materials.add(material)))
        .collect();
    entity.with_children(|parent| {
        for (mesh, material) in handles {
            parent.spawn((Mesh3d(mesh), MeshMaterial3d(material)));
        }
    });
}

fn panel_half(travel: Vec3) -> Vec3 {
    let dir = travel.normalize_or_zero();
    Vec3::new(
        if dir.x.abs() > 0.6 { 12.0 } else { 56.0 },
        if dir.y.abs() > 0.6 { 12.0 } else { 96.0 },
        if dir.z.abs() > 0.6 { 12.0 } else { 56.0 },
    )
}

fn trigger_half(dims: [f32; 3], panel: Vec3) -> Vec3 {
    if dims[0].abs() < 1.0 && dims[1].abs() < 1.0 && dims[2].abs() < 1.0 {
        Vec3::new(panel.x + 48.0, panel.y + 48.0, panel.z + 48.0)
    } else {
        Vec3::new(dims[0].abs().max(8.0), dims[1].abs().max(8.0), dims[2].abs().max(8.0))
    }
}

fn sphere_hits(center: Vec3, radius: f32, box_center: Vec3, half: Vec3) -> bool {
    let delta = center - box_center;
    let closest = delta.clamp(-half, half);
    (delta - closest).length_squared() <= radius * radius
}

/// Push a sphere out of an axis-aligned barricade. `half` is the box half-size.
pub(crate) fn push_sphere(mut center: Vec3, radius: f32, box_center: Vec3, half: Vec3) -> Vec3 {
    let delta = center - box_center;
    let ext = half + Vec3::splat(radius);
    if delta.x.abs() >= ext.x || delta.y.abs() >= ext.y || delta.z.abs() >= ext.z {
        return center;
    }
    let px = ext.x - delta.x.abs();
    let py = ext.y - delta.y.abs();
    let pz = ext.z - delta.z.abs();
    if px <= py && px <= pz {
        let sign = if delta.x < 0.0 { -1.0 } else { 1.0 };
        center.x += px * sign;
    } else if py <= pz {
        let sign = if delta.y < 0.0 { -1.0 } else { 1.0 };
        center.y += py * sign;
    } else {
        let sign = if delta.z < 0.0 { -1.0 } else { 1.0 };
        center.z += pz * sign;
    }
    center
}

/// First hit of a ray against an axis-aligned box. `None` when the origin is
/// inside the box or the ray misses.
pub(crate) fn ray_aabb(origin: Vec3, dir: Vec3, center: Vec3, half: Vec3) -> Option<f32> {
    let min = center - half;
    let max = center + half;
    let mut t0 = 0.0_f32;
    let mut t1 = f32::MAX;
    for i in 0..3 {
        let o = origin[i];
        let d = dir[i];
        if d.abs() < 1e-8 {
            if o < min[i] || o > max[i] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d;
        let mut ta = (min[i] - o) * inv;
        let mut tb = (max[i] - o) * inv;
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    (t0 >= 0.0).then_some(t0)
}

pub(crate) fn harm(door: &mut Barricade, amount: f32) -> bool {
    door.hp = (door.hp - amount).max(0.0);
    door.hp <= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogo_assets::dat::{Framer, Marker, MissionMessage, MissionNode, MotionKey};

    fn node(
        class: NodeClass,
        name: &str,
        pos: [f32; 3],
        locked: bool,
        messages: Vec<(&str, &str)>,
    ) -> MissionNode {
        MissionNode {
            class,
            name: name.to_string(),
            pos,
            dims: [32.0, 32.0, 32.0],
            send_delay: 0.0,
            touch: true,
            player: true,
            ai: false,
            ai_name: String::new(),
            locked,
            messages: messages
                .into_iter()
                .map(|(target, body)| MissionMessage {
                    target: target.to_string(),
                    body: body.to_string(),
                })
                .collect(),
            destination: if class == NodeClass::Exit {
                "02_Quarters".into()
            } else {
                String::new()
            },
            start_name: "start1".into(),
            add_objectives: if name == "obj1" { "1040".into() } else { String::new() },
            remove_objectives: String::new(),
            complete_objectives: if name == "obj2" { "1040".into() } else { String::new() },
            string_id: if class == NodeClass::Transmission { 3671 } else { 0 },
        }
    }

    #[test]
    fn follow_path_uses_numbered_keys_only() {
        let names = ["b0", "b1", "b10", "barb1", "b2"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let pos = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [9.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
        ];
        let points = follow_points("b", &names, &pos);
        assert_eq!(
            points,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [10.0, 0.0, 0.0]
            ]
        );
    }

    #[test]
    fn start_sends_the_squad_along_its_keys() {
        let data = MissionData {
            nodes: vec![node(
                NodeClass::Trigger,
                "start",
                [0.0, 0.0, 0.0],
                false,
                vec![(
                    "dudley",
                    "Script 24;SetMovement RUN;FollowPath b;",
                )],
            )],
            markers: vec![
                Marker {
                    name: "b0".into(),
                    pos: [10.0, 0.0, 0.0],
                },
                Marker {
                    name: "b1".into(),
                    pos: [20.0, 0.0, 0.0],
                },
            ],
            keys: Vec::new(),
            framers: Vec::new(),
            doors: Vec::new(),
        };
        let mut machine = Machine::boot(data);
        machine.tick(0.0, [0.0, 0.0, 0.0], &[]);
        let orders = machine.take_orders();
        match &orders[..] {
            [ActorOrder::Follow { name, points, run }] => {
                assert_eq!(name, "dudley");
                assert!(run);
                assert_eq!(points.len(), 2);
            }
            other => panic!("expected a follow order, got {other:?}"),
        }
    }

    #[test]
    fn locked_exit_opens_after_unlock_and_ends_the_mission() {
        let data = MissionData {
            nodes: vec![
                node(NodeClass::Trigger, "start", [0.0, 0.0, 0.0], false, vec![]),
                node(
                    NodeClass::Exit,
                    "outtahere",
                    [100.0, 0.0, 0.0],
                    true,
                    vec![("obj2", "Trigger")],
                ),
                node(NodeClass::Objective, "obj1", [0.0, 0.0, 0.0], false, vec![]),
                node(NodeClass::Objective, "obj2", [0.0, 0.0, 0.0], false, vec![]),
            ],
            markers: Vec::new(),
            keys: Vec::new(),
            framers: Vec::new(),
            doors: vec![shogo_assets::dat::Door {
                name: "barb1".into(),
                pos: [0.0, 0.0, 0.0],
                hit_points: 100.0,
                move_dist: 0.0,
                move_dir: [0.0, 0.0, 0.0],
                speed: 0.0,
                move_delay: 0.0,
                open_wait: 0.0,
                close_wait: 0.0,
                trigger_dims: [0.0, 0.0, 0.0],
                waveform: 0,
                state_flags: 0,
            }],
        };
        // obj1 is not fired by start in this fixture. Fire it directly.
        let mut machine = Machine::boot(data);
        machine.deliver("obj1", "Trigger");
        machine.tick(0.0, [100.0, 0.0, 0.0], &[]);
        assert!(machine.finished().is_none());
        assert_eq!(machine.line(), "The way out is locked");
        machine.deliver("outtahere", "Unlock");
        machine.tick(0.0, [100.0, 0.0, 0.0], &[]);
        assert_eq!(machine.finished(), Some(("02_Quarters", "start1")));
        assert_eq!(
            machine.take_transition(),
            Some(("02_Quarters".into(), "start1".into()))
        );
        assert!(machine.take_transition().is_none());
        assert_eq!(machine.finished(), Some(("02_Quarters", "start1")));
    }

    #[test]
    fn keyframer_reaches_a_key_and_sends_its_message() {
        let data = MissionData {
            nodes: vec![node(
                NodeClass::Trigger,
                "start",
                [0.0, 0.0, 0.0],
                false,
                vec![("firstdrop", "On")],
            )],
            markers: Vec::new(),
            keys: vec![
                MotionKey {
                    name: "a0".into(),
                    pos: [0.0, 0.0, 0.0],
                    time: 0.0,
                    target: String::new(),
                    message: String::new(),
                },
                MotionKey {
                    name: "a1".into(),
                    pos: [100.0, 0.0, 0.0],
                    time: 1.0,
                    target: "drop1".into(),
                    message: "Destroy".into(),
                },
            ],
            framers: vec![Framer {
                name: "firstdrop".into(),
                object: "drop1".into(),
                base: "a".into(),
            }],
            doors: Vec::new(),
        };
        let mut machine = Machine::boot(data);
        machine.tick(1.1, [0.0, 0.0, 0.0], &[]);
        let orders = machine.take_orders();
        assert!(matches!(&orders[..], [ActorOrder::Destroy(name)] if name == "drop1"));
        assert_eq!(machine.places()[0].1, [100.0, 0.0, 0.0]);
    }

    #[test]
    fn sphere_slides_off_a_barricade_and_a_shot_hits_it() {
        let center = Vec3::new(0.0, 0.0, 0.0);
        let half = Vec3::new(10.0, 10.0, 10.0);
        let pushed = push_sphere(Vec3::new(12.0, 0.0, 0.0), 4.0, center, half);
        assert!(pushed.x > 12.0);
        let miss = push_sphere(Vec3::new(40.0, 0.0, 0.0), 4.0, center, half);
        assert_eq!(miss.x, 40.0);
        let hit = ray_aabb(Vec3::new(-30.0, 0.0, 0.0), Vec3::X, center, half).unwrap();
        assert!((hit - 20.0).abs() < 1e-3);
        assert!(ray_aabb(Vec3::new(-30.0, 40.0, 0.0), Vec3::X, center, half).is_none());
        let mut door = Barricade { hp: 100.0, half };
        assert!(!harm(&mut door, 40.0));
        assert!(harm(&mut door, 60.0));
    }

    #[test]
    fn ambush_opening_follows_the_dat() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        if !path.exists() {
            return;
        }
        let data = std::fs::read(path).unwrap();
        let mission = shogo_assets::dat::read_mission(&data).unwrap();
        let mut machine = Machine::boot(mission);
        machine.tick(0.05, [3072.0, 64.0, 2688.0], &[]);
        let orders = machine.take_orders();
        assert!(orders.iter().any(|order| matches!(
            order,
            ActorOrder::Follow { name, .. } if same(name, "cristoff")
        )));
        assert!(machine.objectives().0.contains(&1040));
        machine.tick(14.0, [3072.0, 64.0, 2688.0], &[]);
        let orders = machine.take_orders();
        assert!(orders.iter().any(|order| matches!(
            order,
            ActorOrder::Follow { name, run, .. } if same(name, "dudley") && *run
        )));
        assert!(machine.finished().is_none());
    }

    #[test]
    fn a_trigger_message_opens_then_the_door_shuts_itself() {
        let mut door = SlidingDoor {
            name: "mainlift".into(),
            rest: Vec3::ZERO,
            travel: Vec3::Y * 100.0,
            speed: 100.0,
            move_delay: 0.0,
            open_wait: 0.0,
            close_wait: 0.0,
            waveform: 0,
            flags: 4,
            phase: DoorPhase::Closed,
            along: 0.0,
            wait: 0.0,
            locked: false,
            half: Vec3::new(56.0, 12.0, 56.0),
            touch_half: Vec3::splat(64.0),
            touch: Vec3::ZERO,
        };
        ask_open(&mut door);
        assert_eq!(door.phase, DoorPhase::Opening);
        step_door(&mut door, 0.5);
        assert!((door.along - 0.5).abs() < 1e-3);
        assert!((wave(0, door.along) - 0.5).abs() < 1e-3);
        step_door(&mut door, 0.5);
        assert_eq!(door.phase, DoorPhase::Open);
        step_door(&mut door, 0.016);
        assert_eq!(door.phase, DoorPhase::Closing);
        assert!((wave(2, 1.0) - 1.0).abs() < 1e-4);
        assert!(sphere_hits(Vec3::new(10.0, 0.0, 0.0), 8.0, Vec3::ZERO, Vec3::splat(16.0)));
        assert!(!sphere_hits(Vec3::new(40.0, 0.0, 0.0), 8.0, Vec3::ZERO, Vec3::splat(16.0)));
    }
}
