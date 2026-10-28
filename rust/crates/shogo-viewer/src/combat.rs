//! Hitscan rifle, pulse bolts, and a mixed squad roster.
//!
//! Infantry, heavies, and mechs are retail ABC models. Each kind has its own
//! health, speed, attack range, and fire interval. The player's health pool
//! is larger than any single enemy. A wave director fills anchors across the
//! yard, and dead bodies tip over instead of popping.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::collide::Collider;
use crate::input::Actions;
use crate::nav::WalkNav;
use crate::{
    find_asset_root, load_dtx, resolve_ci, world_path, Checkpoint, LevelEntity, Player, WorldCollide,
    BODY, MAG_SIZE, PLAYER_HP, RESERVE_SIZE,
};

const ASSAULT_DAMAGE: f32 = 15.0;
const ASSAULT_COOLDOWN: f32 = 0.14;
const PULSE_DAMAGE: f32 = 40.0;
const PULSE_COOLDOWN: f32 = 0.5;
const PULSE_SPEED: f32 = 3_200.0;
const RELOAD_TIME: [f32; 2] = [1.25, 1.7];

const MARKSMAN_LO: f32 = 96.0;
const MARKSMAN_HI: f32 = 128.0;
const MARKSMAN_SCALE: f32 = 0.001;
const MIN_SPAWN: f32 = 560.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnemyKind {
    Trooper = 0,
    Shock = 1,
    Andra5 = 2,
    Andra10 = 3,
    Raksha = 4,
    Ruin = 5,
    Uhlan = 6,
}

const KINDS: [EnemyKind; 7] = [
    EnemyKind::Trooper,
    EnemyKind::Shock,
    EnemyKind::Andra5,
    EnemyKind::Andra10,
    EnemyKind::Raksha,
    EnemyKind::Ruin,
    EnemyKind::Uhlan,
];

struct EnemyDef {
    health: f32,
    move_speed: f32,
    attack_distance: f32,
    fire_interval: f32,
    damage: f32,
    hit_radius: f32,
    body_radius: f32,
    height: f32,
    sight: f32,
    model: &'static str,
    skin: &'static str,
    name: &'static str,
}

const DEFS: [EnemyDef; 7] = [
    EnemyDef {
        health: 80.0,
        move_speed: 300.0,
        attack_distance: 150.0,
        fire_interval: 0.30,
        damage: 12.0,
        hit_radius: 30.0,
        body_radius: 20.0,
        height: 64.0,
        sight: 4_000.0,
        model: "MODELS/ENEMIES/ONFOOT/TROOPER.ABC",
        skin: "SKINS/ENEMIES/TROOPER_SHOGO.DTX",
        name: "trooper",
    },
    EnemyDef {
        health: 160.0,
        move_speed: 220.0,
        attack_distance: 240.0,
        fire_interval: 0.5,
        damage: 18.0,
        hit_radius: 34.0,
        body_radius: 26.0,
        height: 78.0,
        sight: 4_400.0,
        model: "MODELS/ENEMIES/ONFOOT/SHOCKTROOPER.ABC",
        skin: "SKINS/ENEMIES/SHOCKTROOPER_SHOGO.DTX",
        name: "shock trooper",
    },
    EnemyDef {
        health: 220.0,
        move_speed: 190.0,
        attack_distance: 340.0,
        fire_interval: 0.65,
        damage: 20.0,
        hit_radius: 64.0,
        body_radius: 52.0,
        height: 150.0,
        sight: 4_800.0,
        model: "MODELS/ENEMIES/MCA/ANDRA5.ABC",
        skin: "SKINS/ENEMIES/ANDRA5_SHOGO.DTX",
        name: "andra 5",
    },
    EnemyDef {
        health: 300.0,
        move_speed: 170.0,
        attack_distance: 400.0,
        fire_interval: 0.8,
        damage: 24.0,
        hit_radius: 78.0,
        body_radius: 64.0,
        height: 175.0,
        sight: 5_000.0,
        model: "MODELS/ENEMIES/MCA/ANDRA10.ABC",
        skin: "SKINS/ENEMIES/ANDRA10_SHOGO.DTX",
        name: "andra 10",
    },
    EnemyDef {
        health: 360.0,
        move_speed: 210.0,
        attack_distance: 440.0,
        fire_interval: 0.7,
        damage: 26.0,
        hit_radius: 72.0,
        body_radius: 58.0,
        height: 160.0,
        sight: 5_200.0,
        model: "MODELS/ENEMIES/MCA/RAKSHA.ABC",
        skin: "SKINS/ENEMIES/RAKSHA_SHOGO.DTX",
        name: "raksha",
    },
    EnemyDef {
        health: 420.0,
        move_speed: 120.0,
        attack_distance: 480.0,
        fire_interval: 1.1,
        damage: 30.0,
        hit_radius: 110.0,
        body_radius: 90.0,
        height: 110.0,
        sight: 5_200.0,
        model: "MODELS/ENEMIES/ONFOOT/RUIN150.ABC",
        skin: "SKINS/ENEMIES/RUIN150_SHOGO.DTX",
        name: "ruin 150",
    },
    EnemyDef {
        health: 260.0,
        move_speed: 160.0,
        attack_distance: 380.0,
        fire_interval: 0.75,
        damage: 22.0,
        hit_radius: 80.0,
        body_radius: 70.0,
        height: 90.0,
        sight: 5_000.0,
        model: "MODELS/ENEMIES/MCA/UHLANA3.ABC",
        skin: "SKINS/ENEMIES/UHLANA3_SHOGO.DTX",
        name: "uhlan",
    },
];

impl EnemyKind {
    fn def(self) -> &'static EnemyDef {
        &DEFS[self as usize]
    }

    fn from_u8(v: u8) -> Self {
        KINDS.get(v as usize).copied().unwrap_or(Self::Trooper)
    }

    /// Retail MCA bodies live under `MODELS/ENEMIES/MCA`. Ruin stays on-foot.
    pub(crate) fn is_mech(self) -> bool {
        matches!(
            self,
            Self::Andra5 | Self::Andra10 | Self::Raksha | Self::Uhlan
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AiState {
    Idle,
    Patrol,
    Chase,
    Attack,
    Dead,
}

#[derive(Component)]
pub(crate) struct Enemy {
    pub(crate) kind: EnemyKind,
    /// UCA units in a campaign mission stay with the player.
    pub(crate) ally: bool,
    pub(crate) health: f32,
    pub(crate) fire_cd: f32,
    pub(crate) alive: bool,
    pub(crate) state: AiState,
    pub(crate) patrol: [Vec3; 2],
    pub(crate) patrol_i: u8,
    pub(crate) idle: f32,
    pub(crate) lost: f32,
    pub(crate) last_seen: Vec3,
    pub(crate) path: Vec<Vec3>,
    pub(crate) path_cd: f32,
    pub(crate) yaw: f32,
    pub(crate) dead_t: f32,
    pub(crate) fall_v: f32,
    /// Object name from the DAT, used by `FollowPath` and `Destroy`.
    pub(crate) label: String,
    /// Authored waypoints. Empty until a script order arrives.
    pub(crate) route: Vec<Vec3>,
    pub(crate) route_i: u16,
    pub(crate) run: bool,
    /// Placed by the mission, so they wait on the script instead of patrolling.
    pub(crate) authored: bool,
}

#[derive(Component)]
pub(crate) struct ViewGun(pub(crate) u8);

#[derive(Component)]
pub(crate) struct HealthPip(pub(crate) u8);

#[derive(Component)]
pub(crate) struct AmmoPip(u8);

#[derive(Component)]
pub(crate) struct FormLamp;

/// A HUD quad whose position and size are fractions of the view height, so the
/// overlay stays put when the field of view or the window changes.
#[derive(Component)]
pub(crate) struct HudWidget {
    sx: f32,
    sy: f32,
    sw: f32,
    sh: f32,
    /// Fill the view. `sh` is the height fraction; width follows the aspect.
    cover: bool,
}

#[derive(Component)]
pub(crate) struct HurtVeil;

#[derive(Component)]
pub(crate) struct Bolt {
    dir: Vec3,
    damage: f32,
    life: f32,
    speed: f32,
}

#[derive(Component)]
pub(crate) struct MuzzleFlash {
    life: f32,
    duration: f32,
    frames: Vec<Handle<StandardMaterial>>,
    world: bool,
    scale: f32,
}

#[derive(Component)]
pub(crate) struct ShotBeam {
    life: f32,
}

#[derive(Resource)]
pub(crate) struct ShotRng(pub(crate) u32);

#[derive(Resource)]
pub(crate) struct MuzzleBank {
    mesh: Handle<Mesh>,
    assault: Vec<Handle<StandardMaterial>>,
    pulse: Vec<Handle<StandardMaterial>>,
    enemy: Vec<Handle<StandardMaterial>>,
    bolt_mesh: Handle<Mesh>,
    bolt_mat: Handle<StandardMaterial>,
}

#[derive(Resource)]
pub(crate) struct Roster {
    mesh: Vec<Handle<Mesh>>,
    mat: Vec<Handle<StandardMaterial>>,
}

#[derive(Resource)]
pub(crate) struct Waves {
    index: u32,
    queue: Vec<u8>,
    timer: f32,
    calm: f32,
    anchors: Vec<Vec3>,
    cursor: usize,
    spawned: u32,
    /// The level placed its own cast. Do not run the deathmatch director.
    scripted: bool,
}

struct CastMember {
    kind: EnemyKind,
    pos: Vec3,
    ally: bool,
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    label: String,
}

#[derive(Resource)]
pub(crate) struct Mission {
    cast: Vec<CastMember>,
}

impl Waves {
    fn restart(&mut self) {
        self.queue.clear();
        self.timer = 0.0;
        self.cursor = 0;
        self.spawned = 0;
        if self.scripted {
            return;
        }
        self.index = 0;
        self.calm = 0.6;
    }
}

/// Cast, props, and the wave director for the current world. Guns and the HUD
/// stay on the player, so a level change calls this without `spawn_fight`.
pub(crate) fn spawn_cast(
    mut commands: Commands,
    collide: Option<Res<WorldCollide>>,
    checkpoint: Option<Res<Checkpoint>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(collide), Some(checkpoint)) = (collide, checkpoint) else {
        return;
    };
    let root = world_path().and_then(|p| find_asset_root(std::path::Path::new(&p)));
    deploy_cast(
        &mut commands,
        &collide,
        &checkpoint,
        root.as_deref(),
        meshes.as_mut(),
        materials.as_mut(),
        images.as_mut(),
    );
}

fn deploy_cast(
    commands: &mut Commands,
    collide: &WorldCollide,
    checkpoint: &Checkpoint,
    root: Option<&std::path::Path>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let mut mesh = Vec::with_capacity(DEFS.len());
    let mut mat = Vec::with_capacity(DEFS.len());
    for def in &DEFS {
        let model = load_abc(root, def.model);
        mesh.push(meshes.add(match &model {
            Some(model) => character_mesh(model, def.height),
            None => Mesh::from(Cuboid::new(def.body_radius * 2.0, def.height, def.body_radius)),
        }));
        let skin = root
            .and_then(|r| load_dtx(r, def.skin))
            .map(|image| images.add(image));
        mat.push(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: skin,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        }));
        info!(
            "{} prefab ({})",
            def.name,
            if model.is_some() { "abc" } else { "fallback" }
        );
    }
    let roster = Roster { mesh, mat };
    let mission = world_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|data| shogo_assets::dat::level_actors(&data).ok())
        .unwrap_or_default();
    let scripted = mission.iter().any(|actor| actor.class.contains('_'));
    if scripted {
        let cast = place_mission(
            commands,
            &collide.0,
            root,
            &mission,
            checkpoint.pos,
            meshes,
            materials,
            images,
        );
        let hostiles = cast.iter().filter(|m| !m.ally).count();
        let allies = cast.len() - hostiles;
        info!("mission cast {hostiles} hostiles, {allies} allies");
        commands.insert_resource(Mission { cast });
        commands.insert_resource(Waves {
            index: 0,
            queue: Vec::new(),
            timer: 0.0,
            calm: 0.0,
            anchors: Vec::new(),
            cursor: 0,
            spawned: 0,
            scripted: true,
        });
    } else {
        let anchors = anchors(&collide.0, checkpoint.pos);
        info!("wave anchors {}", anchors.len());
        const OPENING: [u8; 6] = [0, 1, 2, 3, 4, 0];
        let opening = anchors.len().min(OPENING.len());
        for (slot, pos) in anchors.iter().take(opening).enumerate() {
            spawn_enemy(
                commands,
                &roster,
                EnemyKind::from_u8(OPENING[slot]),
                *pos,
                checkpoint.pos,
            );
        }
        info!("opening squad {opening}");
        commands.insert_resource(Waves {
            index: 1,
            queue: Vec::new(),
            timer: 0.0,
            calm: 2.5,
            anchors,
            cursor: 0,
            spawned: 0,
            scripted: false,
        });
    }
    commands.insert_resource(roster);
}

pub(crate) fn spawn_fight(
    mut commands: Commands,
    collide: Option<Res<WorldCollide>>,
    checkpoint: Option<Res<Checkpoint>>,
    player: Query<Entity, With<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(collide), Some(checkpoint), Ok(player)) = (collide, checkpoint, player.single())
    else {
        return;
    };
    let root = world_path().and_then(|p| find_asset_root(std::path::Path::new(&p)));

    let assault = view_gun(
        root.as_deref(),
        "MODELS/PV_WEAPONS/ASSAULTRIFLE_PV.ABC",
        "SKINS/WEAPONS/ASSAULTRIFLE_PV_A.DTX",
        Color::srgb(0.35, 0.35, 0.32),
        images.as_mut(),
    );
    let pulse = view_gun(
        root.as_deref(),
        "MODELS/PV_WEAPONS/PULSERIFLE_PV.ABC",
        "SKINS/WEAPONS/PULSERIFLE_PV_A.DTX",
        Color::srgb(0.25, 0.55, 0.85),
        images.as_mut(),
    );
    let hold = Vec3::new(0.38, -0.30, -1.2);
    commands.entity(player).with_children(|parent| {
        parent.spawn(gun_bundle(
            &mut meshes,
            &mut materials,
            assault,
            0,
            hold,
            Visibility::Visible,
        ));
        parent.spawn(gun_bundle(
            &mut meshes,
            &mut materials,
            pulse,
            1,
            hold,
            Visibility::Hidden,
        ));
        let quad = meshes.add(Cuboid::new(1.0, 1.0, 0.001));
        parent.spawn((
            HurtVeil,
            HudWidget {
                sx: 0.0,
                sy: 0.0,
                sw: 1.0,
                sh: 1.08,
                cover: true,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.7, 0.05, 0.05, 0.0),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
        ));
        let hair = materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.95, 0.9),
            unlit: true,
            ..default()
        });
        parent.spawn((
            HudWidget {
                sx: 0.0,
                sy: 0.0,
                sw: 0.028,
                sh: 0.0035,
                cover: false,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(hair.clone()),
            Transform::default(),
        ));
        parent.spawn((
            HudWidget {
                sx: 0.0,
                sy: 0.0,
                sw: 0.0035,
                sh: 0.028,
                cover: false,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(hair),
            Transform::default(),
        ));
        for i in 0..10u8 {
            let x = -0.52 + i as f32 * 0.026;
            parent.spawn((
                HealthPip(i),
                HudWidget {
                    sx: x,
                    sy: -0.40,
                    sw: 0.022,
                    sh: 0.032,
                    cover: false,
                },
                Mesh3d(quad.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.25, 0.85, 0.3),
                    unlit: true,
                    ..default()
                })),
                Transform::default(),
            ));
        }
        for i in 0..8u8 {
            let x = 0.28 + i as f32 * 0.026;
            parent.spawn((
                AmmoPip(i),
                HudWidget {
                    sx: x,
                    sy: -0.40,
                    sw: 0.022,
                    sh: 0.032,
                    cover: false,
                },
                Mesh3d(quad.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.9, 0.75, 0.2),
                    unlit: true,
                    ..default()
                })),
                Transform::default(),
            ));
        }
        parent.spawn((
            FormLamp,
            HudWidget {
                sx: 0.52,
                sy: 0.36,
                sw: 0.036,
                sh: 0.036,
                cover: false,
            },
            Mesh3d(quad),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.3, 0.85, 0.4),
                unlit: true,
                ..default()
            })),
            Transform::default(),
        ));
    });

    deploy_cast(
        &mut commands,
        &collide,
        &checkpoint,
        root.as_deref(),
        &mut meshes,
        &mut materials,
        images.as_mut(),
    );

    let square = load_abc(root.as_deref(), "MODELS/1X1_SQUARE.ABC");
    let flash_mesh = meshes.add(match &square {
        Some(model) => sprite_mesh(model),
        None => Mesh::from(Cuboid::new(1.0, 1.0, 0.15)),
    });
    let assault_flash = load_sprite_frames(
        root.as_deref(),
        "SPRITES/ASSAULTRIFLEMUZZLEFLASH.SPR",
        images.as_mut(),
        materials.as_mut(),
    );
    let pulse_flash = load_sprite_frames(
        root.as_deref(),
        "SPRITES/PULSERIFLEMUZZLEFLASH1.SPR",
        images.as_mut(),
        materials.as_mut(),
    );
    info!(
        "muzzle flashes: assault {} frames, pulse {} frames",
        assault_flash.len(),
        pulse_flash.len()
    );
    commands.insert_resource(ShotRng(0x5A17_u32));
    commands.insert_resource(MuzzleBank {
        mesh: flash_mesh,
        enemy: assault_flash.clone(),
        assault: assault_flash,
        pulse: pulse_flash,
        bolt_mesh: meshes.add(Cuboid::new(3.5, 22.0, 3.5)),
        bolt_mat: materials.add(StandardMaterial {
            base_color: Color::linear_rgba(0.55, 2.2, 3.6, 1.0),
            emissive: LinearRgba::new(0.35, 1.8, 3.2, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
    });
}

pub(crate) fn waves(
    time: Res<Time>,
    mut commands: Commands,
    collide: Option<Res<WorldCollide>>,
    roster: Option<Res<Roster>>,
    mut waves: Option<ResMut<Waves>>,
    players: Query<(&Player, &Transform)>,
    enemies: Query<(Entity, &Enemy)>,
) {
    let (Some(collide), Some(roster), Some(waves)) = (collide, roster, waves.as_mut()) else {
        return;
    };
    let Ok((player, player_tf)) = players.single() else {
        return;
    };
    if player.health <= 0.0 {
        return;
    }
    if waves.scripted {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let alive = enemies.iter().filter(|(_, e)| e.alive).count();
    if waves.queue.is_empty() {
        if alive == 0 {
            waves.calm -= dt;
            if waves.calm <= 0.0 {
                for (entity, _) in &enemies {
                    commands.entity(entity).despawn();
                }
                waves.index += 1;
                waves.queue = wave_plan(waves.index);
                waves.timer = 0.15;
                waves.calm = 3.5;
                waves.spawned = 0;
                info!("wave {} ({} hostiles)", waves.index, waves.queue.len());
            }
        }
        return;
    }
    waves.timer -= dt;
    if waves.timer > 0.0 || waves.anchors.is_empty() {
        return;
    }
    let kind = EnemyKind::from_u8(waves.queue.remove(0));
    let anchor = waves.anchors[waves.cursor % waves.anchors.len()];
    waves.cursor += 1;
    let side = (waves.spawned % 5) as f32 - 2.0;
    waves.spawned += 1;
    let to_player = player_tf.translation - anchor;
    let flat = Vec3::new(to_player.x, 0.0, to_player.z).normalize_or_zero();
    let right = flat.cross(Vec3::Y);
    let nudged = anchor + right * side * 70.0;
    let pos = stand_on(&collide.0, nudged.x, nudged.z, nudged.y, kind.def())
        .filter(|p| p.distance(anchor) < 140.0 && (p.y - anchor.y).abs() < 48.0)
        .unwrap_or(anchor);
    spawn_enemy(&mut commands, roster.as_ref(), kind, pos, player_tf.translation);
    waves.timer = 0.65;
}

fn spawn_enemy(commands: &mut Commands, roster: &Roster, kind: EnemyKind, pos: Vec3, player: Vec3) {
    let i = kind as usize;
    spawn_unit(
        commands,
        kind,
        false,
        pos,
        player,
        roster.mesh[i].clone(),
        roster.mat[i].clone(),
        kind.def().name,
        false,
    );
}

fn spawn_unit(
    commands: &mut Commands,
    kind: EnemyKind,
    ally: bool,
    pos: Vec3,
    player: Vec3,
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    label: &str,
    authored: bool,
) {
    let def = kind.def();
    let to_player = player - pos;
    let yaw = (-to_player.x).atan2(-to_player.z);
    let right = Vec3::new(to_player.z, 0.0, -to_player.x).normalize_or_zero();
    let patrol_b = pos + right * 220.0;
    let center = def.height * 0.5;
    commands
        .spawn((
            LevelEntity,
            Enemy {
                kind,
                ally,
                health: def.health,
                fire_cd: def.fire_interval,
                alive: true,
                state: AiState::Idle,
                patrol: [pos, patrol_b],
                patrol_i: 0,
                idle: 0.7,
                lost: 0.0,
                last_seen: player,
                path: Vec::new(),
                path_cd: 0.0,
                yaw,
                dead_t: 0.0,
                fall_v: 0.0,
                label: label.to_string(),
                route: Vec::new(),
                route_i: 0,
                run: false,
                authored,
            },
            Visibility::Visible,
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_xyz(0.0, -center, 0.0),
            ));
        });
    info!("{label} at ({:.0},{:.0},{:.0})", pos.x, pos.y, pos.z);
}

fn place_mission(
    commands: &mut Commands,
    collide: &Collider,
    root: Option<&std::path::Path>,
    actors: &[shogo_assets::dat::LevelActor],
    player: Vec3,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Vec<CastMember> {
    let mut cast = Vec::new();
    for actor in actors {
        let rel = actor.model.replace('\\', "/");
        if actor.class.contains('_') {
            let kind = kind_from_class(&actor.class);
            let ally = is_ally(&actor.class);
            let model = if rel.is_empty() {
                load_abc(root, kind.def().model)
            } else {
                load_abc(root, &rel).or_else(|| load_abc(root, kind.def().model))
            };
            let mesh = meshes.add(match &model {
                Some(model) => character_mesh(model, kind.def().height),
                None => Mesh::from(Cuboid::new(
                    kind.def().body_radius * 2.0,
                    kind.def().height,
                    kind.def().body_radius,
                )),
            });
            let skin_rel = faction_skin(&actor.class, &rel).unwrap_or_else(|| kind.def().skin.to_string());
            let skin = root
                .and_then(|r| load_dtx(r, &skin_rel))
                .or_else(|| root.and_then(|r| load_dtx(r, kind.def().skin)))
                .map(|image| images.add(image));
            let mat = materials.add(StandardMaterial {
                base_color: if ally {
                    Color::srgb(0.45, 0.62, 0.95)
                } else {
                    Color::WHITE
                },
                base_color_texture: skin,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            let authored = crate::world_pos(actor.pos);
            let pos = stand_on(collide, authored.x, authored.z, authored.y, kind.def()).unwrap_or(authored);
            spawn_unit(
                commands,
                kind,
                ally,
                pos,
                player,
                mesh.clone(),
                mat.clone(),
                &actor.name,
                true,
            );
            cast.push(CastMember {
                kind,
                pos,
                ally,
                mesh,
                mat,
                label: actor.name.clone(),
            });
        } else if rel.to_ascii_lowercase().ends_with(".abc") {
            let Some(model) = load_abc(root, &rel) else {
                continue;
            };
            let height = if actor.class.eq_ignore_ascii_case("DropShip") {
                240.0
            } else {
                96.0
            };
            let mesh = meshes.add(character_mesh(&model, height));
            let skin_rel = if actor.skin.is_empty() {
                None
            } else {
                Some(actor.skin.replace('\\', "/"))
            };
            let skin = skin_rel
                .as_deref()
                .and_then(|skin| root.and_then(|r| load_dtx(r, skin)))
                .map(|image| images.add(image));
            let mat = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: skin,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            let authored = crate::world_pos(actor.pos);
            let pos = collide
                .floor_y(authored.x, authored.z, authored.y + 128.0)
                .filter(|fy| (authored.y - fy).abs() < 160.0)
                .map(|fy| Vec3::new(authored.x, fy, authored.z))
                .unwrap_or(authored);
            commands.spawn((
                LevelEntity,
                crate::script::ScriptProp(actor.name.clone()),
                Mesh3d(mesh),
                MeshMaterial3d(mat),
                Transform::from_translation(pos),
            ));
            info!("prop {} at ({:.0},{:.0},{:.0})", actor.name, pos.x, pos.y, pos.z);
        }
    }
    cast
}

fn is_ally(class: &str) -> bool {
    class.to_ascii_uppercase().starts_with("UCA_")
}

fn kind_from_class(class: &str) -> EnemyKind {
    let c = class.to_ascii_lowercase();
    if c.contains("ruin") {
        EnemyKind::Ruin
    } else if c.contains("andra10") {
        EnemyKind::Andra10
    } else if c.contains("andra") {
        EnemyKind::Andra5
    } else if c.contains("raksha") {
        EnemyKind::Raksha
    } else if c.contains("uhlan") {
        EnemyKind::Uhlan
    } else if c.contains("shock") {
        EnemyKind::Shock
    } else if c.contains("sparrow") || c.contains("vigilance") {
        EnemyKind::Andra5
    } else {
        EnemyKind::Trooper
    }
}

fn faction_skin(class: &str, model_rel: &str) -> Option<String> {
    let faction = class.split('_').next()?.to_ascii_uppercase();
    if !matches!(faction.as_str(), "CRONIAN" | "FALLEN" | "UCA" | "CMC" | "SHOGO") {
        return None;
    }
    let stem = std::path::Path::new(model_rel)
        .file_stem()?
        .to_string_lossy()
        .to_ascii_uppercase();
    if stem.is_empty() {
        return None;
    }
    Some(format!("SKINS/ENEMIES/{stem}_{faction}.DTX"))
}

pub(crate) fn weapons(
    time: Res<Time>,
    actions: Res<Actions>,
    mut commands: Commands,
    collide: Option<Res<WorldCollide>>,
    flashes: Option<Res<MuzzleBank>>,
    checkpoint: Option<Res<Checkpoint>>,
    mut rng: Option<ResMut<ShotRng>>,
    mut waves: Option<ResMut<Waves>>,
    mission: Option<Res<Mission>>,
    mut players: Query<(Entity, &mut Player, &mut Transform), Without<Enemy>>,
    mut guns: Query<(&ViewGun, &mut Visibility), Without<Enemy>>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), (Without<Player>, Without<ViewGun>)>,
    mut doors: Query<(Entity, &mut crate::script::Barricade, &Transform), (Without<Enemy>, Without<Player>)>,
) {
    let (Some(collide), Some(flashes)) = (collide.as_ref(), flashes.as_ref()) else {
        return;
    };
    let Ok((player_ent, mut player, mut tf)) = players.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    player.fire_cd = (player.fire_cd - dt).max(0.0);
    player.hurt = (player.hurt - dt).max(0.0);

    if let Some(slot) = actions.weapon_slot {
        if slot != player.weapon {
            player.weapon = slot;
            player.reloading = 0.0;
        }
    }
    if actions.cycle {
        player.weapon = (player.weapon + 1) % 2;
        player.reloading = 0.0;
    }
    for (gun, mut vis) in &mut guns {
        *vis = if gun.0 == player.weapon {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    if player.reloading > 0.0 {
        player.reloading = (player.reloading - dt).max(0.0);
        if player.reloading == 0.0 {
            let p = &mut *player;
            let slot = p.weapon;
            finish_reload(slot, &mut p.ammo, &mut p.reserve);
        }
    } else if player.health > 0.0
        && should_auto_reload(
            player.ammo[player.weapon as usize],
            player.reserve[player.weapon as usize],
        )
    {
        player.reloading = RELOAD_TIME[player.weapon as usize];
    } else if actions.reload && can_reload(player.weapon, player.ammo, player.reserve) {
        player.reloading = RELOAD_TIME[player.weapon as usize];
    }

    if player.health <= 0.0 {
        if actions.respawn {
            if let Some(checkpoint) = checkpoint {
                revive(
                    &mut commands,
                    &mut player,
                    &mut tf,
                    &checkpoint,
                    &mut enemies,
                    waves.as_deref_mut(),
                    mission.as_deref(),
                );
            }
        }
        return;
    }

    if player.reloading > 0.0 || !actions.fire || player.fire_cd > 0.0 {
        return;
    }
    let slot = player.weapon as usize;
    if player.ammo[slot] == 0 {
        if can_reload(player.weapon, player.ammo, player.reserve) {
            player.reloading = RELOAD_TIME[slot];
        }
        return;
    }

    let (damage, cooldown, spread, frames) = if player.weapon == 0 {
        (
            ASSAULT_DAMAGE,
            ASSAULT_COOLDOWN,
            0.008 * (1.0 - 0.85 * player.ads),
            flashes.assault.clone(),
        )
    } else {
        (
            PULSE_DAMAGE,
            PULSE_COOLDOWN,
            0.004 * (1.0 - player.ads),
            flashes.pulse.clone(),
        )
    };
    player.fire_cd = cooldown;
    player.ammo[slot] = player.ammo[slot].saturating_sub(1);
    if player.ammo[slot] == 0 && can_reload(player.weapon, player.ammo, player.reserve) {
        player.reloading = RELOAD_TIME[slot];
    }

    let origin = tf.translation;
    let fwd = tf.forward();
    let forward = Vec3::new(fwd.x, fwd.y, fwd.z);
    let dir = spread_dir(forward, spread, rng.as_deref_mut());
    if player.weapon == 0 {
        let wall = collide.0.raycast(origin, dir, 8_000.0).unwrap_or(8_000.0);
        let mut best_t = wall;
        let mut best: Option<Entity> = None;
        for (entity, enemy, etf) in &enemies {
            if !enemy.alive {
                continue;
            }
            let radius = enemy.kind.def().hit_radius;
            if let Some(t) = ray_sphere(origin, dir, etf.translation, radius) {
                if t < best_t {
                    best_t = t;
                    best = Some(entity);
                }
            }
        }
        let mut door_hit: Option<Entity> = None;
        for (entity, door, dtf) in &doors {
            if door.hp <= 0.0 {
                continue;
            }
            if let Some(t) = crate::script::ray_aabb(origin, dir, dtf.translation, door.half) {
                if t < best_t {
                    best_t = t;
                    best = None;
                    door_hit = Some(entity);
                }
            }
        }
        if let Some(entity) = door_hit {
            if let Ok((_, mut door, dtf)) = doors.get_mut(entity) {
                if crate::script::harm(&mut door, damage) {
                    let at = dtf.translation;
                    commands.trigger(crate::door_audio::BarricadeBroke { at });
                    commands.entity(entity).despawn();
                }
            }
        } else if let Some(entity) = best {
            if let Ok((_, mut enemy, etf)) = enemies.get_mut(entity) {
                let center = etf.translation;
                if kill_or_hurt(&mut enemy, damage) {
                    report_hit(&mut commands, &enemy, entity, origin, dir, best_t, center);
                }
            }
        }
    } else if dir.length_squared() > 0.5 {
        let rot = Quat::from_rotation_arc(Vec3::Y, dir);
        commands.spawn((
            LevelEntity,
            Bolt {
                dir,
                damage,
                life: 1.4,
                speed: PULSE_SPEED,
            },
            Mesh3d(flashes.bolt_mesh.clone()),
            MeshMaterial3d(flashes.bolt_mat.clone()),
            Transform::from_translation(origin + dir * 48.0).with_rotation(rot),
        ));
    }
    spawn_view_flash(&mut commands, flashes, player_ent, frames);
}

pub(crate) fn bolts(
    time: Res<Time>,
    mut commands: Commands,
    collide: Option<Res<WorldCollide>>,
    flashes: Option<Res<MuzzleBank>>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Transform)>,
    mut enemies: Query<(Entity, &mut Enemy, &Transform), Without<Bolt>>,
    mut doors: Query<(Entity, &mut crate::script::Barricade, &Transform), Without<Bolt>>,
) {
    let (Some(collide), Some(flashes)) = (collide, flashes) else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    for (entity, mut bolt, mut tf) in &mut bolts {
        bolt.life -= dt;
        if bolt.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let travel = bolt.speed * dt;
        let origin = tf.translation;
        let wall = collide.0.raycast(origin, bolt.dir, travel).unwrap_or(travel);
        let mut best_t = wall;
        let mut best: Option<Entity> = None;
        for (enemy_ent, enemy, etf) in &enemies {
            if !enemy.alive {
                continue;
            }
            if let Some(t) = ray_sphere(origin, bolt.dir, etf.translation, enemy.kind.def().hit_radius)
            {
                if t < best_t {
                    best_t = t;
                    best = Some(enemy_ent);
                }
            }
        }
        let mut door_hit: Option<Entity> = None;
        for (door_ent, door, dtf) in &doors {
            if door.hp <= 0.0 {
                continue;
            }
            if let Some(t) = crate::script::ray_aabb(origin, bolt.dir, dtf.translation, door.half) {
                if t < best_t {
                    best_t = t;
                    best = None;
                    door_hit = Some(door_ent);
                }
            }
        }
        if let Some(hit) = door_hit {
            if let Ok((_, mut door, dtf)) = doors.get_mut(hit) {
                if crate::script::harm(&mut door, bolt.damage) {
                    let at = dtf.translation;
                    commands.trigger(crate::door_audio::BarricadeBroke { at });
                    commands.entity(hit).despawn();
                }
            }
            spawn_world_flash(&mut commands, &flashes, origin + bolt.dir * best_t, 28.0, 0.12);
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(hit) = best {
            let dir = bolt.dir;
            let damage = bolt.damage;
            if let Ok((_, mut enemy, etf)) = enemies.get_mut(hit) {
                let center = etf.translation;
                if kill_or_hurt(&mut enemy, damage) {
                    report_hit(&mut commands, &enemy, hit, origin, dir, best_t, center);
                }
            }
            spawn_world_flash(&mut commands, &flashes, origin + dir * best_t, 28.0, 0.12);
            commands.entity(entity).despawn();
            continue;
        }
        if wall < travel {
            spawn_world_flash(&mut commands, &flashes, origin + bolt.dir * wall, 18.0, 0.08);
            commands.entity(entity).despawn();
            continue;
        }
        tf.translation = origin + bolt.dir * travel;
    }
}

fn kill_or_hurt(enemy: &mut Enemy, damage: f32) -> bool {
    if !enemy.alive || enemy.ally {
        return false;
    }
    enemy.health = (enemy.health - damage).max(0.0);
    if enemy.health <= 0.0 {
        enemy.alive = false;
        enemy.state = AiState::Dead;
        enemy.dead_t = 0.0;
        enemy.fall_v = 80.0;
    }
    true
}

fn report_hit(
    commands: &mut Commands,
    enemy: &Enemy,
    target: Entity,
    origin: Vec3,
    dir: Vec3,
    t: f32,
    center: Vec3,
) {
    let impact_point = origin + dir * t;
    let mut normal = impact_point - center;
    if normal.length_squared() < 1e-4 {
        normal = -dir;
    }
    commands.trigger(crate::feedback::EntityHitEvent {
        target,
        impact_point,
        normal: normal.normalize_or_zero(),
        is_mech: enemy.kind.is_mech(),
    });
}

fn revive(
    commands: &mut Commands,
    player: &mut Player,
    tf: &mut Transform,
    checkpoint: &Checkpoint,
    enemies: &mut Query<(Entity, &mut Enemy, &Transform), (Without<Player>, Without<ViewGun>)>,
    waves: Option<&mut Waves>,
    mission: Option<&Mission>,
) {
    player.health = PLAYER_HP;
    player.vel = Vec3::ZERO;
    player.hurt = 0.0;
    player.fire_cd = 0.25;
    player.reloading = 0.0;
    player.ammo = MAG_SIZE;
    player.reserve = RESERVE_SIZE;
    player.grounded = true;
    player.yaw = checkpoint.yaw;
    player.pitch = 0.0;
    let floor = checkpoint.pos.y - BODY;
    tf.translation = Vec3::new(checkpoint.pos.x, floor + player.radius, checkpoint.pos.z);
    tf.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    for (entity, _, _) in enemies.iter() {
        commands.entity(entity).despawn();
    }
    if let Some(waves) = waves {
        waves.restart();
    }
    if let Some(mission) = mission {
        for member in &mission.cast {
            spawn_unit(
                commands,
                member.kind,
                member.ally,
                member.pos,
                checkpoint.pos,
                member.mesh.clone(),
                member.mat.clone(),
                &member.label,
                true,
            );
        }
    }
}

pub(crate) fn enemies(
    time: Res<Time>,
    mut commands: Commands,
    collide: Option<Res<WorldCollide>>,
    flashes: Option<Res<MuzzleBank>>,
    nav: Option<Res<WalkNav>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: Option<ResMut<ShotRng>>,
    mut players: Query<(&mut Player, &mut Transform), Without<Enemy>>,
    mut enemies: Query<(&mut Enemy, &mut Transform), (Without<Player>, Without<ViewGun>)>,
) {
    let (Some(collide), Some(flashes)) = (collide, flashes) else {
        return;
    };
    let Ok((mut player, mut ptf)) = players.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.05);
    let player_pos = ptf.translation;
    let mut bodies: Vec<(Vec3, f32, bool)> = Vec::new();
    for (enemy, tf) in &enemies {
        bodies.push((tf.translation, enemy.kind.def().body_radius, enemy.alive));
    }

    for (i, (mut enemy, mut tf)) in enemies.iter_mut().enumerate() {
        if !enemy.alive {
            enemy.dead_t += dt;
            enemy.fall_v -= 1_400.0 * dt;
            let mut pos = tf.translation;
            pos.y += enemy.fall_v * dt;
            if let Some(fy) = collide.0.floor_y(pos.x, pos.z, pos.y + 30.0) {
                if pos.y < fy + 8.0 {
                    pos.y = fy + 8.0;
                    enemy.fall_v = 0.0;
                }
            }
            let tip = (enemy.dead_t / 0.45).clamp(0.0, 1.0);
            tf.rotation = Quat::from_rotation_y(enemy.yaw) * Quat::from_rotation_z(-tip * 1.3);
            tf.translation = pos;
            bodies[i].0 = pos;
            continue;
        }

        let def = enemy.kind.def();
        enemy.fire_cd = (enemy.fire_cd - dt).max(0.0);
        if player.health <= 0.0 {
            continue;
        }
        let mut pos = tf.translation;
        let to = player_pos - pos;
        let flat = Vec3::new(to.x, 0.0, to.z);
        let dist = flat.length();
        let dir = flat.normalize_or_zero();
        let forward = Vec3::new(
            tf.forward().x,
            0.0,
            tf.forward().z,
        )
        .normalize_or_zero();
        let eye = pos + Vec3::Y * (def.height * 0.35);
        let see = !enemy.ally
            && dist < def.sight
            && (enemy.state == AiState::Chase
                || enemy.state == AiState::Attack
                || forward.dot(dir) > 0.15)
            && line_of_sight(&collide.0, eye, player_pos, dist);
        if see {
            enemy.last_seen = player_pos;
            enemy.lost = 0.0;
        }

        if enemy.authored {
            let following = (enemy.route_i as usize) < enemy.route.len();
            let hold = enemy.ally || (!see && !matches!(enemy.state, AiState::Chase | AiState::Attack));
            if hold {
                if following {
                    let goal = enemy.route[enemy.route_i as usize];
                    let steer = Vec3::new(goal.x - pos.x, 0.0, goal.z - pos.z);
                    if steer.length() < 80.0 {
                        enemy.route_i = enemy.route_i.saturating_add(1);
                    } else {
                        let dir_g = steer.normalize_or_zero();
                        enemy.yaw = (-dir_g.x).atan2(-dir_g.z);
                        tf.rotation = Quat::from_rotation_y(enemy.yaw);
                        let speed = if enemy.run {
                            def.move_speed
                        } else {
                            def.move_speed * 0.45
                        };
                        pos = step_on(
                            &collide.0,
                            pos,
                            dir_g,
                            speed,
                            def.body_radius,
                            def.height * 0.5,
                            i,
                            &bodies,
                            dt,
                        );
                    }
                }
                tf.translation = pos;
                bodies[i].0 = pos;
                continue;
            }
        }

        enemy.state = match enemy.state {
            AiState::Dead => AiState::Dead,
            AiState::Idle => {
                enemy.idle -= dt;
                if see {
                    if dist <= def.attack_distance {
                        AiState::Attack
                    } else {
                        AiState::Chase
                    }
                } else if enemy.idle <= 0.0 {
                    AiState::Patrol
                } else {
                    AiState::Idle
                }
            }
            AiState::Patrol => {
                if see {
                    if dist <= def.attack_distance {
                        AiState::Attack
                    } else {
                        AiState::Chase
                    }
                } else {
                    AiState::Patrol
                }
            }
            AiState::Chase => {
                if !see {
                    enemy.lost += dt;
                }
                if see && dist <= def.attack_distance {
                    AiState::Attack
                } else if enemy.lost > 3.2 {
                    AiState::Patrol
                } else {
                    AiState::Chase
                }
            }
            AiState::Attack => {
                if !see || dist > def.attack_distance * 1.35 {
                    AiState::Chase
                } else {
                    AiState::Attack
                }
            }
        };

        let goal = match enemy.state {
            AiState::Patrol => enemy.patrol[enemy.patrol_i as usize],
            AiState::Chase => {
                if see {
                    player_pos
                } else {
                    enemy.last_seen
                }
            }
            _ => pos,
        };

        if matches!(enemy.state, AiState::Chase | AiState::Patrol) {
            enemy.path_cd -= dt;
            if enemy.path_cd <= 0.0 {
                enemy.path = nav.as_ref().map(|n| n.0.find(pos, goal)).unwrap_or_default();
                enemy.path_cd = 0.4;
            }
        }

        if matches!(enemy.state, AiState::Chase | AiState::Patrol) {
            let steer_at = waypoint(&enemy.path, pos, goal);
            let steer = Vec3::new(steer_at.x - pos.x, 0.0, steer_at.z - pos.z).normalize_or_zero();
            if steer.length_squared() > 0.1 {
                enemy.yaw = (-steer.x).atan2(-steer.z);
                tf.rotation = Quat::from_rotation_y(enemy.yaw);
                pos = step_on(
                    &collide.0,
                    pos,
                    steer,
                    def.move_speed,
                    def.body_radius,
                    def.height * 0.5,
                    i,
                    &bodies,
                    dt,
                );
            }
            if enemy.state == AiState::Patrol && pos.distance(enemy.patrol[enemy.patrol_i as usize]) < 90.0
            {
                enemy.patrol_i ^= 1;
            }
        } else if enemy.state == AiState::Attack && dist > 1.0 {
            enemy.yaw = (-dir.x).atan2(-dir.z);
            tf.rotation = Quat::from_rotation_y(enemy.yaw);
            if see && enemy.fire_cd <= 0.0 {
                enemy.fire_cd = def.fire_interval;
                let aim = marksman_dir(to.normalize_or_zero(), rng.as_deref_mut());
                let wall = collide.0.raycast(eye, aim, 4_000.0).unwrap_or(4_000.0);
                let mut end_t = wall;
                if let Some(t) = ray_sphere(eye, aim, player_pos, player.radius.max(BODY)) {
                    if t < wall {
                        end_t = t;
                        player.health = (player.health - def.damage).max(0.0);
                        player.hurt = 0.12;
                        if player.health <= 0.0 {
                            player.vel = Vec3::ZERO;
                        }
                    }
                }
                let muzzle = pos + dir * (def.body_radius + 8.0) + Vec3::Y * (def.height * 0.15);
                spawn_world_flash(&mut commands, &flashes, muzzle, def.height * 0.45, 0.2);
                let show = (end_t - 64.0).clamp(36.0, 900.0);
                spawn_shot_beam(&mut commands, &mut meshes, &mut materials, muzzle, aim, show);
            }
        }

        if player.health > 0.0 {
            let delta = Vec3::new(player_pos.x - pos.x, 0.0, player_pos.z - pos.z);
            let d = delta.length();
            let min = player.radius + def.body_radius;
            if d < min && d > 0.05 {
                ptf.translation += delta.normalize() * (min - d);
            }
        }
        tf.translation = pos;
        bodies[i].0 = pos;
    }
}

fn waypoint(path: &[Vec3], pos: Vec3, fallback: Vec3) -> Vec3 {
    for wp in path {
        let dx = wp.x - pos.x;
        let dz = wp.z - pos.z;
        if dx * dx + dz * dz > 72.0 * 72.0 {
            return *wp;
        }
    }
    path.last().copied().unwrap_or(fallback)
}

fn step_on(
    col: &Collider,
    pos: Vec3,
    dir: Vec3,
    speed: f32,
    body: f32,
    center: f32,
    i: usize,
    bodies: &[(Vec3, f32, bool)],
    dt: f32,
) -> Vec3 {
    let step = dir * speed * dt;
    let mut nx = pos.x + step.x;
    let mut nz = pos.z + step.z;
    for (j, (other, other_body, alive)) in bodies.iter().enumerate() {
        if i == j || !alive {
            continue;
        }
        let away = Vec3::new(nx - other.x, 0.0, nz - other.z);
        let gap = body + other_body + 8.0;
        if away.length() < gap && away.length() > 0.1 {
            let push = away.normalize() * gap;
            nx = other.x + push.x;
            nz = other.z + push.z;
        }
    }
    if let Some(fy) = col.floor_y(nx, nz, pos.y + body + 16.0) {
        let drop = (pos.y - center) - fy;
        if drop < body.max(40.0) {
            let next = Vec3::new(nx, fy + center, nz);
            let (pushed, _) = col.depenetrate(next, body);
            return pushed;
        }
    }
    pos
}

/// An empty magazine with rounds left in reserve reloads on its own.
fn should_auto_reload(ammo: u16, reserve: u16) -> bool {
    ammo == 0 && reserve > 0
}

fn place_hud(widget: &HudWidget, fov: f32, aspect: f32) -> Transform {
    let z = 0.56;
    let half_h = z * (fov * 0.5).tan().max(0.15);
    let view_h = half_h * 2.0;
    let width = if widget.cover {
        widget.sh * aspect.max(0.5)
    } else {
        widget.sw
    };
    Transform::from_translation(Vec3::new(widget.sx * view_h, widget.sy * view_h, -z))
        .with_scale(Vec3::new(width * view_h, widget.sh * view_h, 1.0))
}

fn can_reload(weapon: u8, ammo: [u16; 2], reserve: [u16; 2]) -> bool {
    let w = weapon as usize;
    ammo[w] < MAG_SIZE[w] && reserve[w] > 0
}

fn finish_reload(weapon: u8, ammo: &mut [u16; 2], reserve: &mut [u16; 2]) {
    let w = weapon as usize;
    let room = MAG_SIZE[w].saturating_sub(ammo[w]);
    let take = room.min(reserve[w]);
    ammo[w] = ammo[w].saturating_add(take);
    reserve[w] = reserve[w].saturating_sub(take);
}

fn wave_plan(wave: u32) -> Vec<u8> {
    match wave {
        1 => vec![0, 1, 0, 1],
        2 => vec![0, 1, 2, 1, 0],
        3 => vec![1, 2, 3, 0, 1],
        4 => vec![2, 3, 4, 5, 1],
        _ => {
            let mut plan = vec![0, 1, 2, 3, 4, 5, 6];
            let extra = wave.saturating_sub(4).min(4);
            for i in 0..extra {
                plan.push((i % 3) as u8);
            }
            plan
        }
    }
}

#[cfg(test)]
fn should_chase(dist: f32, attack: f32) -> bool {
    dist > attack
}

fn line_of_sight(col: &Collider, from: Vec3, to: Vec3, dist: f32) -> bool {
    if dist < 1.0 {
        return true;
    }
    let dir = (to - from).normalize_or_zero();
    match col.raycast(from, dir, dist) {
        None => true,
        Some(t) => t > dist - 16.0,
    }
}

pub(crate) fn muzzle_flashes(
    time: Res<Time>,
    mut commands: Commands,
    cam: Query<&Transform, With<Player>>,
    mut flashes: Query<
        (
            Entity,
            &mut MuzzleFlash,
            &mut Transform,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<Player>,
    >,
    mut beams: Query<(Entity, &mut ShotBeam)>,
) {
    let dt = time.delta_secs();
    let cam_pos = cam.single().ok().map(|t| t.translation);
    for (entity, mut beam) in &mut beams {
        beam.life -= dt;
        if beam.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, mut flash, mut tf, mut mat) in &mut flashes {
        flash.life -= dt;
        if flash.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let n = flash.frames.len();
        if n > 0 {
            let played = 1.0 - (flash.life / flash.duration).clamp(0.0, 1.0);
            let idx = ((played * n as f32) as usize).min(n - 1);
            mat.0 = flash.frames[idx].clone();
        }
        if flash.world {
            let Some(cam_pos) = cam_pos else {
                continue;
            };
            let pos = tf.translation;
            if pos.distance_squared(cam_pos) > 4.0 {
                *tf = Transform::from_translation(pos)
                    .looking_at(cam_pos, Vec3::Y)
                    .with_scale(Vec3::splat(flash.scale));
            }
        }
    }
}

fn spawn_view_flash(
    commands: &mut Commands,
    bank: &MuzzleBank,
    player: Entity,
    frames: Vec<Handle<StandardMaterial>>,
) {
    let Some(first) = frames.first().cloned() else {
        return;
    };
    commands.entity(player).with_children(|parent| {
        parent.spawn((
            MuzzleFlash {
                life: 0.12,
                duration: 0.12,
                frames,
                world: false,
                scale: 1.05,
            },
            Mesh3d(bank.mesh.clone()),
            MeshMaterial3d(first),
            Transform::from_xyz(0.36, -0.20, -1.95).with_scale(Vec3::splat(1.05)),
        ));
    });
}

fn spawn_world_flash(commands: &mut Commands, bank: &MuzzleBank, pos: Vec3, scale: f32, life: f32) {
    let frames = bank.enemy.clone();
    let Some(first) = frames.first().cloned() else {
        return;
    };
    commands.spawn((
        LevelEntity,
        MuzzleFlash {
            life,
            duration: life,
            frames,
            world: true,
            scale,
        },
        Mesh3d(bank.mesh.clone()),
        MeshMaterial3d(first),
        Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
    ));
}

fn spawn_shot_beam(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    from: Vec3,
    aim: Vec3,
    end_t: f32,
) {
    let len = end_t.max(0.0);
    if len < 24.0 {
        return;
    }
    let dir = aim.normalize_or_zero();
    if dir.length_squared() < 0.5 {
        return;
    }
    let mid = from + dir * (len * 0.5);
    let rot = Quat::from_rotation_arc(Vec3::Y, dir);
    commands.spawn((
        LevelEntity,
        ShotBeam { life: 0.14 },
        Mesh3d(meshes.add(Cuboid::new(1.4, len, 1.4))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.62, 0.15),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        })),
        Transform::from_translation(mid).with_rotation(rot),
    ));
}

pub(crate) fn hud(
    player: Query<(&Player, &Projection)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut widgets: Query<(&HudWidget, &mut Transform)>,
    mut pips: Query<(&HealthPip, &MeshMaterial3d<StandardMaterial>)>,
    mut ammo: Query<(&AmmoPip, &MeshMaterial3d<StandardMaterial>), Without<HealthPip>>,
    lamp: Query<&MeshMaterial3d<StandardMaterial>, (With<FormLamp>, Without<HealthPip>, Without<AmmoPip>)>,
    mut veil: Query<(&MeshMaterial3d<StandardMaterial>, &mut Visibility), With<HurtVeil>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((player, proj)) = player.single() else {
        return;
    };
    let fov = match proj {
        Projection::Perspective(p) => p.fov,
        _ => 75.0_f32.to_radians(),
    };
    let aspect = windows
        .single()
        .map(|window| window.width() / window.height().max(1.0))
        .unwrap_or(16.0 / 9.0);
    for (widget, mut tf) in &mut widgets {
        *tf = place_hud(widget, fov, aspect);
    }
    for (pip, handle) in &mut pips {
        let Some(mut mat) = materials.get_mut(&handle.0) else {
            continue;
        };
        let lit = player.health > pip.0 as f32 * (PLAYER_HP / 10.0);
        mat.base_color = if lit {
            Color::srgb(0.3, 0.9, 0.35)
        } else {
            Color::srgb(0.18, 0.18, 0.18)
        };
    }
    let slot = player.weapon as usize;
    let frac = player.ammo[slot] as f32 / MAG_SIZE[slot] as f32;
    for (pip, handle) in &mut ammo {
        let Some(mut mat) = materials.get_mut(&handle.0) else {
            continue;
        };
        let lit = frac > pip.0 as f32 / 8.0;
        mat.base_color = if player.reloading > 0.0 {
            Color::srgb(0.85, 0.55, 0.15)
        } else if lit {
            Color::srgb(0.95, 0.78, 0.25)
        } else {
            Color::srgb(0.18, 0.18, 0.18)
        };
    }
    if let Ok(handle) = lamp.single() {
        if let Some(mut mat) = materials.get_mut(&handle.0) {
            mat.base_color = if player.scale > 0.5 {
                Color::srgb(0.95, 0.45, 0.15)
            } else {
                Color::srgb(0.3, 0.85, 0.4)
            };
        }
    }
    let Ok((handle, mut vis)) = veil.single_mut() else {
        return;
    };
    let alpha = if player.health <= 0.0 {
        0.5
    } else {
        (player.hurt * 1.4).clamp(0.0, 0.22)
    };
    *vis = if alpha > 0.02 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if let Some(mut mat) = materials.get_mut(&handle.0) {
        mat.base_color = Color::srgba(0.75, 0.04, 0.04, alpha);
    }
}

fn marksman_dir(dir: Vec3, rng: Option<&mut ShotRng>) -> Vec3 {
    let Some(rng) = rng else {
        return dir.normalize_or_zero();
    };
    let span = MARKSMAN_LO + rng.unit() * (MARKSMAN_HI - MARKSMAN_LO);
    let kick = span * MARKSMAN_SCALE;
    let right = dir.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(dir).normalize_or_zero();
    (dir + right * rng.signed() * kick + up * rng.signed() * kick).normalize_or_zero()
}

fn spread_dir(dir: Vec3, spread: f32, rng: Option<&mut ShotRng>) -> Vec3 {
    if spread <= 0.0 {
        return dir.normalize_or_zero();
    }
    let Some(rng) = rng else {
        return dir.normalize_or_zero();
    };
    let right = dir.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(dir).normalize_or_zero();
    (dir + right * rng.signed() * spread + up * rng.signed() * spread).normalize_or_zero()
}

impl ShotRng {
    fn unit(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = if x == 0 { 1 } else { x };
        self.0 as f32 / u32::MAX as f32
    }

    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = -b - s;
    if t0 > 0.05 {
        return Some(t0);
    }
    let t1 = -b + s;
    (t1 > 0.05).then_some(t1)
}

fn anchors(col: &Collider, player: Vec3) -> Vec<Vec3> {
    // The first probes stay on the street ahead of the start, where a body
    // can actually stand. Later probes spread through the rest of the yard.
    const SPOTS: [(f32, f32); 12] = [
        (0.0, -640.0),
        (260.0, -640.0),
        (0.0, -860.0),
        (260.0, -860.0),
        (600.0, -800.0),
        (1_000.0, -200.0),
        (1_400.0, -800.0),
        (1_400.0, 200.0),
        (1_800.0, -400.0),
        (2_000.0, -200.0),
        (800.0, 200.0),
        (2_000.0, 400.0),
    ];
    let mut out = Vec::new();
    for (dx, dz) in SPOTS {
        if out.len() == 12 {
            break;
        }
        let Some(pos) = stand_on(
            col,
            player.x + dx,
            player.z + dz,
            player.y,
            EnemyKind::Trooper.def(),
        ) else {
            continue;
        };
        if pos.distance(player) < MIN_SPAWN {
            continue;
        }
        if (pos.y - player.y).abs() > 96.0 {
            continue;
        }
        if out.iter().any(|p: &Vec3| p.distance(pos) < 200.0) {
            continue;
        }
        out.push(pos);
    }
    if out.is_empty() {
        out.push(Vec3::new(player.x, player.y, player.z - 700.0));
    }
    out
}

fn stand_on(col: &Collider, x: f32, z: f32, hint_y: f32, def: &EnemyDef) -> Option<Vec3> {
    let center_h = def.height * 0.5;
    let fy = col.floor_y(x, z, hint_y + 96.0)?;
    let center = Vec3::new(x, fy + center_h, z);
    let (pushed, _) = col.depenetrate(center, def.body_radius);
    if pushed.distance_squared(center) > (def.body_radius * 0.75).powi(2) {
        return None;
    }
    let fy = col
        .floor_y(pushed.x, pushed.z, fy + center_h + 48.0)
        .unwrap_or(fy);
    Some(Vec3::new(pushed.x, fy + center_h, pushed.z))
}

fn load_sprite_frames(
    root: Option<&std::path::Path>,
    spr_rel: &str,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<Handle<StandardMaterial>> {
    let Some(root) = root else {
        return Vec::new();
    };
    let Some(path) = resolve_ci(root, spr_rel) else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let mut frames = Vec::new();
    for rel in sprite_paths(&bytes) {
        let Some(image) = load_dtx(root, &rel) else {
            continue;
        };
        frames.push(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            double_sided: true,
            cull_mode: None,
            ..default()
        }));
    }
    frames
}

fn sprite_paths(data: &[u8]) -> Vec<String> {
    if data.len() < 22 {
        return Vec::new();
    }
    let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if n == 0 || n > 64 {
        return Vec::new();
    }
    let mut p = 20usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if p + 2 > data.len() {
            break;
        }
        let len = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
        p += 2;
        if len == 0 || p + len > data.len() {
            break;
        }
        let path = String::from_utf8_lossy(&data[p..p + len]).replace('\\', "/");
        p += len;
        out.push(path);
    }
    out
}

fn sprite_mesh(model: &shogo_assets::AbcModel) -> Mesh {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in &model.positions {
        let g = lt_model(*p);
        min = min.min(g);
        max = max.max(g);
    }
    let size = (max - min).max(Vec3::splat(1.0));
    let hx = size.x * 0.5;
    let hy = size.y * 0.5;
    let positions = vec![
        [-hx, -hy, 0.0],
        [hx, -hy, 0.0],
        [hx, hy, 0.0],
        [-hx, hy, 0.0],
    ];
    let uvs = vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh.compute_area_weighted_normals();
    mesh
}

fn gun_bundle(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    gun: OrientedGun,
    slot: u8,
    hold: Vec3,
    visibility: Visibility,
) -> impl Bundle {
    (
        ViewGun(slot),
        Mesh3d(meshes.add(gun.mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: if gun.skin.is_some() {
                Color::WHITE
            } else {
                gun.tint
            },
            base_color_texture: gun.skin,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        })),
        Transform::from_translation(hold)
            .with_rotation(gun.rotation)
            .with_scale(Vec3::splat(gun.scale)),
        visibility,
    )
}

struct OrientedGun {
    mesh: Mesh,
    skin: Option<Handle<Image>>,
    tint: Color,
    rotation: Quat,
    scale: f32,
}

fn view_gun(
    root: Option<&std::path::Path>,
    model_rel: &str,
    skin_rel: &str,
    fallback: Color,
    images: &mut Assets<Image>,
) -> OrientedGun {
    let skin = root
        .and_then(|r| load_dtx(r, skin_rel))
        .map(|image| images.add(image));
    let Some(model) = load_abc(root, model_rel) else {
        return OrientedGun {
            mesh: Mesh::from(Cuboid::new(0.15, 0.12, 0.7)),
            skin: None,
            tint: fallback,
            rotation: Quat::IDENTITY,
            scale: 1.0,
        };
    };
    let (mesh, rotation, girth) = orient_view_mesh(&model);
    OrientedGun {
        mesh,
        skin,
        tint: fallback,
        rotation,
        scale: 0.32 / girth.max(0.05),
    }
}

fn orient_view_mesh(model: &shogo_assets::AbcModel) -> (Mesh, Quat, f32) {
    let mut pts: Vec<Vec3> = model.positions.iter().copied().map(lt_model).collect();
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in &pts {
        min = min.min(*p);
        max = max.max(*p);
    }
    let ext = max - min;
    let center = (min + max) * 0.5;
    for p in &mut pts {
        *p -= center;
    }
    let (axis, girth) = if ext.y >= ext.x && ext.y >= ext.z {
        (1, ext.x.max(ext.z))
    } else if ext.x >= ext.z {
        (0, ext.y.max(ext.z))
    } else {
        (2, ext.x.max(ext.y))
    };
    let rotation = match axis {
        1 => Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        0 => Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        _ => Quat::from_rotation_y(std::f32::consts::PI),
    };
    let positions: Vec<[f32; 3]> = pts.into_iter().map(|p| [p.x, p.y, p.z]).collect();
    let uvs: Vec<[f32; 2]> = model.uvs.iter().map(|[u, v]| [*u, 1.0 - v]).collect();
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(model.indices.clone()));
    mesh.compute_area_weighted_normals();
    (mesh, rotation, girth)
}

fn character_mesh(model: &shogo_assets::AbcModel, height: f32) -> Mesh {
    let pts: Vec<Vec3> = model.positions.iter().copied().map(character_pos).collect();
    let min_y = pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let max_y = pts.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let span = (max_y - min_y).max(1.0);
    let scale = height / span;
    // Plant the feet under the body. A gun or a shoulder pad must not drag
    // the stance off to one side.
    let foot_y = min_y + span * 0.18;
    let mut fx = 0.0;
    let mut fz = 0.0;
    let mut fn_ = 0.0;
    for p in &pts {
        if p.y <= foot_y {
            fx += p.x;
            fz += p.z;
            fn_ += 1.0;
        }
    }
    let (cx, cz) = if fn_ > 0.0 {
        (fx / fn_, fz / fn_)
    } else {
        (0.0, 0.0)
    };
    let positions: Vec<[f32; 3]> = pts
        .into_iter()
        .map(|p| {
            let v = Vec3::new(p.x - cx, p.y - min_y, p.z - cz) * scale;
            [v.x, v.y, v.z]
        })
        .collect();
    let uvs: Vec<[f32; 2]> = model.uvs.iter().map(|[u, v]| [*u, 1.0 - v]).collect();
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(model.indices.clone()));
    mesh.compute_area_weighted_normals();
    mesh
}

fn character_pos(p: [f32; 3]) -> Vec3 {
    // The decoder already applied the bind pose, and that pose stands on Y.
    // A Z-up rotation here lays the assembled figure on its side.
    Vec3::new(p[0], p[1], p[2])
}

fn lt_model(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], -p[1])
}

fn load_abc(root: Option<&std::path::Path>, rel: &str) -> Option<shogo_assets::AbcModel> {
    let path = root.and_then(|r| resolve_ci(r, rel))?;
    let bytes = std::fs::read(&path).ok()?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".into());
    shogo_assets::abc::decode(&bytes, &name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_in_front_is_hit_and_one_behind_is_not() {
        let origin = Vec3::ZERO;
        let dir = Vec3::NEG_Z;
        let t = ray_sphere(origin, dir, Vec3::new(0.0, 0.0, -10.0), 2.0).unwrap();
        assert!((t - 8.0).abs() < 1e-3);
        assert!(ray_sphere(origin, dir, Vec3::new(0.0, 0.0, 10.0), 2.0).is_none());
    }

    #[test]
    fn sprite_lists_each_frame_path() {
        let data = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../extracted/SHOGO/SPRITES/ASSAULTRIFLEMUZZLEFLASH.SPR",
        ));
        let Ok(data) = data else {
            return;
        };
        let paths = super::sprite_paths(&data);
        assert_eq!(paths.len(), 4);
        assert!(paths[0].to_ascii_lowercase().ends_with("asslt1b.dtx"));
        assert!(paths[3].to_ascii_lowercase().ends_with("asslt4b.dtx"));
    }

    #[test]
    fn trooper_dies_to_six_rifle_rounds() {
        let hp = EnemyKind::Trooper.def().health;
        assert!((hp - ASSAULT_DAMAGE * 6.0) <= 0.0);
        assert!((hp - ASSAULT_DAMAGE * 4.0) > 0.0);
        assert!(EnemyKind::Raksha.def().health > hp * 3.0);
        assert!(PLAYER_HP > EnemyKind::Raksha.def().health);
    }

    #[test]
    fn kinds_stop_inside_their_own_range() {
        for kind in KINDS {
            let def = kind.def();
            assert!(def.attack_distance < def.sight);
            assert!(def.attack_distance > MIN_SPAWN * 0.2);
            assert!(should_chase(def.attack_distance + 1.0, def.attack_distance));
            assert!(!should_chase(def.attack_distance, def.attack_distance));
            assert!(def.fire_interval > 0.0);
            assert!(def.move_speed > 0.0);
        }
        assert!(EnemyKind::Shock.def().health > EnemyKind::Trooper.def().health);
        assert!(EnemyKind::Raksha.def().attack_distance > EnemyKind::Trooper.def().attack_distance);
        assert_eq!(kind_from_class("FALLEN_Andra10"), EnemyKind::Andra10);
        assert_eq!(kind_from_class("CRONIAN_Trooper"), EnemyKind::Trooper);
        assert!(is_ally("UCA_Raksha"));
        assert!(!is_ally("CRONIAN_Trooper"));
    }

    #[test]
    fn street_spots_stand_on_the_spawn_floor() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../extracted/SHOGO/WORLDS/MULTI/MCA_MARITROPA2.DAT");
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(_) => return,
        };
        let world = shogo_assets::dat::decode(&data).unwrap();
        let mut tris = Vec::new();
        for group in &world.groups {
            let name = world
                .textures
                .get(group.texture as usize)
                .map(|s| s.as_str())
                .unwrap_or("");
            if name
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(name)
                .eq_ignore_ascii_case("sky.dtx")
            {
                continue;
            }
            for tri in group.indices.chunks_exact(3) {
                let p = |i: u32| {
                    let q = group.positions[i as usize];
                    Vec3::new(q[0], q[1], q[2])
                };
                tris.push([p(tri[0]), p(tri[1]), p(tri[2])]);
            }
        }
        let col = crate::collide::Collider::from_tris(tris);
        let start = shogo_assets::dat::start_points(&data)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let feet = Vec3::new(start.pos[0], start.pos[1], start.pos[2]);
        let floor = col.floor_y(feet.x, feet.z, feet.y + 128.0).unwrap();
        let player = Vec3::new(feet.x, floor + 32.0, feet.z);
        let mut hits = Vec::new();
        for dist in [620.0_f32, 820.0, 1040.0, 1280.0] {
            for side in [0.0_f32, 120.0, -120.0] {
                if let Some(pos) = stand_on(&col, player.x + side, player.z - dist, player.y, EnemyKind::Trooper.def())
                {
                    if pos.distance(player) > 500.0 {
                        hits.push(pos);
                    }
                }
            }
        }
        assert!(
            hits.len() >= 4,
            "player {player:?} only landed {} spots: {hits:?}",
            hits.len()
        );
        let found = anchors(&col, player);
        assert!(found.len() >= 8, "anchors {found:?}");
        assert!(
            found.iter().any(|p| p.x > player.x + 1_400.0),
            "anchors {found:?}"
        );
    }

    #[test]
    fn reload_fills_from_reserve_and_wave_three_includes_a_mech() {
        let mut ammo = [10, 8];
        let mut reserve = [90, 24];
        finish_reload(0, &mut ammo, &mut reserve);
        assert_eq!(ammo[0], MAG_SIZE[0]);
        assert_eq!(reserve[0], 70);
        assert!(!can_reload(0, ammo, reserve));
        assert!(should_auto_reload(0, 12));
        assert!(!should_auto_reload(8, 12));
        assert!(!should_auto_reload(0, 0));
        let plan = wave_plan(4);
        assert!(plan.contains(&(EnemyKind::Raksha as u8)));
        assert!(plan.contains(&(EnemyKind::Ruin as u8)));
        assert!(wave_plan(2).len() > 4);
        assert!(wave_plan(5).len() > wave_plan(3).len());
        assert!(DEFS.iter().all(|def| PLAYER_HP > def.health));
    }
}
