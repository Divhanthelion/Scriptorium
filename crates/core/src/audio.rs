//! Audio Bibles: recordings that ship with the app, one file per chapter
//! (`audio/<recording>/<BOOK>.<CH>.ogg` beside the app), with each chapter's verse
//! timings built in from `data/audio/<recording>.json`, so the reader can follow along
//! and start at any verse. Nothing is ever downloaded.

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

#[cfg(test)]
mod tests {
    use super::*;

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
