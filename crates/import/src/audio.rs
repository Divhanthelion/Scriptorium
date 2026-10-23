//! `audio-pack`: makes a recording's downloads from its chapter files (docs/AUDIO.md).
//!
//!     cargo run -p kjv-import -- audio-pack <recording> <release> [chapter folder]
//!
//! Reads every chapter the recording's catalogue (`data/audio/<recording>.json`) lists, as
//! `<BOOK>.<CH>.ogg`, from `app/audio/<recording>/` or the folder given, and writes:
//! - the release's files, in `target/audio-release/<release>/`: one per book,
//!   `<recording>.<BOOK>.ogg`, its chapter files end to end in order (which also plays as
//!   one chained Ogg file), with `SHA256SUMS` and the release's notes;
//! - the manifest the app builds in, `data/audio/downloads/<recording>.json`: the release,
//!   and each chapter's size and SHA-256, which the app checks every chapter against
//!   before it keeps it.
//!
//! Nothing is uploaded: the release is made by hand (docs/AUDIO.md), and its files must
//! never change once it is, so a changed recording is packed as a new release.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use kjv_library::books::BOOKS;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Catalogue {
    id: String,
    credit: String,
    source: String,
    chapters: BTreeMap<String, serde_json::Value>,
}

/// One book's file: each chapter's size and SHA-256, in order.
struct Book {
    code: &'static str,
    chapters: Vec<(u64, String)>,
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn run(args: &[String]) -> Result<(), String> {
    let usage = || "usage: kjv-import audio-pack <recording> <release> [chapter folder]".to_string();
    let recording = args.first().ok_or_else(usage)?;
    let release = args.get(1).ok_or_else(usage)?;
    let root = crate::root();
    let from = args.get(2).map(PathBuf::from).unwrap_or_else(|| root.join("app/audio").join(recording));
    let out = root.join("target/audio-release").join(release);
    let manifest = root.join("data/audio/downloads").join(format!("{}.json", recording));
    pack(&root.join("data/audio"), recording, release, &from, &out, &manifest)?;
    let shown = |p: &Path| p.strip_prefix(&root).unwrap_or(p).display().to_string().replace('\\', "/");
    println!("  release files: {}\n  manifest: {}", shown(&out), shown(&manifest));
    Ok(())
}

/// Pack `recording` (catalogued in `catalogues`) from the chapter files in `from` into
/// `out`, and write its manifest to `manifest`.
pub fn pack(catalogues: &Path, recording: &str, release: &str, from: &Path, out: &Path, manifest: &Path) -> Result<(), String> {
    let n = release.strip_prefix(&format!("audio-{}-", recording)).and_then(|n| n.parse::<u32>().ok());
    if !n.is_some_and(|n| n >= 1) {
        return Err(format!("name the release audio-{}-<n> (1, 2, …), not {}", recording, release));
    }
    let path = catalogues.join(format!("{}.json", recording));
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let catalogue: Catalogue = serde_json::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e))?;
    if catalogue.id != recording {
        return Err(format!("{} is the catalogue of {:?}", path.display(), catalogue.id));
    }

    // Every chapter the catalogue lists has its file, and every file is one of them
    let mut found = Vec::new();
    for entry in fs::read_dir(from).map_err(|e| format!("read {}: {}", from.display(), e))? {
        let name = entry.map_err(|e| e.to_string())?.file_name().to_string_lossy().into_owned();
        if let Some(key) = name.strip_suffix(".ogg") {
            found.push(key.to_string());
        }
    }
    let extra: Vec<&String> = found.iter().filter(|k| !catalogue.chapters.contains_key(*k)).collect();
    let missing: Vec<&String> = catalogue.chapters.keys().filter(|k| !found.contains(k)).collect();
    if !extra.is_empty() || !missing.is_empty() {
        return Err(format!(
            "{}: {} chapter files the catalogue doesn't list ({:?}), {} it lists missing ({:?})",
            from.display(),
            extra.len(),
            extra.iter().take(5).collect::<Vec<_>>(),
            missing.len(),
            missing.iter().take(5).collect::<Vec<_>>()
        ));
    }

    fs::create_dir_all(out).map_err(|e| format!("create {}: {}", out.display(), e))?;
    let mut books = Vec::new();
    for book in BOOKS {
        let count = catalogue.chapters.keys().filter(|k| k.split_once('.').is_some_and(|(code, _)| code == book.code)).count();
        if count == 0 {
            continue;
        }
        let asset = out.join(format!("{}.{}.ogg", recording, book.code));
        let mut file = fs::File::create(&asset).map_err(|e| format!("create {}: {}", asset.display(), e))?;
        let mut chapters = Vec::new();
        for n in 1..=count {
            let key = format!("{}.{}", book.code, n);
            if !catalogue.chapters.contains_key(&key) {
                return Err(format!("{}: {} has {} chapters but not {}", recording, book.code, count, key));
            }
            let path = from.join(format!("{}.ogg", key));
            let bytes = fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
            if !bytes.starts_with(b"OggS") {
                return Err(format!("{} is not an Ogg file", path.display()));
            }
            file.write_all(&bytes).map_err(|e| format!("write {}: {}", asset.display(), e))?;
            chapters.push((bytes.len() as u64, hex(&Sha256::digest(&bytes))));
        }
        file.sync_all().map_err(|e| format!("write {}: {}", asset.display(), e))?;
        drop(file);

        // What will be uploaded is what the manifest says, chapter by chapter
        let written = fs::read(&asset).map_err(|e| format!("read {}: {}", asset.display(), e))?;
        let mut at = 0usize;
        for (i, (size, sha256)) in chapters.iter().enumerate() {
            let slice = written.get(at..at + *size as usize).ok_or_else(|| format!("{} is short", asset.display()))?;
            if hex(&Sha256::digest(slice)) != *sha256 {
                return Err(format!("{}: chapter {} doesn't match after writing", asset.display(), i + 1));
            }
            at += *size as usize;
        }
        if at != written.len() {
            return Err(format!("{} is longer than its chapters", asset.display()));
        }
        books.push(Book { code: book.code, chapters });
    }

    write_manifest(manifest, recording, release, &books)?;
    write_release_extras(out, &catalogue, release, &books)?;
    let total: u64 = books.iter().flat_map(|b| &b.chapters).map(|c| c.0).sum();
    let chapters: usize = books.iter().map(|b| b.chapters.len()).sum();
    println!("{}: {} books, {} chapters, {:.1} MB", recording, books.len(), chapters, total as f64 / 1e6);
    Ok(())
}

/// One chapter a line, so a changed chapter shows as one changed line.
fn write_manifest(path: &Path, recording: &str, release: &str, books: &[Book]) -> Result<(), String> {
    let mut text = format!(
        "{{\n  \"recording\": {},\n  \"release\": {},\n  \"books\": {{\n",
        serde_json::to_string(recording).unwrap(),
        serde_json::to_string(release).unwrap()
    );
    for (b, book) in books.iter().enumerate() {
        text.push_str(&format!("    \"{}\": [\n", book.code));
        for (c, (size, sha256)) in book.chapters.iter().enumerate() {
            let comma = if c + 1 < book.chapters.len() { "," } else { "" };
            text.push_str(&format!("      [{}, \"{}\"]{}\n", size, sha256, comma));
        }
        text.push_str(if b + 1 < books.len() { "    ],\n" } else { "    ]\n" });
    }
    text.push_str("  }\n}\n");
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("create {}: {}", dir.display(), e))?;
    }
    fs::write(path, text).map_err(|e| format!("write {}: {}", path.display(), e))
}

/// SHA256SUMS for each book's file, and the release's notes (who reads it, its licence).
fn write_release_extras(out: &Path, catalogue: &Catalogue, release: &str, books: &[Book]) -> Result<(), String> {
    let mut sums = String::new();
    for book in books {
        let name = format!("{}.{}.ogg", catalogue.id, book.code);
        let bytes = fs::read(out.join(&name)).map_err(|e| format!("read {}: {}", name, e))?;
        sums.push_str(&format!("{}  {}\n", hex(&Sha256::digest(&bytes)), name));
    }
    fs::write(out.join("SHA256SUMS"), sums).map_err(|e| format!("write SHA256SUMS: {}", e))?;
    let total: u64 = books.iter().flat_map(|b| &b.chapters).map(|c| c.0).sum();
    let notes = format!(
        "{credit}\n\nSource: {source}\n\n\
         Downloaded by Scriptorium when you ask for it (Settings, Audio Bibles), which checks every \
         chapter against the SHA-256 built into the app. One file per book ({books} books, {mb:.0} MB in all): \
         the book's chapters as Ogg Opus files end to end, so each also plays as one file. \
         SHA256SUMS lists each file's SHA-256.\n\nRelease {release}: these files never change; a changed \
         recording is published as a new release.\n",
        credit = catalogue.credit,
        source = catalogue.source,
        books = books.len(),
        mb = total as f64 / 1e6,
    );
    fs::write(out.join("notes.md"), notes).map_err(|e| format!("write notes.md: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ogg(n: u8, len: usize) -> Vec<u8> {
        let mut bytes = b"OggS".to_vec();
        bytes.resize(len, n);
        bytes
    }

    #[test]
    fn packs_books_end_to_end_and_lists_every_chapter() {
        let dir = std::env::temp_dir().join(format!("kjv-audio-pack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let (catalogues, from, out) = (dir.join("audio"), dir.join("chapters"), dir.join("out"));
        fs::create_dir_all(&catalogues).unwrap();
        fs::create_dir_all(&from).unwrap();
        let catalogue = r#"{"id": "test-reader", "reader": "A Reader", "bibles": ["bsb"], "licence": "cc0",
            "credit": "Read by A Reader.", "source": "https://example.org",
            "chapters": {"3JN.1": {"d": 1, "v": [["1", 0]]}, "2JN.1": {"d": 1, "v": [["1", 0]]}, "RUT.1": {}, "RUT.2": {}}}"#;
        fs::write(catalogues.join("test-reader.json"), catalogue).unwrap();
        for (key, n, len) in [("RUT.1", 1, 10), ("RUT.2", 2, 20), ("2JN.1", 3, 5), ("3JN.1", 4, 7)] {
            fs::write(from.join(format!("{}.ogg", key)), ogg(n, len)).unwrap();
        }
        let manifest = dir.join("downloads/test-reader.json");
        assert!(pack(&catalogues, "test-reader", "test-reader-1", &from, &out, &manifest).is_err(), "release name");
        pack(&catalogues, "test-reader", "audio-test-reader-1", &from, &out, &manifest).unwrap();

        let ruth = fs::read(out.join("test-reader.RUT.ogg")).unwrap();
        assert_eq!(ruth, [ogg(1, 10), ogg(2, 20)].concat(), "chapters end to end, in order");
        let text = fs::read_to_string(&manifest).unwrap();
        let m: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(m["release"], "audio-test-reader-1");
        assert_eq!(m["books"]["RUT"][1][0], 20);
        assert_eq!(m["books"]["RUT"][1][1], hex(&Sha256::digest(ogg(2, 20))));
        let order: Vec<&str> = text.lines().filter_map(|l| l.trim().strip_suffix(": [")).collect();
        assert_eq!(order, ["\"RUT\"", "\"2JN\"", "\"3JN\""], "books in the Bible's order");
        let sums = fs::read_to_string(out.join("SHA256SUMS")).unwrap();
        assert!(sums.contains(&format!("{}  test-reader.RUT.ogg", hex(&Sha256::digest(&ruth)))));

        // A chapter file too many, or one missing, or not Ogg, is refused
        fs::write(from.join("RUT.3.ogg"), ogg(5, 3)).unwrap();
        assert!(pack(&catalogues, "test-reader", "audio-test-reader-1", &from, &out, &manifest).is_err());
        fs::remove_file(from.join("RUT.3.ogg")).unwrap();
        fs::remove_file(from.join("2JN.1.ogg")).unwrap();
        assert!(pack(&catalogues, "test-reader", "audio-test-reader-1", &from, &out, &manifest).is_err());
        fs::write(from.join("2JN.1.ogg"), b"ID3 not ogg").unwrap();
        assert!(pack(&catalogues, "test-reader", "audio-test-reader-1", &from, &out, &manifest).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
