//! Builds in the audio Bibles' catalogues: every data/audio/<recording>.json (who reads
//! it, its licence, and each chapter's verse timings). The recordings themselves ship
//! beside the app.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../data/audio");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut code = String::from("/// Each recording's catalogue, as data/audio/<recording>.json has it.\npub static CATALOGUES: &[&str] = &[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        code.push_str(&format!("    include_str!({:?}),\n", f.display().to_string()));
    }
    code.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("audio_catalogues.rs"), code).unwrap();
}
