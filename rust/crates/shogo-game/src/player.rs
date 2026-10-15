//! First-person body. The camera entity is the player; nothing here is a
//! C++ object hierarchy.
//!
//! `simulate` maps the stick wish into `physics::integrate`. That step is the
//! client move in `FUN_10012ac0` (`decompiled/mod2_CShellRef.c`), after
//! `FUN_100122e0` has packed the command bits, and the engine integral in
//! `FUN_00436b50` (`decompiled/Client.exe.c`).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::input::{Actions, LookConfig};
use crate::physics::{self, BodyTune, Kinematics, MoveWish, BODY};
use crate::targeting::{self, Hostile, LockOn, Reticle, FRICTION_CONE};
use crate::ui::Wheel;
use crate::weapons::{self, ARMOR_MAX, PLAYER_HP};

#[derive(Resource, Default)]
pub struct Sim {
    pub run: bool,
}

#[derive(Resource)]
pub struct Fresh(pub bool);

impl Default for Fresh {
    fn default() -> Self {
        Self(false)
    }
}

#[derive(Component)]
pub struct Player {
    pub yaw: f32,
    pub pitch: f32,
    pub vel: Vec3,
    pub grounded: bool,
    pub health: f32,
    pub armor: f32,
    pub scale: f32,
    pub want_mca: bool,
    pub scale_lock: f32,
    pub radius: f32,
    pub ads: f32,
    pub weapon: u8,
    pub fire_cd: f32,
    pub ammo: [u16; 2],
    pub reserve: [u16; 2],
    pub reloading: f32,
}

impl Player {
    pub fn new() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            vel: Vec3::ZERO,
            grounded: true,
            health: PLAYER_HP,
            armor: ARMOR_MAX,
            scale: 0.0,
            want_mca: false,
            scale_lock: 0.0,
            radius: BODY,
            ads: 0.0,
            weapon: 0,
            fire_cd: 0.0,
            ammo: [weapons::WEAPONS[0].mag, weapons::WEAPONS[1].mag],
            reserve: [weapons::WEAPONS[0].reserve, weapons::WEAPONS[1].reserve],
            reloading: 0.0,
        }
    }

    fn kinematics(&self) -> Kinematics {
        Kinematics {
            yaw: self.yaw,
            pitch: self.pitch,
            vel: self.vel,
            grounded: self.grounded,
            scale: self.scale,
            want_mca: self.want_mca,
            scale_lock: self.scale_lock,
            ads: self.ads,
            radius: self.radius,
        }
    }

    fn write_back(&mut self, body: &Kinematics) {
        self.yaw = body.yaw;
        self.pitch = body.pitch;
        self.vel = body.vel;
        self.grounded = body.grounded;
        self.scale = body.scale;
        self.want_mca = body.want_mca;
        self.scale_lock = body.scale_lock;
        self.ads = body.ads;
        self.radius = body.radius;
    }
}

pub fn simulate(
    time: Res<Time>,
    sim: Res<Sim>,
    actions: Res<Actions>,
    look: Res<LookConfig>,
    lock: Res<LockOn>,
    wheel: Res<Wheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut reticle: ResMut<Reticle>,
    mut players: Query<(&mut Player, &mut Transform, &mut Projection)>,
    hostiles: Query<(Entity, &Hostile, &Transform)>,
) {
    let Ok((mut player, mut tf, mut proj)) = players.single_mut() else {
        return;
    };
    if actions.weapon_slot.is_some() && sim.run {
        player.weapon = actions.weapon_slot.unwrap();
    }
    if wheel.open {
        player.weapon = wheel.slot;
    } else if actions.cycle && sim.run {
        player.weapon = (player.weapon + 1) % 2;
    }

    if !sim.run {
        reticle.ndc = Vec2::ZERO;
        reticle.locked = false;
        return;
    }

    let dt = time.delta_secs().min(0.05);
    if player.health <= 0.0 {
        player.vel = Vec3::ZERO;
        if actions.respawn {
            player.health = PLAYER_HP;
            player.armor = ARMOR_MAX;
            player.vel = Vec3::ZERO;
            player.fire_cd = 0.25;
            player.reloading = 0.0;
            player.ammo = [weapons::WEAPONS[0].mag, weapons::WEAPONS[1].mag];
            player.reserve = [weapons::WEAPONS[0].reserve, weapons::WEAPONS[1].reserve];
            tf.translation = Vec3::new(0.0, player.radius, 0.0);
        }
        reticle.locked = false;
        reticle.ndc = Vec2::ZERO;
        return;
    }

    let origin = tf.translation;
    let forward = physics::facing(player.yaw, player.pitch);
    let volumes: Vec<(Vec3, f32)> = hostiles
        .iter()
        .filter(|(_, h, _)| h.health > 0.0)
        .map(|(_, h, t)| (t.translation + Vec3::Y * h.chest, h.radius))
        .collect();
    let friction = if look.assist && lock.target.is_none() {
        targeting::aim_friction(origin, forward, &volumes, FRICTION_CONE)
    } else {
        1.0
    };
    let look_mul = if actions.lock { 0.0 } else { friction };

    let mut body = player.kinematics();
    let wish = MoveWish {
        wish: actions.wish,
        look: actions.look,
        look_mul,
        sprint: actions.sprint,
        jump: actions.jump,
        aim: actions.aim,
        toggle_body: actions.toggle_body,
    };
    let mut pos = physics::integrate(tf.translation, &mut body, &wish, dt, 0.0);
    pos.x = pos.x.clamp(-1_600.0, 1_600.0);
    pos.z = pos.z.clamp(-1_600.0, 1_600.0);

    let mut locked = false;
    let mut ndc = Vec2::ZERO;
    if actions.lock {
        if let Some(entity) = lock.target {
            if let Ok((_, hostile, htf)) = hostiles.get(entity) {
                let com = htf.translation + Vec3::Y * hostile.chest;
                let (want_yaw, want_pitch) = targeting::desired_lock(pos, com);
                let tau = if lock.snap > 0.35 { 0.06 } else { 0.14 };
                let t = 1.0 - (-dt / tau).exp();
                body.yaw = physics::lerp_angle(body.yaw, want_yaw, t);
                body.pitch += (want_pitch - body.pitch) * t;
                locked = true;
                let aspect = windows
                    .single()
                    .map(|w| w.width() / w.height().max(1.0))
                    .unwrap_or(16.0 / 9.0);
                let fov = BodyTune::at(body.scale).fov * (1.0 - 0.2 * body.ads);
                if let Some(p) = targeting::project(body.yaw, body.pitch, fov, aspect, pos, com) {
                    ndc = p;
                }
            }
        }
    }

    let tune = BodyTune::at(body.scale);
    if let Projection::Perspective(persp) = &mut *proj {
        persp.fov = tune.fov * (1.0 - 0.2 * body.ads);
    }
    tf.translation = pos;
    tf.rotation = physics::camera_rotation(body.yaw, body.pitch);
    player.write_back(&body);
    reticle.ndc = ndc;
    reticle.locked = locked;
}

pub fn spawn_player(commands: &mut Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            near: 0.5,
            far: 20_000.0,
            fov: BodyTune::at(0.0).fov,
            ..default()
        }),
        Transform::from_translation(Vec3::new(0.0, BODY, 0.0)),
        Player::new(),
    ));
}
