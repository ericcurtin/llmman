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

/// Returns true if `reference` already carries an explicit registry host
/// (i.e. the first path component contains a dot or equals "localhost").
fn has_host(reference: &str) -> bool {
    let first = reference.split('/').next().unwrap_or("");
    first.contains('.') || first.eq_ignore_ascii_case("localhost")
}

/// Resolve `reference` through the short-name alias table, then default the
/// registry to `hf.co` when no host is present.
///
/// Resolution order:
/// 1. Exact alias match → return the mapped value
/// 2. Reference already has a host → return as-is
/// 3. No host → prepend `hf.co/`
///
/// Examples:
/// - `"qwen3.5:0.8b-q4_K_M"`              → alias → `"hf.co/unsloth/Qwen3.5-0.8B-GGUF:Q4_K_M"`
/// - `"unsloth/Qwen3.5-0.8B-GGUF:Q4_K_M"` → no host → `"hf.co/unsloth/Qwen3.5-0.8B-GGUF:Q4_K_M"`
/// - `"hf.co/unsloth/Qwen3.5-0.8B-GGUF"`  → has host → unchanged
pub fn resolve(reference: &str) -> String {
    if let Some(mapped) = aliases().get(reference) {
        return mapped.clone();
    }
    if has_host(reference) {
        return reference.to_owned();
    }
    format!("hf.co/{reference}")
}
