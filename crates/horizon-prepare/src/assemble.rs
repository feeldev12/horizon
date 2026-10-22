//! Preparing without Java (Nerion's fork): a phone has no JDK, and every player of a version would
//! compute the same tables from Mojang's code anyway.
//!
//! - [`pack_facts`] (on a computer, after a full `prepare`): the computed tables, everything the
//!   Java exporters produced, into one zip. Nothing copied verbatim from Mojang's files goes in:
//!   not the client jar's `assets/` and `data/`, not the world generator's structures and JSON
//!   bundles, not the block tags or the asset index.
//! - [`assemble`] (on the phone): that zip, plus what the player's own client jar and the
//!   launcher's asset store hold, into the same resources folder `prepare` writes.

use crate::pyjson::{self, Value};
use crate::{
    artifacts, json_resources, member_path, mkdir, open_zip, write_if_changed, write_json,
    write_text, zip_bytes, zip_names, Log, WORLDGEN_BUNDLES,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The block tags `prepare` bundles from the jars.
const BLOCK_TAGS: &str = "data/minecraft/tags/block/";
/// The world generator's structure templates in the jars.
const STRUCTURES: &str = "data/minecraft/structure/";

/// Whether `relative` (a resources path with `/`) is Mojang's file or a jar copy, rather than a
/// computed table: it stays out of the facts zip, and [`assemble`] writes it from the jar.
fn from_jar(relative: &str) -> bool {
    let bundle = |name: &str| {
        WORLDGEN_BUNDLES
            .iter()
            .any(|(bundle, _)| name == format!("worldgen/{bundle}.json"))
    };
    relative.starts_with("assets/")
        || relative.starts_with("data/")
        || relative.starts_with("worldgen/structure/")
        // The exporters' compiled classes and logs: build leftovers.
        || relative.starts_with("classes/")
        || relative.starts_with("client-classes/")
        || relative.ends_with(".log")
        || relative == "block-tags.json"
        || relative == "asset-index.json"
        || bundle(relative)
}

/// Zip the computed tables of the prepared `resources` into `out`. Returns how many files went in.
pub fn pack_facts(resources: &Path, out: &Path) -> Result<usize, String> {
    let mut files = Vec::new();
    collect(resources, resources, &mut files)?;
    files.sort();
    let file = fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0;
    for relative in files.iter().filter(|r| !from_jar(r)) {
        let data = fs::read(resources.join(relative)).map_err(|e| format!("{relative}: {e}"))?;
        zip.start_file(relative.as_str(), options)
            .map_err(|e| format!("{relative}: {e}"))?;
        zip.write_all(&data)
            .map_err(|e| format!("{relative}: {e}"))?;
        count += 1;
    }
    zip.finish()
        .map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(count)
}

/// Every file under `dir`, relative to `root` with `/` separators.
fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect(root, &path, files)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            let parts: Vec<_> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            files.push(parts.join("/"));
        }
    }
    Ok(())
}

/// What [`assemble`] builds from.
pub struct Sources<'a> {
    /// The facts zip [`pack_facts`] wrote, for this version.
    pub facts: &'a Path,
    /// The player's client jar of the version (read as a zip; nothing in it runs).
    pub client_jar: &'a Path,
    /// The launcher's asset index of the version (`<assets>/indexes/<id>.json`, objects beside
    /// it): the title panorama and the Unifont glyphs, which the jar only holds placeholders for.
    pub asset_index: Option<&'a Path>,
}

/// Build the resources folder `out` (what `prepare` writes) from `sources`, without Java.
pub fn assemble(sources: &Sources<'_>, out: &Path, log: Log) -> Result<(), String> {
    mkdir(out)?;
    log("Unpacking the computed tables");
    let mut facts = open_zip(sources.facts)?;
    for name in zip_names(&facts) {
        if name.ends_with('/') {
            continue;
        }
        if from_jar(&name) {
            return Err(format!("{name} in the facts zip should come from the jar"));
        }
        let relative = member_path(&name, "Invalid path in the facts zip")?;
        write_if_changed(&out.join(relative), &zip_bytes(&mut facts, &name)?)?;
    }

    log("Copying the client's assets and data");
    let mut client = open_zip(sources.client_jar)?;
    for name in zip_names(&client) {
        if !(name.starts_with("assets/") || name.starts_with("data/")) || name.ends_with('/') {
            continue;
        }
        let relative = member_path(&name, "Invalid resource path")?;
        write_if_changed(&out.join(relative), &zip_bytes(&mut client, &name)?)?;
    }
    write_json(
        &out.join("block-tags.json"),
        &json_resources(&mut client, BLOCK_TAGS)?,
    )?;

    log("Writing the world generator's bundle");
    let worldgen = out.join("worldgen");
    mkdir(&worldgen)?;
    for name in zip_names(&client) {
        if name.starts_with(STRUCTURES) && name.ends_with(".nbt") {
            let relative = member_path(&name[STRUCTURES.len()..], "Invalid structure path")?;
            let data = zip_bytes(&mut client, &name)?;
            write_if_changed(&worldgen.join("structure/minecraft").join(relative), &data)?;
        }
    }
    for (name, path) in WORLDGEN_BUNDLES {
        let resources = json_resources(&mut client, &format!("data/minecraft/{path}/"))?;
        if resources.as_object().is_none_or(<[_]>::is_empty) {
            return Err(format!("The client jar has no worldgen resources: {path}"));
        }
        write_text(
            &worldgen.join(format!("{name}.json")),
            &pyjson::dumps(&resources, &pyjson::COMPACT),
        )?;
    }

    if let Some(index) = sources.asset_index {
        log("Copying the title panorama and fonts");
        index_assets(index, out)?;
    }
    Ok(())
}

/// The asset index beside the resources, and the objects the jar only has placeholders for: the
/// title panorama and the fonts (`minecraft/font/`, Unifont), each checked against its SHA-1.
fn index_assets(index: &Path, out: &Path) -> Result<(), String> {
    let objects_dir = index
        .parent()
        .and_then(Path::parent)
        .map(|assets| assets.join("objects"))
        .ok_or_else(|| format!("{} is not in an assets/indexes folder", index.display()))?;
    fs::copy(index, out.join("asset-index.json"))
        .map_err(|e| format!("{}: {e}", index.display()))?;
    let text = fs::read_to_string(index).map_err(|e| format!("{}: {e}", index.display()))?;
    let parsed = pyjson::loads(&text)?;
    let objects = parsed
        .get("objects")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{} has no objects", index.display()))?;
    for (name, info) in objects {
        let wanted = name.starts_with("minecraft/textures/gui/title/background/panorama_")
            || name.starts_with("minecraft/font/");
        if !wanted {
            continue;
        }
        let hash = info
            .get("hash")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{name} has no hash in the asset index"))?;
        let relative = artifacts::safe_relative(name)
            .ok_or_else(|| format!("Unsafe asset name {name:?} in the asset index"))?;
        let source: PathBuf = objects_dir.join(hash.get(..2).unwrap_or("")).join(hash);
        artifacts::import_copy(
            &source,
            &out.join("assets").join(relative),
            hash,
            "sha1",
            "",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::from_jar;

    #[test]
    fn jar_copies_stay_out_of_the_facts() {
        for path in [
            "assets/minecraft/textures/block/stone.png",
            "data/minecraft/recipe/stick.json",
            "worldgen/structure/minecraft/igloo/top.nbt",
            "worldgen/biomes.json",
            "block-tags.json",
            "asset-index.json",
            "classes/VanillaHeightmapOracle.class",
            "export.log",
        ] {
            assert!(from_jar(path), "{path}");
        }
        for path in [
            "facts.json",
            "horizon-resources.json",
            "worldgen/block_flags.json",
            "worldgen/biome_parameters/overworld.json",
            "worldgen/blocks/survival.json",
            "biome-parameters/overworld.json",
            "occlusion.nbt",
        ] {
            assert!(!from_jar(path), "{path}");
        }
    }
}
