//! Screen-space mission waypoint.
//!
//! Objective ids come from [`crate::script::ObjectiveState`]. The marker target
//! is the player exit or touch trigger that the mission graph uses to complete
//! that id. LithTech world units are inches (a door shell is 96 units tall),
//! so the counter converts to meters.

use bevy::prelude::*;
use bevy::text::{FontSize, Justify, TextLayout};
use bevy::ui::FocusPolicy;
use shogo_assets::dat::{MissionData, NodeClass};

use crate::script::{ObjectiveState, TriggerVolume};
use crate::{LevelEntity, Player};

/// Half the on-screen widget, plus a few pixels, so the icon and the meter
/// readout stay inside the window.
const BOX_W: f32 = 88.0;
const BOX_H: f32 = 48.0;
const METERS_PER_UNIT: f32 = 0.0254;

const AMBER: Color = Color::srgb(1.0, 0.68, 0.16);
const AMBER_EDGE: Color = Color::srgb(1.0, 0.9, 0.62);
const READOUT: Color = Color::srgb(0.95, 0.97, 1.0);

/// One cue for the current objective. It sits on the world entity it points at.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct MissionMarker {
    pub target: Entity,
    pub active: bool,
}

#[derive(Component)]
pub(crate) struct WaypointRoot;

#[derive(Component)]
pub(crate) struct WaypointDiamond;

#[derive(Component)]
pub(crate) struct WaypointChevron;

#[derive(Component)]
pub(crate) struct WaypointRange;

#[derive(Clone, Debug)]
struct Snap {
    name: String,
    class: NodeClass,
    pos: [f32; 3],
    player: bool,
    touch: bool,
    messages: Vec<(String, String)>,
    complete: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq)]
struct Pick {
    name: String,
    class: NodeClass,
    pos: [f32; 3],
}

impl Pick {
    fn matches(&self, volume: &TriggerVolume) -> bool {
        volume.class == self.class && same(&volume.name, &self.name)
    }
}

/// Static view of the mission graph, built once when the world is placed.
#[derive(Resource, Debug)]
pub(crate) struct Guide {
    nodes: Vec<Snap>,
}

impl Guide {
    pub(crate) fn compile(data: &MissionData) -> Self {
        let nodes = data
            .nodes
            .iter()
            .map(|node| Snap {
                name: node.name.clone(),
                class: node.class,
                pos: node.pos,
                player: node.player,
                touch: node.touch,
                messages: node
                    .messages
                    .iter()
                    .map(|message| (message.target.clone(), message.body.clone()))
                    .collect(),
                complete: id_list(&node.complete_objectives),
            })
            .collect();
        Self { nodes }
    }

    fn pick(&self, state: &ObjectiveState) -> Option<Pick> {
        self.pick_ids(&state.active, &state.done)
    }

    /// First active objective that is not finished, then the volume the player
    /// has to reach for it to complete. Exits win over plain touch triggers.
    /// Objective and transmission nodes are logic, not destinations.
    fn pick_ids(&self, active: &[i32], done: &[i32]) -> Option<Pick> {
        let current = active.iter().copied().find(|id| !done.contains(id))?;
        let completers = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.complete.contains(&current))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if completers.is_empty() {
            return None;
        }
        let mut seen = vec![false; self.nodes.len()];
        let mut frontier = completers;
        for index in &frontier {
            seen[*index] = true;
        }
        for _ in 0..8 {
            if let Some(index) = choose_walkable(&self.nodes, &frontier) {
                let node = &self.nodes[index];
                return Some(Pick {
                    name: node.name.clone(),
                    class: node.class,
                    pos: node.pos,
                });
            }
            let mut next = Vec::new();
            for index in &frontier {
                let name = self.nodes[*index].name.clone();
                for (other, node) in self.nodes.iter().enumerate() {
                    if seen[other] {
                        continue;
                    }
                    let fires = node.messages.iter().any(|(target, body)| {
                        is_trigger(body) && same(target, &name)
                    });
                    if fires {
                        seen[other] = true;
                        next.push(other);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        None
    }
}

/// Exits fire from a player overlap even when `TriggerTouch` is off. Ambush's
/// `outtahere` is that case. Touch triggers still require both flags.
fn is_walkable(node: &Snap) -> bool {
    match node.class {
        NodeClass::Exit => node.player,
        NodeClass::Trigger => node.player && node.touch,
        NodeClass::Objective | NodeClass::Transmission => false,
    }
}

fn choose_walkable(nodes: &[Snap], ids: &[usize]) -> Option<usize> {
    let mut touch = None;
    for &index in ids {
        if !is_walkable(&nodes[index]) {
            continue;
        }
        if nodes[index].class == NodeClass::Exit {
            return Some(index);
        }
        if touch.is_none() {
            touch = Some(index);
        }
    }
    touch
}

/// Move the single marker onto the volume for the current objective.
/// An objective with no walkable target deactivates it.
pub(crate) fn retarget(
    commands: &mut Commands,
    guide: &Guide,
    objectives: &ObjectiveState,
    volumes: &Query<(Entity, &mut TriggerVolume, &Transform)>,
    markers: &mut Query<(Entity, &mut MissionMarker)>,
) {
    let chosen = guide.pick(objectives).and_then(|pick| {
        let mut found = None;
        for (ent, volume, _) in volumes {
            if pick.matches(volume) {
                found = Some(ent);
                break;
            }
        }
        found
    });
    let marked = markers.iter().map(|(ent, _)| ent).collect::<Vec<_>>();
    let mut sitting = false;
    for ent in marked {
        let Ok((_, mut marker)) = markers.get_mut(ent) else {
            continue;
        };
        if Some(ent) == chosen {
            marker.target = ent;
            marker.active = true;
            sitting = true;
        } else if chosen.is_none() {
            marker.active = false;
        } else {
            marker.active = false;
            commands.entity(ent).remove::<MissionMarker>();
        }
    }
    if let Some(ent) = chosen {
        if !sitting {
            commands.entity(ent).insert(MissionMarker {
                target: ent,
                active: true,
            });
        }
    }
}

struct Frame {
    center: Vec2,
    clamped: bool,
    heading: f32,
    meters: String,
}

struct ScreenCue {
    center: Vec2,
    clamped: bool,
    /// Clockwise radians from screen-up. Used by the off-screen chevron.
    heading: f32,
}

/// `projected` is `Camera::world_to_viewport` when that call succeeds.
/// `None` means the target is behind the near plane (the method returns
/// [`bevy::camera::ViewportConversionError::PastNearPlane`]) or the projection
/// was otherwise unusable. `view_right` / `view_up` are the target offset in
/// camera axes and aim the edge pin in that case.
fn place_cue(
    projected: Option<Vec2>,
    view_right: f32,
    view_up: f32,
    viewport: Rect,
    margin: f32,
) -> ScreenCue {
    let size = viewport.size();
    let fit = size.min_element() * 0.5 - 1.0;
    if fit <= 0.0 || size.x < 2.0 || size.y < 2.0 {
        return ScreenCue {
            center: viewport.center(),
            clamped: true,
            heading: 0.0,
        };
    }
    let margin = margin.min(fit).max(0.0);
    let min = viewport.min + Vec2::splat(margin);
    let max = viewport.max - Vec2::splat(margin);
    let origin = viewport.center().clamp(min, max);
    if let Some(point) = projected {
        if point.cmpge(min).all() && point.cmple(max).all() {
            return ScreenCue {
                center: point,
                clamped: false,
                heading: 0.0,
            };
        }
        let center = ray_box(origin, point, min, max);
        return ScreenCue {
            center,
            clamped: true,
            heading: heading_of(center - origin),
        };
    }
    let mut dir = Vec2::new(view_right, -view_up);
    if dir.length_squared() < 1.0 {
        // Straight behind the camera: pin to the bottom edge.
        dir = Vec2::new(0.0, 1.0);
    }
    let dir = dir.normalize() * (size.x + size.y);
    let center = ray_box(origin, origin + dir, min, max);
    ScreenCue {
        center,
        clamped: true,
        heading: heading_of(center - origin),
    }
}

fn heading_of(dir: Vec2) -> f32 {
    if dir.length_squared() < 1e-6 {
        0.0
    } else {
        dir.x.atan2(-dir.y)
    }
}

/// First hit of the ray from `origin` through `toward` on the inset box.
/// A `toward` inside the box is left where it is.
fn ray_box(origin: Vec2, toward: Vec2, min: Vec2, max: Vec2) -> Vec2 {
    if min.x > max.x || min.y > max.y {
        return origin;
    }
    let dir = toward - origin;
    if dir.length_squared() < 1e-8 {
        return origin.clamp(min, max);
    }
    let mut best = f32::INFINITY;
    if dir.x.abs() > 1e-8 {
        let edge = if dir.x > 0.0 { max.x } else { min.x };
        let t = (edge - origin.x) / dir.x;
        if t > 0.0 {
            let y = origin.y + t * dir.y;
            if (min.y..=max.y).contains(&y) {
                best = best.min(t);
            }
        }
    }
    if dir.y.abs() > 1e-8 {
        let edge = if dir.y > 0.0 { max.y } else { min.y };
        let t = (edge - origin.y) / dir.y;
        if t > 0.0 {
            let x = origin.x + t * dir.x;
            if (min.x..=max.x).contains(&x) {
                best = best.min(t);
            }
        }
    }
    if !best.is_finite() {
        return origin.clamp(min, max);
    }
    (origin + dir * best.min(1.0)).clamp(min, max)
}

fn edge_inset(viewport: Rect) -> f32 {
    let want = BOX_W.max(BOX_H) * 0.5 + 6.0;
    let fit = viewport.size().min_element() * 0.5 - 1.0;
    if fit <= 4.0 {
        fit.max(0.0)
    } else {
        want.min(fit)
    }
}

pub(crate) fn format_meters(units: f32) -> String {
    let meters = units.abs() * METERS_PER_UNIT;
    if meters >= 10.0 {
        format!("{meters:.0} m")
    } else {
        format!("{meters:.1} m")
    }
}

/// Project the active marker every frame. Viewport pixels come from the player
/// camera, so the cue tracks the window size instead of a fixed layout.
pub(crate) fn project(
    mut commands: Commands,
    guide: Option<Res<Guide>>,
    objectives: Option<Res<ObjectiveState>>,
    volumes: Query<(Entity, &TriggerVolume, &Transform)>,
    markers: Query<&MissionMarker>,
    cameras: Query<(&Camera, &Transform), With<Player>>,
    mut roots: Query<
        (&mut Node, &mut Visibility),
        (
            With<WaypointRoot>,
            Without<WaypointDiamond>,
            Without<WaypointChevron>,
        ),
    >,
    mut diamonds: Query<
        &mut Node,
        (
            With<WaypointDiamond>,
            Without<WaypointRoot>,
            Without<WaypointChevron>,
        ),
    >,
    mut chevrons: Query<
        (&mut Node, &mut UiTransform),
        (
            With<WaypointChevron>,
            Without<WaypointRoot>,
            Without<WaypointDiamond>,
        ),
    >,
    mut ranges: Query<&mut Text, With<WaypointRange>>,
) {
    let Some(frame) = compose(
        guide.as_deref(),
        objectives.as_deref(),
        &volumes,
        &markers,
        &cameras,
    ) else {
        conceal(&mut roots);
        return;
    };
    if roots.is_empty() {
        spawn_hud(&mut commands, &frame);
        return;
    }
    if let Ok((mut node, mut vis)) = roots.single_mut() {
        node.left = Val::Px(frame.center.x - BOX_W * 0.5);
        node.top = Val::Px(frame.center.y - BOX_H * 0.5);
        *vis = Visibility::Visible;
    }
    let show_diamond = if frame.clamped {
        Display::None
    } else {
        Display::Flex
    };
    let show_chevron = if frame.clamped {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut node) = diamonds.single_mut() {
        node.display = show_diamond;
    }
    if let Ok((mut node, mut xform)) = chevrons.single_mut() {
        node.display = show_chevron;
        *xform = UiTransform::from_rotation(Rot2::radians(frame.heading));
    }
    if let Ok(mut text) = ranges.single_mut() {
        *text = Text::new(frame.meters);
    }
}

fn conceal(
    roots: &mut Query<
        (&mut Node, &mut Visibility),
        (
            With<WaypointRoot>,
            Without<WaypointDiamond>,
            Without<WaypointChevron>,
        ),
    >,
) {
    for (_, mut vis) in roots {
        *vis = Visibility::Hidden;
    }
}

fn compose(
    guide: Option<&Guide>,
    objectives: Option<&ObjectiveState>,
    volumes: &Query<(Entity, &TriggerVolume, &Transform)>,
    markers: &Query<&MissionMarker>,
    cameras: &Query<(&Camera, &Transform), With<Player>>,
) -> Option<Frame> {
    let world = target_world(guide, objectives, volumes, markers)?;
    let (camera, cam_tf) = cameras.single().ok()?;
    let viewport = camera.logical_viewport_rect()?;
    let delta = world - cam_tf.translation;
    let ahead = cam_tf.forward().dot(delta);
    // The stored `GlobalTransform` is still last frame's pose. The player
    // camera has no parent, so the local transform is the world pose.
    let pose = GlobalTransform::from(*cam_tf);
    let projected = if ahead > 0.0 {
        camera.world_to_viewport(&pose, world).ok()
    } else {
        None
    };
    let cue = place_cue(
        projected,
        cam_tf.right().dot(delta),
        cam_tf.up().dot(delta),
        viewport,
        edge_inset(viewport),
    );
    Some(Frame {
        center: cue.center,
        clamped: cue.clamped,
        heading: cue.heading,
        meters: format_meters(delta.length()),
    })
}

fn target_world(
    guide: Option<&Guide>,
    objectives: Option<&ObjectiveState>,
    volumes: &Query<(Entity, &TriggerVolume, &Transform)>,
    markers: &Query<&MissionMarker>,
) -> Option<Vec3> {
    let marked = markers.iter().find(|marker| marker.active).map(|marker| marker.target);
    if let Some(ent) = marked {
        if let Ok((_, _, tf)) = volumes.get(ent) {
            return Some(tf.translation);
        }
    }
    let guide = guide?;
    let objectives = objectives?;
    let pick = guide.pick(objectives)?;
    for (_, volume, tf) in volumes {
        if pick.matches(volume) {
            return Some(tf.translation);
        }
    }
    None
}

fn spawn_hud(commands: &mut Commands, frame: &Frame) {
    let (diamond, chevron) = if frame.clamped {
        (Display::None, Display::Flex)
    } else {
        (Display::Flex, Display::None)
    };
    commands
        .spawn((
            LevelEntity,
            WaypointRoot,
            root_node(frame.center),
            Visibility::Visible,
            GlobalZIndex(12),
            FocusPolicy::Pass,
        ))
        .with_children(|parent| {
            parent.spawn((
                WaypointDiamond,
                Node {
                    display: diamond,
                    width: Val::Px(12.0),
                    height: Val::Px(12.0),
                    border: UiRect::all(Val::Px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                UiTransform::from_rotation(Rot2::radians(std::f32::consts::FRAC_PI_4)),
                BackgroundColor(AMBER),
                BorderColor::all(AMBER_EDGE),
            ));
            parent.spawn((
                WaypointChevron,
                Node {
                    display: chevron,
                    width: Val::Px(22.0),
                    height: Val::Px(22.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    ..default()
                },
                UiTransform::from_rotation(Rot2::radians(frame.heading)),
                Text::new("▲"),
                TextFont {
                    font_size: FontSize::Px(18.0),
                    ..default()
                },
                TextColor(AMBER),
                TextLayout::justify(Justify::Center),
            ));
            parent.spawn((
                WaypointRange,
                Text::new(frame.meters.clone()),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(READOUT),
                TextLayout::justify(Justify::Center),
            ));
        });
}

fn root_node(center: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(center.x - BOX_W * 0.5),
        top: Val::Px(center.y - BOX_H * 0.5),
        width: Val::Px(BOX_W),
        height: Val::Px(BOX_H),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(1.0),
        ..default()
    }
}

fn id_list(text: &str) -> Vec<i32> {
    text.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter_map(|part| part.trim().parse().ok())
        .collect()
}

fn is_trigger(body: &str) -> bool {
    body.trim().eq_ignore_ascii_case("trigger")
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogo_assets::dat::{MissionMessage, MissionNode};

    fn node(
        class: NodeClass,
        name: &str,
        pos: [f32; 3],
        player: bool,
        touch: bool,
        messages: Vec<(&str, &str)>,
        complete: &str,
    ) -> MissionNode {
        MissionNode {
            class,
            name: name.to_string(),
            pos,
            dims: [32.0, 32.0, 32.0],
            send_delay: 0.0,
            touch,
            player,
            ai: false,
            ai_name: String::new(),
            locked: false,
            messages: messages
                .into_iter()
                .map(|(target, body)| MissionMessage {
                    target: target.to_string(),
                    body: body.to_string(),
                })
                .collect(),
            destination: String::new(),
            start_name: String::new(),
            add_objectives: String::new(),
            remove_objectives: String::new(),
            complete_objectives: complete.to_string(),
            string_id: 0,
        }
    }

    fn view(width: f32, height: f32) -> Rect {
        Rect::from_corners(Vec2::ZERO, Vec2::new(width, height))
    }

    #[test]
    fn on_screen_point_stays_put_at_two_aspects() {
        let wide = view(1280.0, 720.0);
        let tall = view(600.0, 900.0);
        let margin = 40.0;
        let wide_cue = place_cue(Some(Vec2::new(640.0, 360.0)), 80.0, 0.0, wide, margin);
        assert!(!wide_cue.clamped);
        assert_eq!(wide_cue.center, Vec2::new(640.0, 360.0));
        let tall_cue = place_cue(Some(Vec2::new(300.0, 450.0)), 0.0, -20.0, tall, margin);
        assert!(!tall_cue.clamped);
        assert_eq!(tall_cue.center, Vec2::new(300.0, 450.0));
    }

    #[test]
    fn offscreen_and_behind_camera_pin_to_the_edge() {
        let viewport = view(1280.0, 720.0);
        let margin = 40.0;
        let right = place_cue(Some(Vec2::new(2000.0, 360.0)), -50.0, 0.0, viewport, margin);
        assert!(right.clamped);
        assert!((right.center.x - (1280.0 - margin)).abs() < 1e-2);
        assert!((right.center.y - 360.0).abs() < 1e-2);
        assert!((right.heading - std::f32::consts::FRAC_PI_2).abs() < 1e-2);

        let top = place_cue(Some(Vec2::new(400.0, -80.0)), 0.0, 0.0, viewport, margin);
        assert!(top.clamped);
        assert!((top.center.y - margin).abs() < 1e-2);
        // The pin follows the ray from the viewport center, so x moves
        // toward the off-screen point instead of staying on a fixed column.
        assert!(top.center.x > 400.0 && top.center.x < 640.0);
        assert!(top.heading < 0.0 && top.heading > -std::f32::consts::FRAC_PI_2);

        // In-front projection wins over the view axes, so a left-edge point
        // stays on the left even if the view-right hint points the other way.
        let left = place_cue(Some(Vec2::new(-30.0, 200.0)), 400.0, 10.0, viewport, margin);
        assert!(left.clamped);
        assert!((left.center.x - margin).abs() < 1e-2);

        let behind_right = place_cue(None, 12.0, 0.0, viewport, margin);
        assert!(behind_right.clamped);
        assert!((behind_right.center.x - (1280.0 - margin)).abs() < 1e-2);
        assert!((behind_right.center.y - 360.0).abs() < 1.0);

        let behind = place_cue(None, 0.0, 0.0, viewport, margin);
        assert!(behind.clamped);
        assert!((behind.center.y - (720.0 - margin)).abs() < 1e-2);
        assert!((behind.center.x - 640.0).abs() < 1e-2);

        let shifted = Rect::from_corners(Vec2::new(100.0, 40.0), Vec2::new(900.0, 640.0));
        let pinned = place_cue(Some(Vec2::new(2000.0, 300.0)), 0.0, 0.0, shifted, margin);
        assert!((pinned.center.x - (900.0 - margin)).abs() < 1e-2);
        assert!(pinned.center.x < 900.0 - margin + 0.1);
        assert!(pinned.center.y >= 40.0 + margin - 0.1);
    }

    #[test]
    fn lithtech_inches_read_as_meters() {
        assert_eq!(format_meters(0.0), "0.0 m");
        assert_eq!(format_meters(1.0 / METERS_PER_UNIT), "1.0 m");
        assert_eq!(format_meters(10.0 / METERS_PER_UNIT), "10 m");
        let ambush = Vec3::new(-2304.0, 160.0, 2624.0) - Vec3::new(3072.0, 64.0, 2688.0);
        let text = format_meters(ambush.length());
        assert!(text.ends_with(" m"));
        assert!(text.chars().next().unwrap().is_ascii_digit());
        let meters: f32 = text.trim_end_matches(" m").parse().unwrap();
        assert!((meters - 136.0).abs() < 2.0);
    }

    #[test]
    fn exit_beats_a_touch_trigger_for_the_same_objective() {
        let guide = Guide::compile(&MissionData {
            nodes: vec![
                node(
                    NodeClass::Objective,
                    "done",
                    [0.0, 0.0, 0.0],
                    false,
                    false,
                    vec![],
                    "7",
                ),
                node(
                    NodeClass::Trigger,
                    "side",
                    [10.0, 0.0, 0.0],
                    true,
                    true,
                    vec![("done", "Trigger")],
                    "",
                ),
                node(
                    NodeClass::Exit,
                    "gate",
                    [40.0, 2.0, 0.0],
                    true,
                    false,
                    vec![("done", "Trigger")],
                    "",
                ),
            ],
            ..default()
        });
        let pick = guide.pick_ids(&[7], &[]).expect("target");
        assert_eq!(pick.name, "gate");
        assert_eq!(pick.class, NodeClass::Exit);
        assert_eq!(pick.pos, [40.0, 2.0, 0.0]);
        assert!(guide.pick_ids(&[7], &[7]).is_none());
        assert!(guide.pick_ids(&[], &[]).is_none());
    }

    #[test]
    fn a_touch_trigger_is_the_target_when_nothing_else_completes_it() {
        let guide = Guide::compile(&MissionData {
            nodes: vec![
                node(
                    NodeClass::Objective,
                    "obj",
                    [1.0, 1.0, 1.0],
                    false,
                    false,
                    vec![],
                    "3",
                ),
                node(
                    NodeClass::Trigger,
                    "pad",
                    [8.0, 0.0, 2.0],
                    true,
                    true,
                    vec![("obj", "Trigger")],
                    "",
                ),
            ],
            ..default()
        });
        let pick = guide.pick_ids(&[3], &[]).expect("pad");
        assert_eq!(pick.name, "pad");
        assert_eq!(pick.class, NodeClass::Trigger);
        assert_eq!(pick.pos, [8.0, 0.0, 2.0]);
    }

    #[test]
    fn ambush_first_objective_marks_the_exit() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/01_AMBUSH.DAT");
        if !path.exists() {
            return;
        }
        let data = std::fs::read(path).unwrap();
        let mission = shogo_assets::dat::read_mission(&data).unwrap();
        let guide = Guide::compile(&mission);
        let mut machine = crate::script::Machine::boot(mission);
        machine.tick(0.0, [3072.0, 64.0, 2688.0], &[]);
        let (active, done) = machine.objectives();
        assert!(active.contains(&1040), "opening objective missing: {active:?}");
        let pick = guide.pick_ids(active, done).expect("ambush marker");
        assert!(same(&pick.name, "outtahere"));
        assert_eq!(pick.class, NodeClass::Exit);
        assert!((pick.pos[0] + 2304.0).abs() < 0.05);
        assert!((pick.pos[1] - 160.0).abs() < 0.05);
        assert!((pick.pos[2] - 2624.0).abs() < 0.05);
        assert!(pick.pos[0] < 0.0);
        let still = guide.pick_ids(&[1040, 1145], &[]).expect("1040 stays first");
        assert!(same(&still.name, "outtahere"));
        assert!(guide.pick_ids(&[1145], &[1040]).is_none());
    }
}
