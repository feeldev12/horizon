//! Where a sound file is: in the prepared resources (`<root>/assets/<namespace>/<path>`, what
//! `horizon-prepare sounds` copies), or else in a launcher's asset store, by the version's asset
//! index (`--asset-index`, `indexes/<id>.json` beside `objects/`). Nerion's launcher already holds
//! that store for the version, so the game reads the sounds from it instead of a second copy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The asset index's objects by name (`minecraft/sounds/...` -> its file in `objects/`).
static INDEX: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();

/// Read the launcher's asset index at `path` (once, at start). Unreadable: the game keeps to its
/// prepared resources.
pub fn set_index(path: &Path) {
    let Some(objects) = path
        .parent()
        .and_then(Path::parent)
        .map(|a| a.join("objects"))
    else {
        return;
    };
    let parsed = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let Some(entries) = parsed
        .as_ref()
        .and_then(|v| v.get("objects"))
        .and_then(|o| o.as_object())
    else {
        log::warn!("Horizon: asset index {} is unreadable", path.display());
        return;
    };
    let files = entries
        .iter()
        .filter_map(|(name, object)| {
            let hash = object.get("hash")?.as_str()?;
            Some((name.clone(), objects.join(hash.get(..2)?).join(hash)))
        })
        .collect();
    let _ = INDEX.set(files);
}

/// `namespace:path`'s file: the prepared copy if there is one, else the asset store's.
pub fn resolve(root: &Path, namespace: &str, path: &str) -> Option<PathBuf> {
    let prepared = root.join("assets").join(namespace).join(path);
    if prepared.is_file() {
        return Some(prepared);
    }
    INDEX
        .get()?
        .get(&format!("{namespace}/{path}"))
        .filter(|file| file.is_file())
        .cloned()
}
