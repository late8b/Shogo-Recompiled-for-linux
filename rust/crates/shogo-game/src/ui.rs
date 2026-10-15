//! Flex HUD and menus. Lengths that must keep their shape use [`Val::VMin`],
//! so a 16:9, 16:10, and ultrawide window share the same bar proportions.
//! [`UiScale`] tracks the short side against 1080, which scales `Px` fonts
//! by the same factor.

use bevy::ecs::message::MessageWriter;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;

use crate::input::{Actions, Device, Glyphs, LookConfig};
use crate::player::{Fresh, Player, Sim};
use crate::targeting::Reticle;
use crate::weapons::{self, ARMOR_MAX, PLAYER_HP};

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Phase {
    #[default]
    Menu,
    Play,
    Paused,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Menu,
    Pause,
    Settings,
    Hud,
    Wheel,
}

#[derive(Resource)]
pub struct Wheel {
    pub open: bool,
    pub slot: u8,
}

impl Default for Wheel {
    fn default() -> Self {
        Self { open: false, slot: 0 }
    }
}

#[derive(Resource, Default)]
pub(crate) struct Focus {
    menu: usize,
    pause: usize,
    settings: usize,
}

impl Focus {
    fn get(&self, page: Page) -> usize {
        match page {
            Page::Menu => self.menu,
            Page::Pause => self.pause,
            Page::Settings => self.settings,
            _ => 0,
        }
    }

    fn set(&mut self, page: Page, value: usize) {
        match page {
            Page::Menu => self.menu = value,
            Page::Pause => self.pause = value,
            Page::Settings => self.settings = value,
            _ => {}
        }
    }
}

#[derive(Resource)]
pub(crate) struct SettingsBack(Phase);

impl Default for SettingsBack {
    fn default() -> Self {
        Self(Phase::Menu)
    }
}

#[derive(Component)]
pub(crate) struct Layer(Page);

#[derive(Component)]
pub(crate) struct Choice {
    page: Page,
    index: usize,
}

#[derive(Component)]
pub(crate) struct RowText {
    page: Page,
    index: usize,
}

#[derive(Component)]
pub(crate) struct BarFill(bool);

#[derive(Component)]
pub(crate) struct HudLine;

#[derive(Component)]
pub(crate) struct HintLine;

#[derive(Component)]
pub(crate) struct CrossTick;

const HOT: Color = Color::srgba(0.86, 0.40, 0.12, 0.95);
const IDLE: Color = Color::srgba(0.07, 0.08, 0.11, 0.9);
const INK: Color = Color::srgb(0.95, 0.92, 0.86);

/// Short-side scale against a 1080 reference. Px fonts follow this. Vmin
/// lengths follow the viewport directly, so the two stay in proportion.
pub fn ui_scale_factor(width: f32, height: f32) -> f32 {
    if width < 1.0 || height < 1.0 {
        return 1.0;
    }
    (width.min(height) / 1080.0).clamp(0.5, 2.5)
}

/// Health / armor bar in pixels. Both axes come from the short side.
pub fn hud_bar_px(viewport: Vec2) -> Vec2 {
    let vmin = viewport.x.min(viewport.y);
    Vec2::new(vmin * 0.22, vmin * 0.018)
}

pub fn scale_to_window(windows: Query<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    if let Ok(window) = windows.single() {
        scale.0 = ui_scale_factor(window.width(), window.height());
    }
}

pub fn spawn_ui(mut commands: Commands) {
    commands.spawn(screen(
        Page::Menu,
        "SHOGO",
        &[
            ("Deploy", 0),
            ("Settings", 1),
            ("Quit", 2),
        ],
    ));
    commands.spawn(screen(
        Page::Pause,
        "Paused",
        &[("Resume", 0), ("Settings", 1), ("Menu", 2)],
    ));
    commands.spawn(screen(
        Page::Settings,
        "Settings",
        &[
            ("Look sensitivity", 0),
            ("Aim assist", 1),
            ("Back", 2),
        ],
    ));
    commands.spawn(hud_root());
    commands.spawn(wheel_root());
}

fn screen(page: Page, title: &str, rows: &[(&str, usize)]) -> impl Bundle {
    (
        Layer(page),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Val::VMin(1.4),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.78)),
        GlobalZIndex(if page == Page::Menu { 4 } else { 6 }),
        children![
            (
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(48.0),
                    ..default()
                },
                TextColor(INK),
                Node {
                    margin: UiRect::bottom(Val::VMin(1.6)),
                    ..default()
                },
            ),
            button(page, rows[0].0, rows[0].1),
            button(page, rows[1].0, rows[1].1),
            button(page, rows[2].0, rows[2].1),
            (
                HintLine,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.72, 0.66)),
                Node {
                    margin: UiRect::top(Val::VMin(2.0)),
                    ..default()
                },
            ),
        ],
    )
}

fn button(page: Page, label: &str, index: usize) -> impl Bundle {
    (
        Choice { page, index },
        RelativeCursorPosition::default(),
        BackgroundColor(if index == 0 { HOT } else { IDLE }),
        Node {
            width: Val::VMin(32.0),
            height: Val::VMin(5.6),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        children![(
            RowText { page, index },
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(22.0),
                ..default()
            },
            TextColor(INK),
        )],
    )
}

fn hud_root() -> impl Bundle {
    (
        Layer(Page::Hud),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        GlobalZIndex(2),
        children![
            (
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::VMin(3.0),
                    bottom: Val::VMin(3.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::VMin(0.6),
                    ..default()
                },
                children![
                    bar(true, Color::srgb(0.78, 0.18, 0.14)),
                    bar(false, Color::srgb(0.28, 0.58, 0.86)),
                    (
                        HudLine,
                        Text::new("Assault Rifle"),
                        TextFont {
                            font_size: FontSize::Px(18.0),
                            ..default()
                        },
                        TextColor(INK),
                    ),
                ],
            ),
            (
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::VMin(3.0),
                    bottom: Val::VMin(3.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
                children![(
                    Text::new("30  /  90"),
                    TextFont {
                        font_size: FontSize::Px(28.0),
                        ..default()
                    },
                    TextColor(INK),
                    AmmoLine,
                )],
            ),
            (
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![(
                    CrossRoot,
                    Node {
                        width: Val::VMin(2.4),
                        height: Val::VMin(2.4),
                        ..default()
                    },
                    children![
                        tick(8.0, 46.0, 28.0, 8.0),
                        tick(64.0, 46.0, 28.0, 8.0),
                        tick(46.0, 8.0, 8.0, 28.0),
                        tick(46.0, 64.0, 8.0, 28.0),
                    ],
                )],
            ),
        ],
    )
}

#[derive(Component)]
pub(crate) struct AmmoLine;

#[derive(Component)]
pub(crate) struct CrossRoot;

fn bar(health: bool, color: Color) -> impl Bundle {
    (
        Node {
            width: Val::VMin(22.0),
            height: Val::VMin(1.8),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
        children![(
            BarFill(health),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(color),
        )],
    )
}

fn tick(left: f32, top: f32, width: f32, height: f32) -> impl Bundle {
    (
        CrossTick,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(left),
            top: Val::Percent(top),
            width: Val::Percent(width),
            height: Val::Percent(height),
            ..default()
        },
        BackgroundColor(Color::srgb(0.95, 0.95, 0.95)),
    )
}

fn wheel_root() -> impl Bundle {
    (
        Layer(Page::Wheel),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::VMin(14.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::VMin(1.2),
            ..default()
        },
        GlobalZIndex(5),
        children![wheel_card(0, "Assault Rifle"), wheel_card(1, "Pulse Rifle")],
    )
}

fn wheel_card(slot: u8, label: &str) -> impl Bundle {
    (
        WheelCard(slot),
        Node {
            width: Val::VMin(18.0),
            height: Val::VMin(7.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(IDLE),
        children![(
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            TextColor(INK),
        )],
    )
}

#[derive(Component)]
pub(crate) struct WheelCard(u8);

pub(crate) fn drive(
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    mut sim: ResMut<Sim>,
    mut fresh: ResMut<Fresh>,
    mut focus: ResMut<Focus>,
    mut back: ResMut<SettingsBack>,
    mut look: ResMut<LookConfig>,
    actions: Res<Actions>,
    wheel: Res<Wheel>,
    mut exit: MessageWriter<AppExit>,
    mut layers: Query<(&Layer, &mut Node)>,
    mut choices: Query<(&Choice, &RelativeCursorPosition, &mut BackgroundColor)>,
    mut rows: Query<(&RowText, &mut Text)>,
    mut hints: Query<&mut Text, (With<HintLine>, Without<RowText>, Without<HudLine>, Without<AmmoLine>)>,
    mut cards: Query<(&WheelCard, &mut BackgroundColor), Without<Choice>>,
) {
    let mut run = *phase.get() == Phase::Play && !wheel.open;
    let page = match *phase.get() {
        Phase::Menu => Some(Page::Menu),
        Phase::Paused => Some(Page::Pause),
        Phase::Settings => Some(Page::Settings),
        Phase::Play => None,
    };

    if let Some(page) = page {
        let count = match page {
            Page::Menu | Page::Pause | Page::Settings => 3,
            _ => 1,
        };
        if actions.ui_step.y != 0 {
            let cur = focus.get(page) as i32;
            let next_i = (cur - actions.ui_step.y).rem_euclid(count);
            focus.set(page, next_i as usize);
        }
        let mut clicked = None;
        for (choice, cursor, mut color) in &mut choices {
            if choice.page != page {
                continue;
            }
            if actions.device == Device::Keyboard && cursor.cursor_over() {
                focus.set(page, choice.index);
            }
            let hot = choice.index == focus.get(page);
            *color = BackgroundColor(if hot { HOT } else { IDLE });
            if cursor.cursor_over() && actions.pointer {
                clicked = Some(choice.index);
            }
            let _ = hot;
        }
        if clicked.is_some() || actions.confirm {
            let index = clicked.unwrap_or_else(|| focus.get(page));
            activate(page, index, &mut next, &mut fresh, &mut back, &mut look, &actions, &mut exit, *phase.get());
            if page == Page::Settings && index == 0 && actions.ui_step.x != 0 {
                // confirm on the slider does not also leave the page
            }
        }
        if page == Page::Settings {
            if focus.settings == 0 && actions.ui_step.x != 0 {
                look.sens = (look.sens + actions.ui_step.x as f32 * 0.1).clamp(0.5, 1.8);
            }
            if focus.settings == 1 && actions.ui_step.x != 0 {
                look.assist = !look.assist;
            }
            if actions.back {
                next.set(back.0);
            }
        } else if actions.back && page == Page::Pause {
            next.set(Phase::Play);
        }
    } else if actions.pause || actions.back {
        next.set(Phase::Paused);
        run = false;
    }

    if wheel.open {
        run = false;
    }
    sim.run = run;

    let show_menu = *phase.get() == Phase::Menu;
    let show_pause = *phase.get() == Phase::Paused;
    let show_settings = *phase.get() == Phase::Settings;
    let show_hud = matches!(*phase.get(), Phase::Play | Phase::Paused);
    for (layer, mut node) in &mut layers {
        let on = match layer.0 {
            Page::Menu => show_menu,
            Page::Pause => show_pause,
            Page::Settings => show_settings,
            Page::Hud => show_hud && !wheel.open,
            Page::Wheel => wheel.open,
        };
        node.display = if on { Display::Flex } else { Display::None };
    }

    let glyphs = Glyphs::for_device(actions.device);
    let hint = match *phase.get() {
        Phase::Play => format!(
            "{}   {} jump   {} fire   {} lock   {} wheel   {} aim",
            glyphs.movement, glyphs.jump, glyphs.fire, glyphs.lock, glyphs.wheel, glyphs.aim
        ),
        Phase::Settings => format!(
            "Left / right adjust    {} back",
            glyphs.back
        ),
        _ => format!("{} confirm    arrows or stick move", glyphs.confirm),
    };
    for mut text in &mut hints {
        *text = Text::new(hint.clone());
    }
    for (row, mut text) in &mut rows {
        if row.page != Page::Settings {
            continue;
        }
        let label = match row.index {
            0 => format!("Look sensitivity   {:.1}", look.sens),
            1 => format!("Aim assist   {}", if look.assist { "On" } else { "Off" }),
            _ => "Back".to_string(),
        };
        *text = Text::new(label);
    }
    for (card, mut color) in &mut cards {
        *color = BackgroundColor(if card.0 == wheel.slot { HOT } else { IDLE });
    }
}

fn activate(
    page: Page,
    index: usize,
    next: &mut NextState<Phase>,
    fresh: &mut Fresh,
    back: &mut SettingsBack,
    look: &mut LookConfig,
    actions: &Actions,
    exit: &mut MessageWriter<AppExit>,
    current: Phase,
) {
    match (page, index) {
        (Page::Menu, 0) => {
            fresh.0 = true;
            next.set(Phase::Play);
        }
        (Page::Menu, 1) => {
            back.0 = Phase::Menu;
            next.set(Phase::Settings);
        }
        (Page::Menu, 2) => {
            exit.write(AppExit::Success);
        }
        (Page::Pause, 0) => next.set(Phase::Play),
        (Page::Pause, 1) => {
            back.0 = Phase::Paused;
            next.set(Phase::Settings);
        }
        (Page::Pause, 2) => next.set(Phase::Menu),
        (Page::Settings, 1) if actions.confirm => look.assist = !look.assist,
        (Page::Settings, 2) => next.set(back.0),
        _ => {
            let _ = current;
        }
    }
}

pub(crate) fn hud(
    phase: Res<State<Phase>>,
    reticle: Res<Reticle>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scale: Res<UiScale>,
    players: Query<&Player>,
    mut fills: Query<(&BarFill, &mut Node), Without<Layer>>,
    mut lines: Query<&mut Text, With<HudLine>>,
    mut ammo: Query<&mut Text, (With<AmmoLine>, Without<HudLine>)>,
    mut ticks: Query<&mut BackgroundColor, With<CrossTick>>,
    mut cross: Query<&mut Node, (With<CrossRoot>, Without<BarFill>, Without<Layer>)>,
) {
    if !matches!(*phase.get(), Phase::Play | Phase::Paused) {
        return;
    }
    let Ok(player) = players.single() else {
        return;
    };
    let health = (player.health / PLAYER_HP).clamp(0.0, 1.0) * 100.0;
    let armor = (player.armor / ARMOR_MAX).clamp(0.0, 1.0) * 100.0;
    for (fill, mut node) in &mut fills {
        node.width = Val::Percent(if fill.0 { health } else { armor });
    }
    let kind = weapons::weapon(player.weapon);
    let body = if player.want_mca { "MCA" } else { "On foot" };
    let status = if player.health <= 0.0 {
        "Down".to_string()
    } else if reticle.locked {
        format!("{body}   {}   LOCK", kind.name)
    } else {
        format!("{body}   {}", kind.name)
    };
    for mut text in &mut lines {
        *text = Text::new(status.clone());
    }
    let slot = player.weapon as usize;
    let mag = format!("{:02}   /   {:02}", player.ammo[slot], player.reserve[slot]);
    for mut text in &mut ammo {
        *text = Text::new(mag.clone());
    }
    let ink = if reticle.locked {
        Color::srgb(0.95, 0.62, 0.18)
    } else {
        Color::srgb(0.95, 0.95, 0.95)
    };
    for mut color in &mut ticks {
        *color = BackgroundColor(ink);
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let ui = scale.0.max(0.25);
    let px = reticle.ndc.x * window.width() * 0.5 / ui;
    let py = -reticle.ndc.y * window.height() * 0.5 / ui;
    for mut node in &mut cross {
        node.margin.left = Val::Px(px);
        node.margin.right = Val::Px(-px);
        node.margin.top = Val::Px(py);
        node.margin.bottom = Val::Px(-py);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_proportions_hold_across_aspect_ratios() {
        let wide = hud_bar_px(Vec2::new(1920.0, 1080.0));
        let ultra = hud_bar_px(Vec2::new(3440.0, 1440.0));
        let sixteen_ten = hud_bar_px(Vec2::new(1680.0, 1050.0));
        let ratio = |v: Vec2| v.x / v.y;
        assert!((ratio(wide) - ratio(ultra)).abs() < 0.02);
        assert!((ratio(wide) - ratio(sixteen_ten)).abs() < 0.02);
        assert!((ui_scale_factor(1920.0, 1080.0) - 1.0).abs() < 1e-3);
        assert!((ui_scale_factor(2560.0, 1080.0) - 1.0).abs() < 1e-3);
        assert!((ui_scale_factor(3840.0, 2160.0) - 2.0).abs() < 1e-3);
        assert!((ui_scale_factor(400.0, 300.0) - 0.5).abs() < 1e-3);
        assert!(ui_scale_factor(3440.0, 1440.0) > 1.3);
        assert!(ui_scale_factor(1280.0, 720.0) < 0.7);
    }
}
