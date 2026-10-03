//! What the study assistant can look up for itself, when the reader lets it: passages
//! in any translation, with commentaries' notes, cross-references, and the Hebrew and
//! Greek, as the context engine gives them ([`READ`]); where words occur in the
//! library ([`SEARCH`]); and lexicon entries ([`LEXICON`]). The model asks by name with
//! JSON arguments, and gets text: the library's own words, never more than the room
//! it is given.

use serde_json::{Value, json};

use kjv_library::Library;

use crate::api::{Scope, strongs_display};
use crate::bundle::DataBundle;
use crate::context::{self, PassageSpec, Spec, estimate_tokens};
use crate::models::normalize_strongs;
use crate::search::{self, Kind, SearchArgs};

pub const READ: &str = "read";
pub const SEARCH: &str = "search";
pub const LEXICON: &str = "lexicon";

/// A tool, for the provider: its name, what it does, and its arguments (JSON Schema).
pub struct ToolSpec {
    pub name: &'static str,
    pub description: String,
    pub parameters: Value,
}

/// What a lookup found.
pub struct Found {
    /// For the reader: "John 3:16 · WEB"
    pub label: String,
    /// For the model
    pub text: String,
    /// It couldn't be done (`text` says why)
    pub failed: bool,
}

/// Results given for each source searched, and in all
const SEARCH_EACH: usize = 20;
const SEARCH_ALL: usize = 60;
/// Places per verse for cross-references read
const CROSSREF_LIMIT: usize = 10;

/// Commentaries offered as notes (the Treasury is offered as cross-references, as the
/// app offers it)
fn commentaries(lib: &Library) -> Vec<&kjv_library::notes::CommentaryInfo> {
    let lists: Vec<&str> = lib.crossrefs().iter().filter_map(|c| c.commentary.as_deref()).collect();
    lib.commentaries().iter().filter(|c| !lists.contains(&c.id.as_str())).collect()
}

/// The tools, with the library's translations, commentaries, and cross-reference
/// collections named in them.
pub fn tools(lib: &Library) -> Vec<ToolSpec> {
    let bibles: Vec<&str> = lib.bibles().iter().map(|b| b.id.as_str()).collect();
    let notes = commentaries(lib);
    let note_ids: Vec<&str> = notes.iter().map(|c| c.id.as_str()).collect();
    let xref_ids: Vec<&str> = lib.crossrefs().iter().map(|c| c.id.as_str()).collect();
    let catalogue = format!(
        "Translations: {}.\nCommentaries: {}.\nCross-references: {}.",
        lib.bibles().iter().map(|b| format!("{} ({}, {})", b.id, b.name, b.year)).collect::<Vec<_>>().join("; "),
        notes.iter().map(|c| format!("{} ({} by {}, {}; {})", c.id, c.name, c.author, c.year, c.tradition)).collect::<Vec<_>>().join("; "),
        lib.crossrefs().iter().map(|c| format!("{} ({})", c.id, c.name)).collect::<Vec<_>>().join("; "),
    );
    let ids = |list: &[&str], what: &str| json!({"type": "array", "items": {"type": "string", "enum": list}, "description": what});
    vec![
        ToolSpec {
            name: READ,
            description: format!(
                "Read passages from the library, word for word: Scripture in any of its translations, the notes of its \
                 commentaries on them, their cross-references, and the Hebrew, Aramaic, and Greek behind the KJV. The text \
                 comes as attached text does (<passage>, <bible>, <commentary>, <crossrefs>, <original>). Ask for what the \
                 question needs: a few verses or a chapter, not a whole book, and only the commentaries it calls for.\n{}",
                catalogue
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "references": {
                        "type": "string",
                        "description": "The passages, as Book chapter:verse: \"John 3:16\", \"Romans 8\", \"Psalm 23; Isaiah 53:4-6\". Several are separated by semicolons. They are numbered as the first of `translations` numbers them."
                    },
                    "translations": ids(&bibles, "Translations to give the text in, by id (the KJV when none)"),
                    "commentaries": ids(&note_ids, "Commentaries whose notes on the passages to give, by id"),
                    "cross_references": ids(&xref_ids, "Cross-reference collections to give, by id: places elsewhere in Scripture that bear on each verse"),
                    "original": {"type": "boolean", "description": "Also the Hebrew, Aramaic, or Greek words behind the KJV, each with its Strong's number and a short gloss"}
                },
                "required": ["references"]
            }),
        },
        ToolSpec {
            name: SEARCH,
            description: "Find where words occur in the library: every verse of a translation, or every note of a commentary, \
                          that contains them (case, curly quotes, and accents set aside; a phrase is found as written). \
                          Gives each verse's reference and words, or the words around the match in a note. Use the ids \
                          listed for read."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "The words to find, as they would be printed: \"son of man\", \"propitiation\""},
                    "translations": ids(&bibles, "Translations to search, by id (the KJV when none, and no commentaries)"),
                    "commentaries": ids(&note_ids, "Commentaries to search, by id"),
                    "testament": {"type": "string", "enum": ["old", "new"], "description": "Only the Old Testament (with the Apocrypha) or only the New"}
                },
                "required": ["query"]
            }),
        },
        ToolSpec {
            name: LEXICON,
            description: "The full lexicon entries (STEP Bible's Hebrew and Greek lexicons) for Strong's numbers: the word, \
                          its transliteration, grammar, gloss, and definition with its range of meaning."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "strongs": {"type": "array", "items": {"type": "string"}, "description": "Strong's numbers: \"H1254\", \"G26\""}
                },
                "required": ["strongs"]
            }),
        },
    ]
}

/// Tokens the tools' definitions add to a request.
pub fn tools_tokens(lib: &Library) -> usize {
    tools(lib).iter().map(|t| estimate_tokens(&t.description) + estimate_tokens(&t.parameters.to_string())).sum()
}

/// Carry out tool `name` with `args`, giving at most `room` tokens of text.
pub fn run(data: &DataBundle, lib: &Library, name: &str, args: &Value, room: usize) -> Found {
    let failed = |label: &str, text: String| Found { label: label.to_string(), text, failed: true };
    if !args.is_object() {
        return failed("Couldn't look that up", format!("The arguments for {} weren't a JSON object.", name));
    }
    match name {
        READ => read(data, lib, args, room).unwrap_or_else(|e| failed("Couldn't read that", e)),
        SEARCH => search_library(data, lib, args, room).unwrap_or_else(|e| failed("Couldn't search", e)),
        LEXICON => lexicon(data, args, room).unwrap_or_else(|e| failed("Couldn't find that", e)),
        other => failed("Couldn't look that up", format!("There is no tool called {:?}: there are read, search, and lexicon.", other)),
    }
}

/// `args[key]`, a list of strings (absent: empty). Models sometimes send a list as a
/// string, `"[\"kjv\", \"web\"]"` or `"kjv, web"`: it's read as the list it means.
fn strings(args: &Value, key: &str) -> Result<Vec<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) if s.trim_start().starts_with('[') => match serde_json::from_str::<Value>(s) {
            Ok(list @ Value::Array(_)) => strings(&json!({ key: list }), key),
            _ => Err(format!("{} must be a list of ids", key)),
        },
        Some(Value::String(s)) => Ok(s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()),
        Some(Value::Array(items)) => {
            items.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| format!("{} must be a list of ids", key))).collect()
        }
        Some(_) => Err(format!("{} must be a list of ids", key)),
    }
}

/// Each id must be one of `known`; the error names them all.
fn check(ids: &[String], known: &[&str], what: &str) -> Result<(), String> {
    match ids.iter().find(|id| !known.contains(&id.as_str())) {
        Some(id) => Err(format!("There is no {} {:?}. The {}s are: {}.", what, id, what, known.join(", "))),
        None => Ok(()),
    }
}

fn read(data: &DataBundle, lib: &Library, args: &Value, room: usize) -> Result<Found, String> {
    let references = args.get("references").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or("Say which passages to read")?;
    let translations = strings(args, "translations")?;
    let commentaries = strings(args, "commentaries")?;
    let crossrefs = strings(args, "cross_references")?;
    let bible_ids: Vec<&str> = lib.bibles().iter().map(|b| b.id.as_str()).collect();
    check(&translations, &bible_ids, "translation")?;
    let notes = self::commentaries(lib);
    check(&commentaries, &notes.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), "commentary")?;
    check(&crossrefs, &lib.crossrefs().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), "cross-reference collection")?;
    let first = translations.first().cloned().unwrap_or_else(|| "kjv".to_string());
    let parsed = context::parse(data, lib, references, &first)?;
    let spec = Spec {
        passages: parsed
            .into_iter()
            .map(|p| PassageSpec { bible: p.bible, refs: p.refs, translations: None, commentaries: None, crossrefs: None, original: None })
            .collect(),
        translations: if translations.is_empty() { vec![first] } else { translations.clone() },
        commentaries: commentaries.clone(),
        crossrefs,
        crossref_limit: CROSSREF_LIMIT,
        crossref_text: false,
        original: args.get("original").and_then(Value::as_bool).unwrap_or(false),
        definitions: false,
    };
    // Built to fit the room: Hebrew and Greek take more tokens per byte than English,
    // so a build that comes out too long is made again, shorter
    let mut cap = room.saturating_mul(4);
    let mut built = context::build(data, lib, &spec, Some(cap))?;
    for _ in 0..4 {
        if built.tokens <= room || cap < 2_000 {
            break;
        }
        cap = cap * room / built.tokens * 9 / 10;
        built = context::build(data, lib, &spec, Some(cap))?;
    }
    let problems: Vec<String> = built.passages.iter().filter_map(|p| p.problem.clone()).collect();
    if built.text.is_empty() {
        return Err(if problems.is_empty() { "Nothing was found to read.".to_string() } else { format!("{}.", problems.join("; ")) });
    }
    let mut text = String::new();
    for p in &problems {
        text.push_str(&format!("({}.)\n", p));
    }
    text.push_str(&built.text);
    if built.capped {
        text.push_str("(Cut short here: there wasn't room for the rest. Read less at a time.)\n");
    }
    // "John 3:16 · WEB, KJV, Matthew Henry, OpenBible.info, Hebrew/Greek"
    let abbr = |id: &String| lib.bible(id).map_or(id.clone(), |b| b.abbr.clone());
    let mut sources: Vec<String> = spec.translations.iter().map(abbr).collect();
    sources.extend(commentaries.iter().filter_map(|id| notes.iter().find(|c| &c.id == id)).map(|c| c.short.clone().unwrap_or_else(|| c.name.clone())));
    sources.extend(spec.crossrefs.iter().filter_map(|id| lib.crossrefs().iter().find(|c| &c.id == id)).map(|c| c.short.clone()));
    if spec.original {
        sources.push("Hebrew/Greek".into());
    }
    Ok(Found { label: format!("{} · {}", built.label, sources.join(", ")), text, failed: false })
}

fn search_library(data: &DataBundle, lib: &Library, args: &Value, room: usize) -> Result<Found, String> {
    let query = args.get("query").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or("Say what to search for")?;
    let mut translations = strings(args, "translations")?;
    let commentaries = strings(args, "commentaries")?;
    let bible_ids: Vec<&str> = lib.bibles().iter().map(|b| b.id.as_str()).collect();
    check(&translations, &bible_ids, "translation")?;
    let notes = self::commentaries(lib);
    check(&commentaries, &notes.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), "commentary")?;
    if translations.is_empty() && commentaries.is_empty() {
        translations.push("kjv".to_string());
    }
    let scope = match args.get("testament").and_then(Value::as_str) {
        Some("old") => Scope::Old,
        Some("new") => Scope::New,
        _ => Scope::All,
    };
    let sources: Vec<(Kind, String)> =
        translations.iter().map(|t| (Kind::Bible, t.clone())).chain(commentaries.iter().map(|c| (Kind::Commentary, c.clone()))).collect();
    let mut text = String::new();
    let mut given = 0;
    let mut names = Vec::new();
    for (kind, id) in &sources {
        let name = match kind {
            Kind::Bible => lib.bible(id).map_or(id.clone(), |b| b.abbr.clone()),
            Kind::Commentary => notes.iter().find(|c| &c.id == id).map_or(id.clone(), |c| c.short.clone().unwrap_or_else(|| c.name.clone())),
        };
        names.push(name.clone());
        let found = search::search(data, lib, &SearchArgs { query: query.to_string(), kind: *kind, source: id.clone(), scope, book: None, limit: SEARCH_EACH })?;
        let unit = if *kind == Kind::Bible { "verse" } else { "note" };
        let shown = found.hits.len().min(SEARCH_ALL.saturating_sub(given));
        let mut section = format!(
            "## {}: {} {}{} contain{} “{}”{}\n",
            name,
            found.total,
            unit,
            if found.total == 1 { "" } else { "s" },
            if found.total == 1 { "s" } else { "" },
            query,
            if shown < found.total { format!(" (the first {} here)", shown) } else { String::new() },
        );
        for hit in found.hits.iter().take(shown) {
            let words: String = hit.segments.iter().map(|s| s.text.as_str()).collect();
            section.push_str(&format!("{}{} {}\n", hit.reference, if *kind == Kind::Commentary { ":" } else { "" }, words.trim()));
        }
        if estimate_tokens(&text) + estimate_tokens(&section) > room {
            text.push_str("(No room for more results: search fewer sources, or for something rarer.)\n");
            break;
        }
        text.push_str(&section);
        given += shown;
    }
    Ok(Found { label: format!("“{}” in {}", query, names.join(", ")), text, failed: false })
}

fn lexicon(data: &DataBundle, args: &Value, room: usize) -> Result<Found, String> {
    let numbers = strings(args, "strongs")?;
    if numbers.is_empty() {
        return Err("Give the Strong's numbers to look up: \"H1254\", \"G26\"".into());
    }
    let mut text = String::from("<definitions>\n");
    let mut found = Vec::new();
    for n in numbers.iter().take(20) {
        let entry = normalize_strongs(n).and_then(|key| context::lexicon_entry(data, &key).map(|e| (key, e)));
        match entry {
            Some((key, entry)) => {
                if estimate_tokens(&text) + estimate_tokens(&entry) > room {
                    text.push_str("(No room for more entries.)\n");
                    break;
                }
                text.push_str(&entry);
                found.push(strongs_display(&key));
            }
            None => text.push_str(&format!("(No lexicon entry for {:?}.)\n", n)),
        }
    }
    text.push_str("</definitions>\n");
    Ok(Found { label: format!("Strong's {}", if found.is_empty() { numbers.join(", ") } else { found.join(", ") }), failed: found.is_empty(), text })
}
