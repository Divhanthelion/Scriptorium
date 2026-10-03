//! What the assistant can look up for itself, checked against the real data and library.

use std::path::Path;
use std::sync::OnceLock;

use kjv_core::bundle::DataBundle;
use kjv_core::context::estimate_tokens;
use kjv_core::lookups::{self, Found, LEXICON, READ, SEARCH};
use kjv_library::{Library, library::build};
use serde_json::{Value, json};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn data() -> &'static DataBundle {
    static DATA: OnceLock<DataBundle> = OnceLock::new();
    DATA.get_or_init(|| DataBundle::from_sources(root()).expect("bundle builds"))
}

fn lib() -> &'static Library {
    static LIB: OnceLock<Library> = OnceLock::new();
    LIB.get_or_init(|| {
        let (bytes, _) = build::archive(root(), &|b| zstd::encode_all(b, 1).unwrap()).unwrap();
        Library::open(bytes).unwrap()
    })
}

fn run(tool: &str, args: Value) -> Found {
    lookups::run(data(), lib(), tool, &args, 24_000)
}

#[test]
fn the_tools_name_every_source_by_id() {
    let tools = lookups::tools(lib());
    assert_eq!(tools.iter().map(|t| t.name).collect::<Vec<_>>(), [READ, SEARCH, LEXICON]);
    let read = &tools[0];
    let translations = read.parameters["properties"]["translations"]["items"]["enum"].as_array().unwrap();
    assert_eq!(translations.len(), lib().bibles().len());
    assert!(read.description.contains("web (World English Bible, 2020)"), "{}", read.description);
    let notes: Vec<&str> = read.parameters["properties"]["commentaries"]["items"]["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(notes.contains(&"mhc") && notes.contains(&"chrysostom"));
    // The Treasury is offered as cross-references, as the app offers it
    assert!(!notes.contains(&"tsk"));
    assert!(lookups::tools_tokens(lib()) > 500);
}

#[test]
fn read_gives_the_library_s_own_words() {
    let f = run(READ, json!({"references": "John 3:16", "translations": ["web", "kjv"]}));
    assert!(!f.failed, "{}", f.text);
    assert_eq!(f.label, "John 3:16 · WEB, KJV");
    assert!(f.text.contains("16 For God so loved the world, that he gave his only born Son, that whoever believes in him should not perish, but have eternal life."), "{}", f.text);
    assert!(f.text.contains("16 For God so loved the world, that he gave his only begotten Son"), "{}", f.text);
    // Commentaries, cross-references, and the Greek, as asked
    let f = run(READ, json!({"references": "Romans 3:25", "commentaries": ["mhc"], "cross_references": ["openbible"], "original": true}));
    assert_eq!(f.label, "Romans 3:25 · KJV, Matthew Henry, OpenBible, Hebrew/Greek");
    assert!(f.text.contains("<commentary") && f.text.contains("<crossrefs") && f.text.contains("G2435"), "{}", &f.text[..f.text.len().min(2000)]);
    // A list sent as a string, as models sometimes do, is the list it means
    for translations in [json!("[\"web\", \"kjv\"]"), json!("web, kjv")] {
        let f = run(READ, json!({"references": "John 3:16", "translations": translations}));
        assert_eq!(f.label, "John 3:16 · WEB, KJV", "{}", f.text);
    }
    // In the numbering of the first translation asked for: the Douay-Rheims' Psalm 22 is the KJV's 23
    let f = run(READ, json!({"references": "Psalm 22:1", "translations": ["dra", "kjv"]}));
    assert!(f.text.contains("The Lord ruleth me") && f.text.contains("The LORD is my shepherd"), "{}", f.text);
}

#[test]
fn read_says_what_it_cant_do() {
    let f = run(READ, json!({"references": "John 3:16", "translations": ["niv"]}));
    assert!(f.failed && f.text.starts_with("There is no translation \"niv\". The translations are: kjv,"), "{}", f.text);
    let f = run(READ, json!({"references": "Hezekiah 4:2"}));
    assert!(f.failed && f.text.contains("No book called"), "{}", f.text);
    // A translation without the book says so in its place
    let f = run(READ, json!({"references": "Tobit 1", "translations": ["web"]}));
    assert!(!f.failed && f.text.contains("(Not in this translation.)"), "{}", f.text);
    let f = run(READ, json!({}));
    assert!(f.failed);
    let f = run("write", json!({}));
    assert!(f.failed && f.text.contains("There is no tool called \"write\""));
    let f = lookups::run(data(), lib(), READ, &Value::Null, 1000);
    assert!(f.failed);
}

#[test]
fn read_fits_the_room_it_is_given() {
    let f = lookups::run(data(), lib(), READ, &json!({"references": "Psalm 119", "original": true}), 3_000);
    assert!(!f.failed);
    assert!(estimate_tokens(&f.text) <= 3_200, "{} tokens", estimate_tokens(&f.text));
    assert!(f.text.contains("(Cut short here"), "{}", &f.text[f.text.len().saturating_sub(300)..]);
    assert!(f.text.trim_end().ends_with(")") && f.text.contains("</context>"));
}

#[test]
fn search_finds_verses_and_notes() {
    let f = run(SEARCH, json!({"query": "Jesus wept"}));
    assert!(!f.failed);
    assert_eq!(f.label, "“Jesus wept” in KJV");
    assert!(f.text.contains("## KJV: 1 verse contains “Jesus wept”\nJohn 11:35 Jesus wept."), "{}", f.text);
    let f = run(SEARCH, json!({"query": "Melchizedek", "translations": ["web"], "commentaries": ["mhc"], "testament": "old"}));
    assert!(f.text.contains("## WEB: ") && f.text.contains("## Matthew Henry: "), "{}", f.text);
    assert!(f.text.contains("Genesis 14:18 "), "{}", f.text);
    assert!(!f.text.contains("Hebrews"), "the Old Testament only");
    // Results are capped, and the total is still given
    let f = run(SEARCH, json!({"query": "the"}));
    assert!(f.text.contains("(the first 20 here)"), "{}", &f.text[..200]);
}

#[test]
fn lexicon_gives_whole_entries() {
    let f = run(LEXICON, json!({"strongs": ["G26", "h1254", "G999999"]}));
    assert!(!f.failed);
    assert_eq!(f.label, "Strong's G26, H1254");
    assert!(f.text.starts_with("<definitions>\n## G26 ") && f.text.contains("agapē · G:N-F · love"), "{}", &f.text[..200]);
    assert!(f.text.contains("## H1254 "));
    assert!(f.text.contains("(No lexicon entry for \"G999999\".)"));
    assert!(run(LEXICON, json!({"strongs": []})).failed);
}
