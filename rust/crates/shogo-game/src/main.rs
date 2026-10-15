//! Player loop: menu, HUD, on-foot / MCA motion, and lock-on.
//!
//! ```text
//! cd rust && WGPU_BACKEND=vulkan cargo run -p shogo-game
//! SHOGO_SKIP_MENU=1   boot straight into the arena
//! SHOGO_GRAB=0        leave the mouse free
//! ```

use bevy::prelude::*;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.42, 0.55, 0.68)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Shogo MAD — player loop".into(),
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(shogo_game::ShogoGamePlugin)
        .run();
}
