//! Audio Bibles: recordings read chapter by chapter, one file per chapter, with each
//! chapter's verse timings built in from `data/audio/<recording>.json`, so the reader can
//! follow along and start at any verse.
//!
//! A recording is downloaded when the reader asks for it. `data/audio/downloads/
//! <recording>.json`, built in too, names the release of the app's repository that holds
//! it (one file per book: the book's chapter files end to end) and each chapter's size and
//! SHA-256, which every chapter downloaded must match before it's kept. A build can also
//! ship recordings beside the app (`audio/<recording>/<BOOK>.<CH>.ogg`).

use std::collections::HashMap;
use std::sync::OnceLock;

use kjv_library::books;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/audio_catalogues.rs"));

#[derive(Debug, Deserialize)]
struct Catalogue {
    id: String,
    reader: String,
    /// The translations whose text it reads (ids in the library)
    bibles: Vec<String>,
    licence: String,
    credit: String,
    source: String,
    chapters: HashMap<String, Timing>,
}

#[derive(Debug, Clone, Deserialize)]
struct Timing {
    /// Length in seconds
    d: f64,
    /// Each verse's label and where it starts, in seconds
    v: Vec<(String, f64)>,
}

fn catalogues() -> &'static [Catalogue] {
    static ALL: OnceLock<Vec<Catalogue>> = OnceLock::new();
    ALL.get_or_init(|| {
        CATALOGUES.iter().map(|text| serde_json::from_str(text).expect("data/audio catalogue parses")).collect()
    })
}

/// A recording, for the list in Settings and the Licences page.
#[derive(Debug, Serialize)]
pub struct Recording {
    pub id: String,
    pub reader: String,
    pub bibles: Vec<String>,
    pub licence: String,
    pub credit: String,
    pub source: String,
    pub chapters: usize,
}

pub fn recordings() -> Vec<Recording> {
    catalogues()
        .iter()
        .map(|c| Recording {
            id: c.id.clone(),
            reader: c.reader.clone(),
            bibles: c.bibles.clone(),
            licence: c.licence.clone(),
            credit: c.credit.clone(),
            source: c.source.clone(),
            chapters: c.chapters.len(),
        })
        .collect()
}

/// One chapter's recording: the file to play (relative to the app's audio folder), its
/// length, and where each verse starts.
#[derive(Debug, Serialize)]
pub struct ChapterAudio {
    pub recording: String,
    pub reader: String,
    /// The book's code ("GEN") and the chapter, for what's downloaded
    pub code: String,
    pub chapter: u32,
    pub file: String,
    pub duration: f64,
    pub verses: Vec<(String, f64)>,
}

/// The recording of `book` (the app's name for it, "First Samuel") chapter `chapter` in
/// translation `bible`, if there is one.
pub fn chapter(bible: &str, book: &str, chapter: u32) -> Option<ChapterAudio> {
    let code = books::by_name(book)?.code;
    let key = format!("{}.{}", code, chapter);
    catalogues().iter().filter(|c| c.bibles.iter().any(|b| b == bible)).find_map(|c| {
        c.chapters.get(&key).map(|t| ChapterAudio {
            recording: c.id.clone(),
            reader: c.reader.clone(),
            code: code.to_string(),
            chapter,
            file: format!("{}/{}.ogg", c.id, key),
            duration: t.d,
            verses: t.v.clone(),
        })
    })
}

/// Whether `file` names a chapter of a recording ("bsb-souer/GEN.1.ogg"): what the app's
/// audio protocol will open. Anything else, including any path that could leave the
/// audio folder, is refused.
pub fn is_chapter_file(file: &str) -> bool {
    let Some((recording, name)) = file.split_once('/') else {
        return false;
    };
    let Some(key) = name.strip_suffix(".ogg") else {
        return false;
    };
    catalogues().iter().any(|c| c.id == recording && c.chapters.contains_key(key))
}

// ---------------------------------------------------------------- downloads

#[derive(Debug, Deserialize)]
struct Manifest {
    recording: String,
    release: String,
    /// Each book's chapters in order: [size in bytes, SHA-256 in hex]
    books: HashMap<String, Vec<(u64, String)>>,
}

/// What a recording is downloaded as: one file per book, attached to a release of the
/// app's repository, each holding the book's chapter files end to end, in order.
#[derive(Debug)]
pub struct Download {
    pub recording: String,
    /// The release's tag ("audio-bsb-souer-1"). Its files never change: a recording
    /// whose audio changes goes in a new release, so every version of the app can still
    /// download the files it was built with.
    pub release: String,
    /// In the Bible's order
    pub books: Vec<BookFile>,
}

#[derive(Debug)]
pub struct BookFile {
    /// The book's code: "GEN"
    pub code: &'static str,
    /// Chapter 1 first
    pub chapters: Vec<ChapterFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterFile {
    pub size: u64,
    pub sha256: [u8; 32],
}

impl Download {
    /// Every chapter's size, added up.
    pub fn size(&self) -> u64 {
        self.books.iter().map(BookFile::size).sum()
    }

    pub fn book(&self, code: &str) -> Option<&BookFile> {
        self.books.iter().find(|b| b.code == code)
    }

    /// The book's file in the release: "bsb-souer.GEN.ogg".
    pub fn asset(&self, code: &str) -> String {
        format!("{}.{}.ogg", self.recording, code)
    }
}

impl BookFile {
    pub fn size(&self) -> u64 {
        self.chapters.iter().map(|c| c.size).sum()
    }

    /// Where chapter `n` (from 1) starts in the book's file.
    pub fn offset(&self, n: usize) -> u64 {
        self.chapters[..n - 1].iter().map(|c| c.size).sum()
    }
}

impl ChapterFile {
    /// The name chapter `n` of `book` is kept under once its SHA-256 has been checked:
    /// "GEN.1.<the hash's first 16 hex digits>.ogg". Only a file under the name the app's
    /// own manifest gives it is played, so a chapter that a later release changes is
    /// downloaded again rather than played with timings that no longer fit it.
    pub fn stored_name(&self, book: &str, n: usize) -> String {
        format!("{}.{}.{}.ogg", book, n, hex(&self.sha256[..8]))
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn parse_sha256(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

fn parse_download(text: &str) -> Result<Download, String> {
    let m: Manifest = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut books = Vec::new();
    for (code, chapters) in m.books {
        let book = books::by_code(&code).ok_or_else(|| format!("{}: no book {:?}", m.recording, code))?;
        if chapters.is_empty() {
            return Err(format!("{} {}: no chapters", m.recording, code));
        }
        let chapters = chapters
            .into_iter()
            .enumerate()
            .map(|(i, (size, sha256))| match parse_sha256(&sha256) {
                Some(sha256) if size > 0 => Ok(ChapterFile { size, sha256 }),
                _ => Err(format!("{} {} {}: {} bytes, SHA-256 {:?}", m.recording, code, i + 1, size, sha256)),
            })
            .collect::<Result<Vec<_>, String>>()?;
        books.push(BookFile { code: book.code, chapters });
    }
    books.sort_by_key(|b| books::order(b.code));
    Ok(Download { recording: m.recording, release: m.release, books })
}

/// The recordings that can be downloaded: those whose catalogue is built in, in the
/// catalogues' order.
pub fn downloads() -> &'static [Download] {
    static ALL: OnceLock<Vec<Download>> = OnceLock::new();
    ALL.get_or_init(|| {
        let position = |id: &str| catalogues().iter().position(|c| c.id == id);
        let mut all: Vec<Download> = DOWNLOADS
            .iter()
            .map(|text| parse_download(text).expect("data/audio/downloads manifest parses"))
            .filter(|d| position(&d.recording).is_some())
            .collect();
        all.sort_by_key(|d| position(&d.recording));
        all
    })
}

pub fn download(recording: &str) -> Option<&'static Download> {
    downloads().iter().find(|d| d.recording == recording)
}

/// Where a downloaded chapter is kept, relative to the downloads folder, and how big it
/// is: "bsb-souer/GEN.1.ogg" gives ("bsb-souer/GEN.1.<hash>.ogg", its size). None for
/// anything that isn't a chapter of a recording that can be downloaded.
pub fn stored_file(file: &str) -> Option<(String, u64)> {
    if !is_chapter_file(file) {
        return None;
    }
    let (recording, name) = file.split_once('/')?;
    let (code, chapter) = name.strip_suffix(".ogg")?.split_once('.')?;
    let n: usize = chapter.parse().ok()?;
    let c = download(recording)?.book(code)?.chapters.get(n.checked_sub(1)?)?;
    Some((format!("{}/{}", recording, c.stored_name(code, n)), c.size))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every manifest in data/audio/downloads/: its recording has a catalogue, every one of
    /// whose chapters it lists (and nothing else), in a release named for it.
    #[test]
    fn every_download_matches_its_catalogue() {
        let mut seen = std::collections::HashSet::new();
        for text in DOWNLOADS {
            let d = parse_download(text).unwrap();
            assert!(seen.insert(d.recording.clone()), "{}: one manifest", d.recording);
            let c = catalogues().iter().find(|c| c.id == d.recording);
            let c = c.unwrap_or_else(|| panic!("{}: has a catalogue", d.recording));
            let n = d.release.strip_prefix(&format!("audio-{}-", d.recording)).and_then(|n| n.parse::<u32>().ok());
            assert!(n.is_some_and(|n| n >= 1), "{}: released as audio-{}-<n>, not {}", d.recording, d.recording, d.release);
            let mut listed: Vec<String> =
                d.books.iter().flat_map(|b| (1..=b.chapters.len()).map(move |n| format!("{}.{}", b.code, n))).collect();
            let mut catalogued: Vec<String> = c.chapters.keys().cloned().collect();
            listed.sort();
            catalogued.sort();
            assert_eq!(listed, catalogued, "{}: the catalogue's chapters, each once", d.recording);
            assert_eq!(download(&d.recording).map(Download::size), Some(d.size()), "{}: offered", d.recording);
        }
        assert_eq!(downloads().len(), DOWNLOADS.len());
    }

    const SAMPLE: &str = r#"{"recording": "bsb-souer", "release": "audio-bsb-souer-1", "books": {
        "3JN": [[300, "00000000000000000000000000000000000000000000000000000000000000ff"]],
        "GEN": [[100, "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"],
                [250, "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"]]}}"#;

    #[test]
    fn manifests_are_read() {
        let d = parse_download(SAMPLE).unwrap();
        assert_eq!(d.books.iter().map(|b| b.code).collect::<Vec<_>>(), ["GEN", "3JN"], "in the Bible's order");
        assert_eq!((d.size(), d.book("GEN").unwrap().size()), (650, 350));
        assert_eq!(d.asset("GEN"), "bsb-souer.GEN.ogg");
        let genesis = d.book("GEN").unwrap();
        assert_eq!((genesis.offset(1), genesis.offset(2)), (0, 100));
        assert_eq!(genesis.chapters[1].sha256[..2], [0xfe, 0xdc]);
        assert_eq!(genesis.chapters[0].stored_name("GEN", 1), "GEN.1.0123456789abcdef.ogg");
        for bad in [
            SAMPLE.replace("0123456789abcdef0123", "0123456789ABCDEF0123"),
            SAMPLE.replace("[100,", "[0,"),
            SAMPLE.replace("\"GEN\"", "\"GENESIS\""),
            SAMPLE.replace("00ff\"", "00f\""),
            r#"{"recording": "x", "release": "audio-x-1", "books": {"GEN": []}}"#.to_string(),
        ] {
            assert!(parse_download(&bad).is_err(), "{}", bad);
        }
    }

    #[test]
    fn stored_files_are_named_by_their_hash() {
        let Some(d) = downloads().first() else { return };
        let first = &d.books[0];
        let (name, size) = stored_file(&format!("{}/{}.1.ogg", d.recording, first.code)).unwrap();
        assert_eq!(name, format!("{}/{}.1.{}.ogg", d.recording, first.code, hex(&first.chapters[0].sha256[..8])));
        assert_eq!(size, first.chapters[0].size);
        let bad = [
            String::new(),
            "GEN.1.ogg".into(),
            format!("{}/../GEN.1.ogg", d.recording),
            format!("{}/GEN.0.ogg", d.recording),
            "nobody/GEN.1.ogg".into(),
        ];
        for bad in bad {
            assert!(stored_file(&bad).is_none(), "{:?}", bad);
        }
    }

    #[test]
    fn every_catalogue_is_whole_and_in_order() {
        for c in catalogues() {
            assert_eq!(c.chapters.len(), 1189, "{}: every chapter of the 66 books", c.id);
            for (key, t) in &c.chapters {
                let (code, _) = key.split_once('.').unwrap();
                assert!(books::by_code(code).is_some(), "{} {}", c.id, key);
                assert!(t.d > 0.0 && !t.v.is_empty(), "{} {}", c.id, key);
                assert!(t.v.windows(2).all(|w| w[0].1 <= w[1].1), "{} {}: verse starts in order", c.id, key);
                assert!(t.v.iter().all(|v| v.1 >= 0.0 && v.1 <= t.d), "{} {}: starts inside the recording", c.id, key);
            }
        }
    }

    #[test]
    fn file_names_are_checked() {
        let Some(c) = catalogues().first() else { return };
        assert!(is_chapter_file(&format!("{}/GEN.1.ogg", c.id)));
        for bad in ["", "GEN.1.ogg", "../secrets.json", &format!("{}/../x.ogg", c.id), &format!("{}/GEN.1.mp3", c.id),
            &format!("{}/GEN.51.ogg", c.id), &format!("nobody/GEN.1.ogg")] {
            assert!(!is_chapter_file(bad), "{:?}", bad);
        }
    }
}
