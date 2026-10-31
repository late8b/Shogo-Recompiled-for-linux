//! Title, the Ambush bumper, pause, and one continue file.
//!
//! New Game plays the retail bumper, then drops into mission 1. Continue
//! reads the save, skips the bumper, and loads that mission. Esc pauses. F5
//! and the pause menu write the same file the title screen reads.

use std::path::{Path, PathBuf};

use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::RelativeCursorPosition;
use bevy::window::PrimaryWindow;

use crate::tune::Mixer;
use crate::{Checkpoint, LookGrabbed, Player, WorldCollide};

#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Screen {
    Menu,
    Intro,
    Mission,
}

#[derive(Resource, Default, PartialEq, Eq)]
pub struct Paused(pub bool);

#[derive(Clone)]
pub struct Save {
    pub mission: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub scale: f32,
    pub mca: bool,
    pub weapon: u8,
    pub ammo: [u16; 2],
    pub reserve: [u16; 2],
}

#[derive(Resource, Clone, Default)]
pub enum Launch {
    #[default]
    Fresh,
    Continue(Save),
}

#[derive(Component)]
pub(crate) struct FrontRoot;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    NewGame,
    Continue,
    Quit,
    Resume,
    Save,
    Title,
}

#[derive(Component)]
struct Choice {
    slot: u8,
    action: Act,
    enabled: bool,
}

#[derive(Component)]
struct Banner;

#[derive(Resource)]
struct Pick(u8);

#[derive(Resource)]
struct IntroClock(f32);

pub fn install(app: &mut App) {
    app.add_systems(OnEnter(Screen::Menu), show_menu)
        .add_systems(OnEnter(Screen::Intro), show_intro)
        .add_systems(Update, drive);
}

pub fn dismiss(mut commands: Commands, roots: Query<Entity, With<FrontRoot>>) {
    clear_front(&mut commands, &roots);
}

pub fn sim_runs(state: Res<State<Screen>>, paused: Res<Paused>) -> bool {
    *state.get() == Screen::Mission && !paused.0
}

pub fn apply_launch(
    launch: Res<Launch>,
    mut players: Query<(&mut Player, &mut Transform)>,
    mut checkpoint: Option<ResMut<Checkpoint>>,
) {
    let Launch::Continue(save) = &*launch else {
        return;
    };
    // `02_Quarters` and `02_QUARTERS.DAT` are the same mission.
    if !same_mission(&save.mission, &world_file()) {
        return;
    }
    let Ok((mut player, mut tf)) = players.single_mut() else {
        return;
    };
    player.health = save.health.max(1.0);
    player.yaw = save.yaw;
    player.pitch = save.pitch;
    player.scale = save.scale.clamp(0.0, 1.0);
    player.want_mca = save.mca;
    player.radius = crate::BodyTune::at(player.scale).radius;
    player.weapon = save.weapon.min(1);
    // A continue file with both guns empty is a dead mission: Ambush has no
    // pickups, and reload cannot move rounds that are not there. Any rounds
    // still in the file are kept.
    let (ammo, reserve) = crate::carried_rounds(save.ammo, save.reserve);
    player.ammo = ammo;
    player.reserve = reserve;
    player.vel = Vec3::ZERO;
    player.reloading = 0.0;
    tf.translation = Vec3::new(save.x, save.y, save.z);
    tf.rotation = Quat::from_euler(EulerRot::YXZ, save.yaw, save.pitch, 0.0);
    if let Some(checkpoint) = checkpoint.as_mut() {
        checkpoint.pos = tf.translation;
        checkpoint.yaw = save.yaw;
    }
    info!("continued at ({:.0},{:.0},{:.0})", save.x, save.y, save.z);
}

pub fn mission_started(players: Query<(&Player, &Transform)>, tune: Res<Mixer>) {
    if let Ok((player, tf)) = players.single() {
        let save = capture(player, tf);
        if write_save(&save) {
            info!("continue file {}", continue_path().display());
        }
    }
    tune.bed(
        Some(&sound("AMBIENT/MUSIC1.WAV")),
        &sound("AMBIENT/WIND3.WAV"),
    );
}

fn show_menu(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<FrontRoot>>,
    tune: Res<Mixer>,
) {
    clear_front(&mut commands, &old);
    commands.insert_resource(Paused(false));
    commands.insert_resource(Pick(0));
    tune.bed(None, &sound("INTERFACE/LOOP3.WAV"));
    spawn_plate(&mut commands, &mut images, "INTERFACE/MENU/BACKGRND.PCX");
    spawn_caption(&mut commands, 36.0, "SHOGO", 48.0);
    spawn_caption(&mut commands, 92.0, "Mobile Armor Division", 22.0);
    let continue_on = read_save().is_some();
    spawn_button(&mut commands, 0, Act::NewGame, "New Game", true, 200.0);
    spawn_button(&mut commands, 1, Act::Continue, "Continue", continue_on, 268.0);
    spawn_button(&mut commands, 2, Act::Quit, "Quit", true, 336.0);
    spawn_caption(
        &mut commands,
        640.0,
        "Up / Down, Enter. Esc pauses once you are in.",
        16.0,
    );
}

fn show_intro(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<FrontRoot>>,
    tune: Res<Mixer>,
) {
    clear_front(&mut commands, &old);
    commands.insert_resource(IntroClock(0.0));
    commands.insert_resource(Pick(0));
    tune.bed(
        Some(&sound("INTERFACE/BUMPER.WAV")),
        &sound("INTERFACE/LOOP3.WAV"),
    );
    spawn_plate(&mut commands, &mut images, "INTERFACE/BUMPERS/AMBUSH.PCX");
    commands.spawn((
        FrontRoot,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(28.0),
            top: Val::Px(16.0),
            ..default()
        },
        Text::new("Enter"),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::srgb(0.96, 0.93, 0.86)),
    ));
}

#[derive(SystemParam)]
struct FrontIo<'w> {
    state: Res<'w, State<Screen>>,
    next: ResMut<'w, NextState<Screen>>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    time: Res<'w, Time>,
    tune: Res<'w, Mixer>,
    paused: ResMut<'w, Paused>,
    launch: ResMut<'w, Launch>,
    grab: Option<ResMut<'w, LookGrabbed>>,
    pick: Option<ResMut<'w, Pick>>,
    clock: Option<ResMut<'w, IntroClock>>,
    exit: MessageWriter<'w, AppExit>,
}

fn drive(
    mut io: FrontIo,
    mut commands: Commands,
    mut choices: Query<(&Choice, &RelativeCursorPosition, &mut BackgroundColor)>,
    mut banners: Query<&mut Text, With<Banner>>,
    players: Query<(&Player, &Transform)>,
    roots: Query<Entity, With<FrontRoot>>,
    world: Query<Entity, (With<Transform>, Without<ChildOf>, Without<PrimaryWindow>)>,
) {
    if *io.state.get() == Screen::Intro {
        if let Some(clock) = io.clock.as_mut() {
            clock.0 += io.time.delta_secs();
            let skip = clock.0 > 0.45
                && (io.keys.just_pressed(KeyCode::Enter)
                    || io.keys.just_pressed(KeyCode::Space)
                    || io.keys.just_pressed(KeyCode::Escape)
                    || io.mouse.just_pressed(MouseButton::Left));
            if skip || clock.0 > 7.0 {
                io.next.set(Screen::Mission);
            }
        }
        return;
    }

    if *io.state.get() == Screen::Mission {
        if io.keys.just_pressed(KeyCode::Escape) {
            if io.paused.0 {
                resume(&mut commands, &roots, &mut io.paused, io.grab.as_deref_mut());
            } else {
                open_pause(&mut commands, &mut io.paused, io.grab.as_deref_mut());
            }
            return;
        }
        if !io.paused.0 {
            if io.keys.just_pressed(KeyCode::F5) {
                store_player(&players, &mut banners);
            }
            return;
        }
    }

    let Some(mut selected) = io.pick.as_mut().map(|pick| pick.0) else {
        return;
    };
    let mut slots = Vec::new();
    let mut hover = None;
    for (choice, cursor, _) in choices.iter() {
        if choice.enabled {
            slots.push(choice.slot);
        }
        if cursor.cursor_over && choice.enabled {
            hover = Some(choice.slot);
        }
    }
    slots.sort_unstable();
    slots.dedup();
    if let Some(slot) = hover {
        selected = slot;
    }
    if !slots.is_empty()
        && (io.keys.just_pressed(KeyCode::ArrowDown)
            || io.keys.just_pressed(KeyCode::KeyS)
            || io.keys.just_pressed(KeyCode::ArrowUp)
            || io.keys.just_pressed(KeyCode::KeyW))
    {
        let dir = if io.keys.just_pressed(KeyCode::ArrowUp) || io.keys.just_pressed(KeyCode::KeyW) {
            -1
        } else {
            1
        };
        let pos = slots.iter().position(|slot| *slot == selected).unwrap_or(0);
        let n = slots.len() as i32;
        let next_i = (pos as i32 + dir).rem_euclid(n) as usize;
        selected = slots[next_i];
    }

    let mut chosen = None;
    if io.keys.just_pressed(KeyCode::Enter) || io.keys.just_pressed(KeyCode::Space) {
        chosen = slots.iter().find(|slot| **slot == selected).copied();
    }
    if io.mouse.just_pressed(MouseButton::Left) {
        for (choice, cursor, _) in choices.iter() {
            if cursor.cursor_over && choice.enabled {
                chosen = Some(choice.slot);
            }
        }
    }
    for (choice, _, mut bg) in &mut choices {
        *bg = BackgroundColor(plate(choice.slot == selected, choice.enabled));
    }
    if let Some(pick) = io.pick.as_mut() {
        pick.0 = selected;
    }
    let Some(slot) = chosen else {
        return;
    };
    let action = choices.iter().find(|(c, _, _)| c.slot == slot).map(|(c, _, _)| c.action);
    let Some(action) = action else {
        return;
    };
    io.tune.sting(&sound("INTERFACE/SELECT.WAV"));
    match action {
        Act::NewGame => {
            *io.launch = Launch::Fresh;
            io.next.set(Screen::Intro);
        }
        Act::Continue => {
            // Parsed before Mission enter, so the loader can boot this world.
            // A missing or unreadable file still enters Ambush, and skips the bumper.
            *io.launch = match read_save() {
                Some(save) => Launch::Continue(save),
                None => Launch::Fresh,
            };
            io.next.set(Screen::Mission);
        }
        Act::Quit => {
            io.exit.write(AppExit::Success);
        }
        Act::Resume => resume(&mut commands, &roots, &mut io.paused, io.grab.as_deref_mut()),
        Act::Save => store_player(&players, &mut banners),
        Act::Title => {
            store_player(&players, &mut banners);
            io.paused.0 = false;
            leave_mission(&mut commands, &world);
            io.next.set(Screen::Menu);
        }
    }
}

fn open_pause(
    commands: &mut Commands,
    paused: &mut Paused,
    grab: Option<&mut LookGrabbed>,
) {
    paused.0 = true;
    if let Some(grab) = grab {
        grab.0 = false;
    }
    commands.insert_resource(Pick(0));
    commands.spawn((
        FrontRoot,
        Camera2d,
        IsDefaultUiCamera,
        Camera {
            order: 10,
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
    commands.spawn((
        FrontRoot,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.62)),
    ));
    spawn_button(commands, 0, Act::Resume, "Resume", true, 200.0);
    spawn_button(commands, 1, Act::Save, "Save", true, 268.0);
    spawn_button(commands, 2, Act::Title, "Main menu", true, 336.0);
    commands.spawn((
        FrontRoot,
        Banner,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(72.0),
            top: Val::Px(410.0),
            width: Val::Px(280.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Text::new("Esc resumes   F5 quicksaves"),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.88, 0.8)),
    ));
}

fn resume(
    commands: &mut Commands,
    roots: &Query<Entity, With<FrontRoot>>,
    paused: &mut Paused,
    grab: Option<&mut LookGrabbed>,
) {
    paused.0 = false;
    if let Some(grab) = grab {
        grab.0 = std::env::var("SHOGO_GRAB").ok().as_deref() != Some("0");
    }
    clear_front(commands, roots);
}

fn store_player(
    players: &Query<(&Player, &Transform)>,
    banners: &mut Query<&mut Text, With<Banner>>,
) {
    let Ok((player, tf)) = players.single() else {
        return;
    };
    if write_save(&capture(player, tf)) {
        info!("saved {}", continue_path().display());
        for mut text in banners.iter_mut() {
            text.0 = "Saved".into();
        }
    }
}

fn leave_mission(
    commands: &mut Commands,
    world: &Query<Entity, (With<Transform>, Without<ChildOf>, Without<PrimaryWindow>)>,
) {
    for entity in world.iter() {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<WorldCollide>();
    commands.remove_resource::<Checkpoint>();
    commands.remove_resource::<crate::nav::WalkNav>();
    commands.remove_resource::<crate::combat::Waves>();
    commands.remove_resource::<crate::combat::Roster>();
    commands.remove_resource::<crate::combat::Mission>();
    commands.remove_resource::<crate::combat::MuzzleBank>();
    commands.remove_resource::<crate::combat::ShotRng>();
    commands.remove_resource::<LookGrabbed>();
    crate::forget_world();
}

fn clear_front(commands: &mut Commands, roots: &Query<Entity, With<FrontRoot>>) {
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
}

fn spawn_plate(commands: &mut Commands, images: &mut Assets<Image>, rel: &str) {
    commands.spawn((
        FrontRoot,
        Camera2d,
        IsDefaultUiCamera,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            ..default()
        },
    ));
    let image = pcx_image(rel).map(|image| images.add(image));
    if let Some(image) = image {
        commands.spawn((
            FrontRoot,
            ImageNode {
                image,
                ..default()
            },
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ));
    }
}

fn spawn_caption(commands: &mut Commands, top: f32, label: &str, size: f32) {
    commands.spawn((
        FrontRoot,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(48.0),
            top: Val::Px(top),
            ..default()
        },
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::srgb(0.96, 0.93, 0.86)),
    ));
}

fn spawn_button(commands: &mut Commands, slot: u8, action: Act, label: &str, enabled: bool, top: f32) {
    let ink = if enabled {
        Color::srgb(0.96, 0.94, 0.9)
    } else {
        Color::srgb(0.45, 0.45, 0.45)
    };
    commands
        .spawn((
            FrontRoot,
            Choice {
                slot,
                action,
                enabled,
            },
            RelativeCursorPosition::default(),
            BackgroundColor(plate(slot == 0, enabled)),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(72.0),
                top: Val::Px(top),
                width: Val::Px(280.0),
                height: Val::Px(54.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(26.0),
                    ..default()
                },
                TextColor(ink),
            ));
        });
}

fn plate(on: bool, enabled: bool) -> Color {
    if !enabled {
        Color::srgba(0.08, 0.08, 0.08, 0.4)
    } else if on {
        Color::srgba(0.72, 0.14, 0.1, 0.94)
    } else {
        Color::srgba(0.05, 0.06, 0.09, 0.84)
    }
}

fn capture(player: &Player, tf: &Transform) -> Save {
    Save {
        mission: world_file(),
        x: tf.translation.x,
        y: tf.translation.y,
        z: tf.translation.z,
        yaw: player.yaw,
        pitch: player.pitch,
        health: player.health,
        scale: player.scale,
        mca: player.want_mca,
        weapon: player.weapon,
        ammo: player.ammo,
        reserve: player.reserve,
    }
}

fn world_file() -> String {
    crate::world_path()
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "01_AMBUSH.DAT".into())
}

fn same_mission(saved: &str, loaded: &str) -> bool {
    let saved = crate::dat_stem(saved);
    let loaded = crate::dat_stem(loaded);
    !saved.is_empty() && saved.eq_ignore_ascii_case(loaded)
}

pub fn continue_path() -> PathBuf {
    if let Ok(path) = std::env::var("SHOGO_SAVE") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    PathBuf::from(base).join("shogo-mad/continue.sav")
}

fn write_save(save: &Save) -> bool {
    write_at(&continue_path(), save)
}

fn write_at(path: &Path, save: &Save) -> bool {
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    std::fs::write(path, encode(save)).is_ok()
}

fn read_save() -> Option<Save> {
    read_at(&continue_path())
}

fn read_at(path: &Path) -> Option<Save> {
    let text = std::fs::read_to_string(path).ok()?;
    decode(&text)
}

pub fn encode(save: &Save) -> String {
    format!(
        "v1\nmission {}\nx {}\ny {}\nz {}\nyaw {}\npitch {}\nhealth {}\nscale {}\nmca {}\nweapon {}\nammo {} {}\nreserve {} {}\n",
        save.mission,
        save.x,
        save.y,
        save.z,
        save.yaw,
        save.pitch,
        save.health,
        save.scale,
        if save.mca { 1 } else { 0 },
        save.weapon,
        save.ammo[0],
        save.ammo[1],
        save.reserve[0],
        save.reserve[1],
    )
}

pub fn decode(text: &str) -> Option<Save> {
    let mut mission = None;
    let mut x = None;
    let mut y = None;
    let mut z = None;
    let mut yaw = None;
    let mut pitch = None;
    let mut health = None;
    let mut scale = None;
    let mut mca = None;
    let mut weapon = None;
    let mut ammo = None;
    let mut reserve = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        match key {
            "v1" => {}
            "mission" => mission = Some(parts.next()?.to_string()),
            "x" => x = Some(parts.next()?.parse().ok()?),
            "y" => y = Some(parts.next()?.parse().ok()?),
            "z" => z = Some(parts.next()?.parse().ok()?),
            "yaw" => yaw = Some(parts.next()?.parse().ok()?),
            "pitch" => pitch = Some(parts.next()?.parse().ok()?),
            "health" => health = Some(parts.next()?.parse().ok()?),
            "scale" => scale = Some(parts.next()?.parse().ok()?),
            "mca" => mca = Some(parts.next()? == "1"),
            "weapon" => weapon = Some(parts.next()?.parse().ok()?),
            "ammo" => {
                ammo = Some([parts.next()?.parse().ok()?, parts.next()?.parse().ok()?]);
            }
            "reserve" => {
                reserve = Some([parts.next()?.parse().ok()?, parts.next()?.parse().ok()?]);
            }
            _ => {}
        }
    }
    Some(Save {
        mission: mission?,
        x: x?,
        y: y?,
        z: z?,
        yaw: yaw?,
        pitch: pitch?,
        health: health?,
        scale: scale?,
        mca: mca?,
        weapon: weapon?,
        ammo: ammo?,
        reserve: reserve?,
    })
}

fn extracted() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../extracted")
}

fn sound(rel: &str) -> PathBuf {
    extracted().join("SOUND").join(rel)
}

fn pcx_image(rel: &str) -> Option<Image> {
    let bytes = std::fs::read(extracted().join("SHOGO").join(rel)).ok()?;
    let (w, h, rgba) = decode_pcx(&bytes)?;
    Some(crate::rgba_image(w, h, rgba))
}

pub fn decode_pcx(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < 128 || data[0] != 10 || data[3] != 8 || data[65] != 1 {
        return None;
    }
    let xmin = u16::from_le_bytes([data[4], data[5]]) as u32;
    let ymin = u16::from_le_bytes([data[6], data[7]]) as u32;
    let xmax = u16::from_le_bytes([data[8], data[9]]) as u32;
    let ymax = u16::from_le_bytes([data[10], data[11]]) as u32;
    let width = xmax.checked_sub(xmin)? + 1;
    let height = ymax.checked_sub(ymin)? + 1;
    let row = u16::from_le_bytes([data[66], data[67]]) as usize;
    let need = row.checked_mul(height as usize)?;
    let mut index = Vec::with_capacity(need);
    let mut p = 128usize;
    while index.len() < need && p < data.len() {
        let b = data[p];
        p += 1;
        if b & 0xC0 == 0xC0 {
            let n = (b & 0x3F) as usize;
            if p >= data.len() {
                break;
            }
            let v = data[p];
            p += 1;
            index.extend(std::iter::repeat_n(v, n));
        } else {
            index.push(b);
        }
    }
    if index.len() < need {
        return None;
    }
    let pal_at = data.len().checked_sub(769)?;
    if data[pal_at] != 0x0C {
        return None;
    }
    let pal = &data[pal_at + 1..pal_at + 1 + 768];
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = index[y * row + x] as usize * 3;
            rgba.extend_from_slice(&[pal[i], pal[i + 1], pal[i + 2], 255]);
        }
    }
    Some((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Save {
        Save {
            mission: "01_AMBUSH.DAT".into(),
            x: 3072.0,
            y: 32.0,
            z: 2688.0,
            yaw: 0.25,
            pitch: -0.1,
            health: 420.0,
            scale: 1.0,
            mca: true,
            weapon: 1,
            ammo: [10, 3],
            reserve: [40, 8],
        }
    }

    #[test]
    fn quarters_stem_matches_the_loaded_dat() {
        assert!(same_mission("02_Quarters", "02_QUARTERS.DAT"));
        assert!(same_mission("02_QUARTERS.DAT", "02_QUARTERS.DAT"));
        assert!(!same_mission("02_Quarters", "01_AMBUSH.DAT"));
        assert!(!same_mission("no_such_world", "01_AMBUSH.DAT"));
    }

    #[test]
    fn save_roundtrip_keeps_the_mission_and_the_body() {
        let save = sample();
        let back = decode(&encode(&save)).expect("decode");
        assert_eq!(back.mission, "01_AMBUSH.DAT");
        assert_eq!(back.ammo, [10, 3]);
        assert_eq!(back.reserve, [40, 8]);
        assert!(back.mca);
        assert!((back.health - 420.0).abs() < 0.01);
        assert!((back.x - 3072.0).abs() < 0.01);
        assert!((back.pitch + 0.1).abs() < 0.001);
    }

    #[test]
    fn continue_file_reloads() {
        let path = std::env::temp_dir().join(format!("shogo-continue-{}.sav", std::process::id()));
        let save = sample();
        assert!(write_at(&path, &save));
        let back = read_at(&path).expect("read");
        assert!((back.health - 123.0).abs() > 1.0);
        assert!((back.health - 420.0).abs() < 0.01);
        assert_eq!(back.weapon, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ambush_bumper_is_a_picture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/INTERFACE/BUMPERS/AMBUSH.PCX");
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(_) => return,
        };
        let (w, h, rgba) = decode_pcx(&data).expect("pcx");
        assert_eq!((w, h), (640, 480));
        assert_eq!(rgba.len(), 640 * 480 * 4);
        assert!(rgba.iter().any(|b| *b != rgba[0]));
    }
}
