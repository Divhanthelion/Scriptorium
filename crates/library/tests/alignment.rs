//! The verse alignment tables (data/library/alignment/), read as the app reads them.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::OnceLock;

use kjv_library::alignment::Ref;
use kjv_library::{Library, library::build};

fn library() -> &'static Library {
    static LIB: OnceLock<Library> = OnceLock::new();
    LIB.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (bytes, _) = build::archive(&root, &|b| zstd::encode_all(b, 1).unwrap()).unwrap();
        Library::open(bytes).unwrap()
    })
}

fn r(book: &str, chapter: u32, verse: &str) -> Ref {
    (book.to_string(), chapter, verse.to_string())
}

fn map(from: &str, to: &str, at: Ref) -> Vec<Ref> {
    library().map(from, to, &at).unwrap()
}

#[test]
fn every_row_names_real_verses() {
    let lib = library();
    let mut problems = Vec::new();
    for b in lib.bibles().iter().filter(|b| b.id != "kjv") {
        let a = lib.alignment(&b.id).unwrap();
        for native in a.listed() {
            if !lib.has_verse(&b.id, native) {
                problems.push(format!("{}: {:?} is not a verse there", b.id, native));
            }
            for k in a.to_kjv(native, &|_| true) {
                if !lib.has_verse("kjv", &k) {
                    problems.push(format!("{}: {:?} maps to {:?}, not a KJV verse", b.id, native, k));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn the_hard_cases() {
    // The Vulgate's Psalms (Douay-Rheims)
    assert_eq!(map("kjv", "dra", r("PSA", 23, "1")), [r("PSA", 22, "1")]);
    assert_eq!(map("dra", "kjv", r("PSA", 9, "22")), [r("PSA", 10, "1")]);
    assert_eq!(map("kjv", "dra", r("PSA", 116, "10")), [r("PSA", 115, "1")]);
    assert_eq!(map("dra", "kjv", r("PSA", 147, "1")), [r("PSA", 147, "12")]);
    // Between two translations that both renumber (through the KJV): Douay-Rheims 22:1
    // holds the title and verse 1, which Brenton prints as 22:0 and 22:1
    assert_eq!(map("dra", "brenton", r("PSA", 22, "1")), [r("PSA", 22, "0"), r("PSA", 22, "1")]);
    assert_eq!(map("kjv", "brenton", r("PSA", 23, "1")), [r("PSA", 22, "1")]);
    // The WEB prints the Romans doxology at 14:24-26
    assert_eq!(map("kjv", "web", r("ROM", 16, "25")), [r("ROM", 14, "24")]);
    assert_eq!(map("web", "kjv", r("ROM", 14, "26")), [r("ROM", 16, "27")]);
    // Swapped verses
    assert_eq!(map("web", "kjv", r("MAT", 23, "13")), [r("MAT", 23, "14")]);
    // Susanna and Esther's additions, which the KJV prints in its Apocrypha
    assert_eq!(map("dra", "kjv", r("DAN", 13, "1")), [r("SUS", 1, "1")]);
    assert_eq!(map("dra", "kjv", r("EST", 11, "2")), [r("ESG", 11, "2")]);
    // Hebrew numbering (the Orthodox Jewish Bible's Malachi 3:19 is the KJV's 4:1)
    assert_eq!(map("ojb", "kjv", r("MAL", 3, "19")), [r("MAL", 4, "1")]);
    // The Septuagint's Jeremiah: the oracles against the nations (KJV 46-51) come after
    // 25:13, and the KJV's 26-45 follow them as 33-51
    assert_eq!(map("brenton", "kjv", r("JER", 27, "4")), [r("JER", 50, "4")]);
    assert_eq!(map("kjv", "brenton", r("JER", 26, "1")), [r("JER", 33, "1")]);
    assert_eq!(map("brenton", "kjv", r("JER", 25, "18")), [r("JER", 49, "38")]);
    assert_eq!(map("lxx2012", "kjv", r("JER", 33, "1")), [r("JER", 26, "1")]);
    // The Septuagint's Ezra goes on with Nehemiah (2 Esdras); Brenton also prints
    // Nehemiah on its own, which is where the KJV's Nehemiah opens
    assert_eq!(map("brenton", "kjv", r("EZR", 11, "1")), [r("NEH", 1, "1")]);
    assert_eq!(map("brenton", "kjv", r("EZR", 16, "15")), [r("NEH", 6, "15")]);
    assert_eq!(map("kjv", "brenton", r("NEH", 1, "1")), [r("NEH", 1, "1"), r("EZR", 11, "1")]);
    // Greek Esther, whole, against the KJV's Esther and its Additions
    assert_eq!(map("webu", "kjv", r("ESG", 2, "16")), [r("EST", 2, "16")]);
    assert_eq!(map("dra", "kjv", r("EST", 13, "1")), [r("ESG", 13, "1")]);
    // A verse the BSB leaves out
    assert_eq!(map("kjv", "bsb", r("MAT", 17, "21")), Vec::<Ref>::new());
    // Translations not in English (kjv-import's align::by_numbers). RV1909 divides some
    // books as the Hebrew, keeping the KJV's numbers with empty verses: its 1 Samuel
    // 23:29 is empty, its 24:1 the KJV's 23:29, its 24:22 the KJV's 24:21-22
    assert_eq!(map("rv1909", "kjv", r("1SA", 24, "1")), [r("1SA", 23, "29")]);
    assert_eq!(map("rv1909", "kjv", r("1SA", 23, "29")), Vec::<Ref>::new());
    assert_eq!(map("rv1909", "kjv", r("1SA", 24, "22")), [r("1SA", 24, "21"), r("1SA", 24, "22")]);
    assert_eq!(map("kjv", "rv1909", r("JON", 1, "17")), [r("JON", 2, "1")]);
    // and prints a Psalm's title inside verse 1
    assert_eq!(map("rv1909", "kjv", r("PSA", 3, "1")), [r("PSA", 3, "0"), r("PSA", 3, "1")]);
    // Numbered as the Douay-Rheims (through its table): Palabra de Dios para ti's
    // 2 Corinthians 13:13 is the KJV's 13:14
    assert_eq!(map("pddpt", "kjv", r("2CO", 13, "13")), [r("2CO", 13, "14")]);
    // Made from the WEB: its Romans doxology and Matthew 23:13-14
    assert_eq!(map("blm", "kjv", r("ROM", 14, "24")), [r("ROM", 16, "25")]);
    assert_eq!(map("bpm", "kjv", r("MAT", 23, "13")), [r("MAT", 23, "14")]);
    // Read: Philippians 1:16-17 in the critical text's order, 1 Samuel 20:43
    assert_eq!(map("bes", "kjv", r("PHP", 1, "16")), [r("PHP", 1, "17")]);
    assert_eq!(map("nbvpt", "kjv", r("1SA", 20, "43")), [r("1SA", 20, "42")]);
    // A bridged verse stands for its range
    assert_eq!(map("pddpt", "kjv", r("JHN", 8, "1-11")).len(), 11);
    // The same number everywhere else
    assert_eq!(map("kjv", "web", r("JHN", 3, "16")), [r("JHN", 3, "16")]);
}

/// A translation not in English is aligned without comparing its words with the KJV's
/// (kjv-import's align::by_numbers), so its verses' lengths are the check: in a chapter
/// matched a verse off, they fit the lengths of the KJV's verses a verse before or after
/// far better than those of the verses they're matched with (RV1909's 1 Samuel 24 by
/// number fit 0.88 a verse before and -0.11 as matched). Paraphrase and even poetry fit
/// loosely either way, so only a clear difference counts.
#[test]
fn no_translation_in_another_language_is_a_verse_off() {
    // Read and found rightly matched: (translation, book, chapter)
    const READ: &[(&str, &str, u32)] = &[
        // a paraphrase: its verses 1-9 are the KJV's 1-9, one by one
        ("nbves", "NUM", 36),
        // a paraphrase: its verses 1-12 are the KJV's 1-12 (8: "Senhor, eu amo a sua casa")
        ("nbvpt", "PSA", 26),
    ];
    fn fit(xs: &[f64], ys: &[f64]) -> f64 {
        let n = xs.len() as f64;
        let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
        let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
        let sx = xs.iter().map(|x| (x - mx).powi(2)).sum::<f64>().sqrt();
        let sy = ys.iter().map(|y| (y - my).powi(2)).sum::<f64>().sqrt();
        if sx == 0.0 || sy == 0.0 { 0.0 } else { sxy / (sx * sy) }
    }
    let lib = library();
    let mut problems = Vec::new();
    let mut chapters_checked = 0;
    for b in lib.bibles().iter().filter(|b| b.language != "en") {
        let a = lib.alignment(&b.id).unwrap();
        for book in &b.books {
            let Ok(kjv) = lib.verses("kjv", &book.code) else { continue };
            let kjv_len: HashMap<(u32, u32), f64> = kjv
                .iter()
                .filter(|v| !v.title)
                .filter_map(|v| Some(((v.chapter, v.number.parse().ok()?), v.text.chars().count() as f64)))
                .collect();
            // Each chapter's verses matched with one KJV verse of the book: (length, its KJV chapter and verse)
            let mut by_chapter: BTreeMap<u32, Vec<(f64, u32, u32)>> = BTreeMap::new();
            for v in lib.verses(&b.id, &book.code).unwrap().iter().filter(|v| !v.title) {
                let kjv_of = a.to_kjv(&(book.code.clone(), v.chapter, v.number.clone()), &|x| lib.has_verse("kjv", x));
                if let [(k, c, n)] = &kjv_of[..]
                    && *k == book.code
                    && let Ok(n) = n.parse::<u32>()
                {
                    by_chapter.entry(v.chapter).or_default().push((v.text.chars().count() as f64, *c, n));
                }
            }
            for (chapter, verses) in by_chapter {
                if verses.len() < 12 || READ.contains(&(b.id.as_str(), book.code.as_str(), chapter)) {
                    continue;
                }
                chapters_checked += 1;
                let at = |off: i64| {
                    let pairs: Vec<(f64, f64)> = verses
                        .iter()
                        .filter_map(|&(len, c, n)| Some((len, *kjv_len.get(&(c, u32::try_from(n as i64 + off).ok()?))?)))
                        .collect();
                    if pairs.len() < 10 { f64::NAN } else { fit(&pairs.iter().map(|p| p.0).collect::<Vec<_>>(), &pairs.iter().map(|p| p.1).collect::<Vec<_>>()) }
                };
                let (matched, before, after) = (at(0), at(-1), at(1));
                let shifted = before.max(after);
                if shifted >= 0.6 && shifted >= matched + 0.25 {
                    problems.push(format!(
                        "{} {} {}: verse lengths fit {:.2} as matched, {:.2} a verse before, {:.2} a verse after",
                        b.id, book.code, chapter, matched, before, after
                    ));
                }
            }
        }
    }
    assert!(chapters_checked > 10_000, "only {} chapters checked", chapters_checked);
    assert!(problems.is_empty(), "{}", problems.join("
"));
}
