//! Builds in the audio Bibles' catalogues: every data/audio/<recording>.json (who reads
//! it, its licence, and each chapter's verse timings). The recordings themselves ship
//! beside the app.
//!
//! data/audio/sideload/ holds the catalogues of recordings kept out of store builds until
//! their terms for a published app are confirmed (docs/AUDIO.md). They are built in only
//! when SCRIPTORIUM_SIDELOAD_AUDIO=1, for a sideload or development build; the tests check
//! them either way.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SWITCH: &str = "SCRIPTORIUM_SIDELOAD_AUDIO";

/// The catalogues in `dir` (not in its subfolders), in name order.
fn catalogues(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// `item`, a list of the files' texts, under `doc`.
fn list<'a>(doc: &str, item: &str, files: impl Iterator<Item = &'a PathBuf>) -> String {
    let mut code = format!("{}\n{}: &[&str] = &[\n", doc, item);
    for f in files {
        code.push_str(&format!("    include_str!({:?}),\n", f.display().to_string()));
    }
    code.push_str("];\n");
    code
}

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../data/audio");
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rerun-if-env-changed={}", SWITCH);
    let store = catalogues(&dir);
    let sideload = catalogues(&dir.join("sideload"));
    for f in store.iter().chain(&sideload) {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    let built_in = env::var(SWITCH).as_deref() == Ok("1");
    let mut code = list(
        "/// Each recording's catalogue, as data/audio/<recording>.json has it (with\n/// data/audio/sideload/'s when SCRIPTORIUM_SIDELOAD_AUDIO=1).",
        "pub static CATALOGUES",
        store.iter().chain(sideload.iter().filter(|_| built_in)),
    );
    code.push_str("#[cfg(test)]\n");
    code.push_str(&list("/// data/audio/'s catalogues alone.", "static STORE_CATALOGUES", store.iter()));
    code.push_str("#[cfg(test)]\n");
    code.push_str(&list("/// data/audio/sideload/'s, built in or not.", "static SIDELOAD_CATALOGUES", sideload.iter()));
    code.push_str(&format!("/// Whether data/audio/sideload/'s catalogues are built in.\n#[cfg(test)]\nconst SIDELOAD_BUILT_IN: bool = {};\n", built_in));
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("audio_catalogues.rs"), code).unwrap();
}
