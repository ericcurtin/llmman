//! Short-name alias resolution from the bundled `shortnames.conf`.
//!
//! The TOML file contains an `[aliases]` table mapping short user-facing names
//! (e.g. `"qwen3.5:0.8b-q4_K_M"`) to full registry references
//! (e.g. `"hf.co/unsloth/Qwen3.5-0.8B-GGUF:Q4_K_M"`).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Deserialize)]
struct Conf {
    aliases: HashMap<String, String>,
}

static CONF: &str = include_str!("../shortnames.conf");

fn aliases() -> &'static HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        toml::from_str::<Conf>(CONF)
            .map(|c| c.aliases)
            .unwrap_or_default()
    })
}

/// Resolve `reference` through the short-name alias table.
/// Returns the full registry reference if a match is found, otherwise the
/// input unchanged.
pub fn resolve(reference: &str) -> String {
    aliases()
        .get(reference)
        .cloned()
        .unwrap_or_else(|| reference.to_owned())
}
