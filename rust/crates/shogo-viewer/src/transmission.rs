//! Transmission HUD copy from the retail shell string table.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use shogo_assets::StringTable;

/// Loaded once at startup. An empty table means the DLL was missing; the HUD
/// then keeps `Transmission {id}` so the miss stays visible.
#[derive(Resource, Clone)]
pub(crate) struct Dialogue {
    table: StringTable,
}

impl Dialogue {
    pub(crate) fn load() -> Self {
        let table = locate()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| StringTable::parse(&bytes).ok())
            .unwrap_or_default();
        if table.is_empty() {
            warn!("dialogue string table missing; transmissions stay numeric");
        }
        Self { table }
    }

    /// Resolve a script line. Only `Transmission {id}` is rewritten. Objective
    /// lines and status text pass through.
    pub(crate) fn line(&self, raw: &str) -> String {
        let Some(id) = raw
            .strip_prefix("Transmission ")
            .and_then(|rest| rest.parse::<i32>().ok())
        else {
            return raw.to_string();
        };
        match self.table.get(id) {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => format!("Transmission {id}"),
        }
    }
}

fn locate() -> Option<PathBuf> {
    if let Some(path) = std::env::var("SHOGO_STRINGS").ok().filter(|p| !p.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let baked = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../embedded/mod0_credits.dll");
    if baked.is_file() {
        return Some(baked);
    }
    let world = crate::world_path()?;
    let mut dir = Path::new(&world).parent();
    for _ in 0..8 {
        let Some(current) = dir else {
            break;
        };
        let cand = current.join("embedded/mod0_credits.dll");
        if cand.is_file() {
            return Some(cand);
        }
        dir = current.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambush_transmission_shows_retail_text() {
        let dialogue = Dialogue::load();
        if dialogue.table.is_empty() {
            return;
        }
        assert_eq!(
            dialogue.line("Transmission 3671"),
            "Recon 4, fall back to Drop Point Renegade for immediate evac. We're picking up Fallen reinforcements approaching from the southeast. Get your team clear ASAP. Over."
        );
        assert_eq!(dialogue.line("Transmission 999999"), "Transmission 999999");
        assert_eq!(dialogue.line("Objective 1040"), "Objective 1040");
        assert_eq!(dialogue.line("The way out is locked"), "The way out is locked");
    }
}
