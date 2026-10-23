//! Builds in the audio Bibles' catalogues: every data/audio/<recording>.json (who reads
//! it, its licence, and each chapter's verse timings), and what each recording is
//! downloaded as: data/audio/downloads/<recording>.json (each chapter's size and SHA-256).
//! The recordings themselves are downloaded when the reader asks (or ship beside the app).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// `item`: the texts of the *.json files in `dir` (not in its subfolders), in name order.
fn list(doc: &str, item: &str, dir: &Path) -> String {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut code = format!("{}\npub static {}: &[&str] = &[\n", doc, item);
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        code.push_str(&format!("    include_str!({:?}),\n", f.display().to_string()));
    }
    code.push_str("];\n");
    code
}

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../data/audio");
    let mut code = list("/// Each recording's catalogue, as data/audio/<recording>.json has it.", "CATALOGUES", &dir);
    code.push_str(&list(
        "/// What each recording is downloaded as, as data/audio/downloads/<recording>.json has it.",
        "DOWNLOADS",
        &dir.join("downloads"),
    ));
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("audio_catalogues.rs"), code).unwrap();
}
