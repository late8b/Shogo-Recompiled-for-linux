//! Transmission and objective HUD copy from the retail shell string table.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use shogo_assets::StringTable;

/// Loaded once at startup. An empty table means the DLL was missing; the HUD
/// then keeps `Transmission {id}` and `Objective {id}` so the miss stays visible.
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
            warn!("dialogue string table missing; transmissions and objectives stay numeric");
        }
        Self { table }
    }

    /// Resolve a script line. `Transmission {id}` and `Objective {id}` become
    /// retail copy. A missing id stays in that form. Other lines pass through.
    pub(crate) fn line(&self, raw: &str) -> String {
        if let Some(text) = self.lookup(raw, "Transmission ") {
            return text;
        }
        if let Some(text) = self.lookup(raw, "Objective ") {
            return text;
        }
        raw.to_string()
    }

    fn lookup(&self, raw: &str, prefix: &str) -> Option<String> {
        let id = raw.strip_prefix(prefix)?.parse::<i32>().ok()?;
        let label = prefix.trim_end();
        Some(match self.table.get(id) {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => format!("{label} {id}"),
        })
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
        assert_eq!(dialogue.line("The way out is locked"), "The way out is locked");
    }

    #[test]
    fn objective_1040_shows_the_retail_sentence() {
        let dialogue = Dialogue::load();
        if dialogue.table.is_empty() {
            return;
        }
        assert_eq!(
            dialogue.line("Objective 1040"),
            "Fall back to Drop Point Renegade"
        );
        assert_eq!(dialogue.line("Objective 999999"), "Objective 999999");
    }
}
