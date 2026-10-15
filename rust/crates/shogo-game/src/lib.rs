//! Gameplay rewrite for the Shogo player loop.
//!
//! Components stay flat: a player, a hostile volume, a bolt. Systems read
//! [`crate::input::Actions`] and write motion. Original `.ABC` and `.DTX`
//! bytes go through `shogo-assets`; this crate does not load a converted
//! glTF or PNG at runtime.

mod input;
mod pad;
mod physics;
mod player;
mod stage;
mod targeting;
mod ui;
mod weapons;

use bevy::prelude::*;

pub struct ShogoGamePlugin;

impl Plugin for ShogoGamePlugin {
    fn build(&self, app: &mut App) {
        let boot = if std::env::var("SHOGO_SKIP_MENU").ok().as_deref() == Some("1") {
            ui::Phase::Play
        } else {
            ui::Phase::Menu
        };
        app.insert_state(boot)
            .init_resource::<input::Actions>()
            .init_resource::<input::LookConfig>()
            .init_resource::<pad::Pads>()
            .init_resource::<player::Sim>()
            .init_resource::<player::Fresh>()
            .init_resource::<targeting::LockOn>()
            .init_resource::<targeting::Reticle>()
            .init_resource::<ui::Wheel>()
            .init_resource::<ui::Focus>()
            .init_resource::<ui::SettingsBack>()
            .init_resource::<UiScale>()
            .add_systems(Startup, (stage::spawn_world, ui::spawn_ui))
            .add_systems(OnEnter(ui::Phase::Play), stage::reset_if_fresh)
            .add_systems(
                Update,
                (
                    input::gather,
                    input::grab_cursor,
                    ui::drive,
                    ui::scale_to_window,
                    targeting::acquire,
                    player::simulate,
                    weapons::tick,
                    ui::hud,
                )
                    .chain(),
            );
    }
}
