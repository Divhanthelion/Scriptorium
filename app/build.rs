//! Builds the data bundle from the repository's text and data files, compresses it,
//! and hands it to the app via OUT_DIR (embedded with `include_bytes!`).

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use kjv_core::bundle::DataBundle;
use sha2::{Digest, Sha256};

const SIDELOAD_SWITCH: &str = "SCRIPTORIUM_SIDELOAD_AUDIO";

/// The ids of the recording catalogues (*.json) in `dir`; none if there is no such folder.
fn catalogue_ids(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let text = fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            let parsed: serde_json::Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} parses: {e}", p.display()));
            let id = parsed["id"].as_str().unwrap_or_else(|| panic!("{} has an id", p.display()));
            id.to_string()
        })
        .collect()
}

/// A build without SCRIPTORIUM_SIDELOAD_AUDIO=1 (a store build) has only the recordings
/// catalogued in data/audio/*.json; one in data/audio/sideload/ is left out of it
/// (crates/core/build.rs). But `bundle.resources` packs whatever is in app/audio/, and an
/// Android build whatever an earlier one left in its assets: every folder there must be a
/// built-in recording's, or the build fails rather than ship files it shouldn't (a sideload
/// recording's, or one whose catalogue was moved or removed) (docs/AUDIO.md).
fn keep_sideload_audio_out(app: &Path, root: &Path) {
    println!("cargo:rerun-if-env-changed={}", SIDELOAD_SWITCH);
    if env::var(SIDELOAD_SWITCH).as_deref() == Ok("1") {
        return;
    }
    let mut folders = vec![app.join("audio")];
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        folders.push(app.join("gen/android/app/src/main/assets/audio"));
    }
    // (A path that doesn't exist would run this script on every build)
    for folder in folders.iter().filter(|f| f.exists()) {
        println!("cargo:rerun-if-changed={}", folder.display());
    }
    let built_in = catalogue_ids(&root.join("data/audio"));
    let sideload = catalogue_ids(&root.join("data/audio/sideload"));
    let mut found = Vec::new();
    for folder in folders.iter().filter(|f| f.exists()) {
        let entries = fs::read_dir(folder).unwrap_or_else(|e| panic!("read {}: {e}", folder.display()));
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !built_in.contains(&name) {
                let what = if sideload.contains(&name) {
                    "a sideload recording"
                } else {
                    "no catalogue in data/audio/"
                };
                found.push(format!("{} ({what})", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} is not 1, so this build has only the recordings catalogued in data/audio/*.json, \
         but it would pack the audio of others:\n  {}\nMove it out of the app for a store build, or set {}=1 \
         for a sideload or development build (docs/AUDIO.md).",
        SIDELOAD_SWITCH,
        found.join("\n  "),
        SIDELOAD_SWITCH
    );
}

fn main() {
    let app = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = app.join("..");
    for dir in ["old_testament", "new_testament", "data"] {
        println!("cargo:rerun-if-changed={}", root.join(dir).display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    // Before the data bundle, which takes a while
    keep_sideload_audio_out(&app, &root);

    // from_sources validates the bundle: an incomplete text fails the build here
    let bundle = DataBundle::from_sources(&root).expect("build the data bundle");
    let bytes = bundle.to_bytes().expect("serialize the data bundle");
    // Maximum compression for shipped builds; fast for development
    let release = env::var("PROFILE").as_deref() == Ok("release");
    let level = if release { 19 } else { 3 };
    let compressed = zstd::encode_all(&bytes[..], level).expect("compress the data bundle");

    // The app unpacks with ruzstd, not the zstd library that packed it: prove they agree
    let mut unpacked = Vec::with_capacity(bytes.len());
    ruzstd::decoding::StreamingDecoder::new(&compressed[..])
        .expect("the compressed bundle is valid zstd")
        .read_to_end(&mut unpacked)
        .expect("the compressed bundle decompresses");
    assert!(unpacked == bytes, "the compressed bundle does not decompress to the original bytes");

    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bundle.bin.zst");
    fs::write(&out, &compressed).expect("write the data bundle");

    // Identifies the exact data a build embeds. Not reproducible across builds:
    // the bundle's hash maps serialize in their (randomized) in-memory order.
    let hash: String = Sha256::digest(&compressed).iter().map(|b| format!("{:02x}", b)).collect();
    let summary = format!(
        "data bundle: {} bytes, zstd level {} -> {} bytes, sha256 {}",
        bytes.len(),
        level,
        compressed.len(),
        hash
    );
    // Shown in the build output for shipped builds; kept in the build script's log otherwise
    if release {
        println!("cargo:warning={}", summary);
    } else {
        println!("{}", summary);
    }

    // The library (translations, commentaries): one compressed entry per book
    println!("cargo:rerun-if-changed={}", root.join("data/library").display());
    let (library, _) = kjv_library::library::build::archive(&root, &|bytes| {
        zstd::encode_all(bytes, level).expect("compress a library entry")
    })
    .expect("build the library archive");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("library.bin");
    fs::write(&out, library).expect("write the library archive");

    tauri_build::build();
}
