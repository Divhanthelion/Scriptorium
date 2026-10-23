//! Several translations side by side: the leading translation's chapter verse by
//! verse, each row with every other column's corresponding verses (found through the
//! verse alignment, so the Douay-Rheims' Psalm 22 sits beside the KJV's 23), and, if
//! asked, the Hebrew or Greek the KJV's verses are translated from.

use std::collections::HashMap;

use kjv_library::Library;
use kjv_library::alignment::Ref;
use kjv_library::books;
use kjv_library::view::{ChapterView as LibraryChapter, Heading, Part, VerseView as LibraryVerse};
use serde::{Deserialize, Serialize};

use crate::api::{self, ChapterOptions, ChapterRef, OriginalView};
use crate::bundle::DataBundle;
use crate::translations;

/// The column of Hebrew, Aramaic, and Greek
pub const ORIGINAL: &str = "original";

#[derive(Debug, Deserialize)]
pub struct ParallelArgs {
    /// Translation ids, the leading one first; "original" for the Hebrew and Greek
    pub columns: Vec<String>,
    /// The app's key for the book ("First Samuel"), and the chapter, in the leading
    /// translation's numbering
    pub book: String,
    pub chapter: u32,
}

#[derive(Debug, Serialize)]
pub struct Column {
    pub id: String,
    /// "KJV", "DRA", "Hebrew", "Greek", "Hebrew & Aramaic"
    pub abbr: String,
    pub name: String,
    /// The text's language (BCP 47: "en", "es"); empty for the Hebrew and Greek, whose
    /// words carry their own
    pub language: String,
}

#[derive(Debug, Serialize)]
pub struct ParallelChapter {
    /// The app's book key and chapter, as the leading translation has them
    pub book: String,
    pub chapter: u32,
    /// "Psalm 23"
    pub heading: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// The leading translation's headings after its last verse
    pub after: Vec<Heading>,
    pub prev: Option<ChapterRef>,
    pub next: Option<ChapterRef>,
}

/// One verse of the leading translation and what the other columns have for it.
#[derive(Debug, Serialize)]
pub struct Row {
    /// The leading verse's number: "16", "1-2"; "0" for a Psalm title
    pub number: String,
    /// The leading translation's headings before it
    pub before: Vec<Heading>,
    /// One per column, the leading translation's first
    pub cells: Vec<Cell>,
}

#[derive(Debug, Default, Serialize)]
pub struct Cell {
    /// Usually one verse; more where the column divides it differently; none where
    /// the column has nothing for it
    pub verses: Vec<CellVerse>,
    /// What the column has for this verse was given in an earlier row (one verse of
    /// the column holds this one and the one before)
    pub above: bool,
}

#[derive(Debug, Serialize)]
pub struct CellVerse {
    /// Its number, with the chapter, or book and chapter, where those differ from
    /// the row's: "16", "22:1", "Ezra 11:1"; "title" for a Psalm title
    pub label: String,
    /// The text, as the reader draws it
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    /// For the original-language column
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<OriginalView>,
}

/// A chapter of one column, ready to pick verses from.
enum Chapter {
    /// The app's KJV
    Core(api::ChapterView),
    Library(LibraryChapter),
}

impl Chapter {
    /// Its verses' numbers in order, a Psalm title first as "0".
    fn numbers(&self) -> Vec<String> {
        match self {
            Chapter::Core(c) => c.title.iter().map(|_| "0".to_string()).chain(c.verses.iter().map(|v| v.number.to_string())).collect(),
            Chapter::Library(c) => c.title.iter().map(|_| "0".to_string()).chain(c.verses.iter().map(|v| v.number.clone())).collect(),
        }
    }

    fn verse(&self, number: &str) -> Option<Vec<Part>> {
        match self {
            Chapter::Core(c) => {
                let n: u32 = number.parse().ok()?;
                let v = if n == 0 { c.title.as_ref()? } else { c.verses.iter().find(|v| v.number == n)? };
                Some(
                    v.segments
                        .iter()
                        .map(|s| Part::Text { text: s.text.clone(), styles: if s.red { vec!["wj".into()] } else { Vec::new() } })
                        .collect(),
                )
            }
            Chapter::Library(c) => {
                let v = if number == "0" { c.title.as_ref() } else { None }.or_else(|| c.verses.iter().find(|v| v.number == number))?;
                Some(v.parts.clone())
            }
        }
    }
}

fn in_core(data: &DataBundle, code: &str) -> Option<String> {
    let name = books::by_code(code)?.name;
    data.bible.books.iter().any(|b| b.name == name).then(|| name.to_string())
}

struct Chapters<'a> {
    data: &'a DataBundle,
    lib: &'a Library,
    kept: HashMap<(String, String, u32), Option<Chapter>>,
}

impl Chapters<'_> {
    /// Chapter `chapter` of `code` in translation `id` (the KJV's 66 books from the app's KJV).
    fn get(&mut self, id: &str, code: &str, chapter: u32) -> &Option<Chapter> {
        let key = (id.to_string(), code.to_string(), chapter);
        if !self.kept.contains_key(&key) {
            let c = match (id, in_core(self.data, code)) {
                ("kjv", Some(name)) => {
                    let options = ChapterOptions { red_letter: true, query: None, original: false };
                    api::chapter(self.data, &name, chapter, &options).ok().map(Chapter::Core)
                }
                _ => self.lib.chapter(id, code, chapter).ok().map(Chapter::Library),
            };
            self.kept.insert(key.clone(), c);
        }
        &self.kept[&key]
    }
}

/// The row of a verse only another column has
const EXTRA: usize = usize::MAX;

/// What has been given so far: each column's verses, and the row each is in.
struct Given {
    shown: Vec<HashMap<Ref, usize>>,
    languages: Vec<&'static str>,
}

/// Column `i`'s cell in row `row` (of chapter `at`): verses `refs` of translation
/// `target`, those it hasn't given already. The original-language column gives the
/// Hebrew, Aramaic, or Greek of the KJV's.
#[allow(clippy::too_many_arguments)]
fn cell(chapters: &mut Chapters, given: &mut Given, i: usize, original_column: bool, target: &str, refs: &[Ref], row: usize, at: (&str, u32)) -> Cell {
    let mut cell = Cell::default();
    let fresh: Vec<&Ref> = refs.iter().filter(|r| !given.shown[i].contains_key(*r)).collect();
    if fresh.is_empty() && !refs.is_empty() {
        cell.above = true;
    }
    for r in fresh {
        given.shown[i].insert(r.clone(), row);
        let l = label(at, r);
        if original_column {
            if let Some(o) = original(chapters.data, r) {
                if !given.languages.contains(&o.lang) {
                    given.languages.push(o.lang);
                }
                cell.verses.push(CellVerse { label: l, parts: Vec::new(), original: Some(o) });
            }
        } else if let Some(Some(parts)) = chapters.get(target, &r.0, r.1).as_ref().map(|c| c.verse(&r.2))
            && !parts.is_empty()
        {
            // (A verse with no text of its own, such as a Psalm title printed as a
            // heading, isn't given)
            cell.verses.push(CellVerse { label: l, parts, original: None });
        }
    }
    cell
}

/// "16"; "22:1" in another chapter; "Ezra 11:1" in another book; "title" for a Psalm title
fn label(row: (&str, u32), at: &Ref) -> String {
    let number = if at.2 == "0" { "title".to_string() } else { at.2.clone() };
    if (at.0.as_str(), at.1) == row {
        return number;
    }
    if at.0 == row.0 {
        return if at.2 == "0" { format!("{} (title)", at.1) } else { format!("{}:{}", at.1, number) };
    }
    let head = if at.0 == "PSA" { format!("Psalm {}", at.1) } else { format!("{} {}", books::by_code(&at.0).map_or(at.0.as_str(), |b| b.display), at.1) };
    if at.2 == "0" { format!("{} (title)", head) } else { format!("{}:{}", head, number) }
}

/// The Hebrew, Aramaic, or Greek of KJV verse `at`.
fn original(data: &DataBundle, at: &Ref) -> Option<OriginalView> {
    let name = in_core(data, &at.0)?;
    api::original_view(data, &name, at.1, at.2.parse().ok()?)
}

/// The leading translation's chapter: each verse's number and the headings before it.
struct Leading {
    heading: String,
    prev: Option<ChapterRef>,
    next: Option<ChapterRef>,
    verses: Vec<(String, Vec<Heading>)>,
    after: Vec<Heading>,
}

pub fn chapter(data: &DataBundle, lib: &Library, args: &ParallelArgs) -> Result<ParallelChapter, String> {
    let lead = args.columns.first().ok_or("no columns")?.clone();
    if lead == ORIGINAL {
        return Err("the first column must be a translation".into());
    }
    let k = books::by_name(&args.book).ok_or_else(|| format!("no book named {:?}", args.book))?;
    let code = k.code;
    let mut columns = Vec::new();
    for id in &args.columns {
        if id == ORIGINAL {
            columns.push(Column { id: id.clone(), abbr: String::new(), name: String::new(), language: String::new() });
        } else {
            let info = lib.bible(id).ok_or_else(|| format!("no translation {:?}", id))?;
            columns.push(Column { id: id.clone(), abbr: info.abbr.clone(), name: info.name.clone(), language: info.language.clone() });
        }
    }

    // The leading chapter: its verses, headings, and neighbours
    let mut chapters = Chapters { data, lib, kept: HashMap::new() };
    let Leading { heading, prev, next, verses: leading, after } = if lead == "kjv" && in_core(data, code).is_some() {
        let c = api::chapter(data, &args.book, args.chapter, &ChapterOptions::default())?;
        let verses = c.title.iter().chain(c.verses.iter()).map(|v| (v.number.to_string(), Vec::new())).collect();
        Leading { heading: c.heading, prev: c.prev, next: c.next, verses, after: Vec::new() }
    } else {
        let c = translations::chapter(lib, &lead, &args.book, args.chapter)?;
        let verses = c.view.title.iter().chain(c.view.verses.iter()).map(|v: &LibraryVerse| (v.number.clone(), v.before.clone())).collect();
        Leading { heading: c.heading, prev: c.prev, next: c.next, verses, after: c.view.after }
    };

    let mut given = Given { shown: vec![HashMap::new(); args.columns.len()], languages: Vec::new() };
    let mut rows = Vec::with_capacity(leading.len());
    for (number, before) in leading {
        let at: Ref = (code.to_string(), args.chapter, number.clone());
        let row = rows.len();
        let mut cells = Vec::with_capacity(args.columns.len());
        for (i, id) in args.columns.iter().enumerate() {
            let (target, refs) = if id == ORIGINAL {
                ("kjv", if lead == "kjv" { vec![at.clone()] } else { lib.map(&lead, "kjv", &at)? })
            } else if *id == lead {
                (id.as_str(), vec![at.clone()])
            } else {
                (id.as_str(), lib.map(&lead, id, &at)?)
            };
            cells.push(cell(&mut chapters, &mut given, i, id == ORIGINAL, target, &refs, row, (code, args.chapter)));
        }
        rows.push(Row { number, before, cells });
    }

    // Verses a column has that the leading translation doesn't (the KJV's Matthew 17:21
    // beside the BSB, which leaves it out): rows of their own, numbered "", where they
    // fall in that column, with nothing in the leading column
    let mut extra: Vec<(usize, Row)> = Vec::new();
    for (i, id) in args.columns.iter().enumerate() {
        if id == ORIGINAL || *id == lead {
            continue;
        }
        let mut held: Vec<(String, u32)> = given.shown[i].keys().map(|r| (r.0.clone(), r.1)).collect();
        held.sort_by_key(|(b, c)| (books::order(b), *c));
        held.dedup();
        for (book, chapter) in held {
            let numbers = match chapters.get(id, &book, chapter) {
                Some(c) => c.numbers(),
                None => continue,
            };
            // After the row of the column's verse before it
            let mut after = 0;
            for n in numbers {
                let r: Ref = (book.clone(), chapter, n);
                if let Some(&row) = given.shown[i].get(&r) {
                    if row != EXTRA {
                        after = row + 1;
                    }
                    continue;
                }
                let has_text = matches!(chapters.get(id, &book, chapter).as_ref().map(|c| c.verse(&r.2)), Some(Some(parts)) if !parts.is_empty());
                if !has_text || !lib.map(id, &lead, &r)?.is_empty() {
                    continue;
                }
                let mut cells = Vec::with_capacity(args.columns.len());
                for (j, other) in args.columns.iter().enumerate() {
                    let (target, refs) = if j == i {
                        (id.as_str(), vec![r.clone()])
                    } else if *other == lead {
                        (other.as_str(), Vec::new())
                    } else if other == ORIGINAL {
                        ("kjv", if id == "kjv" { vec![r.clone()] } else { lib.map(id, "kjv", &r)? })
                    } else {
                        (other.as_str(), lib.map(id, other, &r)?)
                    };
                    cells.push(cell(&mut chapters, &mut given, j, other == ORIGINAL, target, &refs, EXTRA, (code, args.chapter)));
                }
                extra.push((after, Row { number: String::new(), before: Vec::new(), cells }));
            }
        }
    }
    // Last first, so the earlier places stay where they were (and rows for one place
    // keep their order)
    extra.sort_by_key(|(at, _)| *at);
    for (at, row) in extra.into_iter().rev() {
        rows.insert(at.min(rows.len()), row);
    }
    let languages = given.languages;

    // The original-language column is named for what it holds here
    let names: Vec<&str> = languages
        .iter()
        .map(|l| match *l {
            "he" => "Hebrew",
            "arc" => "Aramaic",
            _ => "Greek",
        })
        .collect();
    for c in &mut columns {
        if c.id == ORIGINAL {
            c.abbr = match names.as_slice() {
                [] => "Hebrew/Greek".to_string(),
                [one] => one.to_string(),
                [a, b] => format!("{} & {}", a, b),
                more => more.join(", "),
            };
            c.name = format!("{}, as the KJV translates it", c.abbr);
        }
    }
    Ok(ParallelChapter { book: args.book.clone(), chapter: args.chapter, heading, columns, rows, after, prev, next })
}
