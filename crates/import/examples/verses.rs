//! Writes a translation's verses as JSON, exactly as the app reads them (the library's
//! `usfm::verses`), for tools outside Rust: the dramatized audio Bible's script
//! (drama/) starts from these.
//!
//!     cargo run --release -p kjv-import --example verses -- rv1909 drama/.cache/text/rv1909.json
//!
//! {"id": "rv1909", "books": {"GEN": [[1, "1", false, "EN el principio …"], …], …}}: each
//! verse as [chapter, number as printed, a Psalm title, text].

use kjv_import::bibles::catalogue;
use kjv_library::usfm::{self, Options};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, id, out] = &args[..] else {
        eprintln!("usage: verses <translation id> <out.json>");
        std::process::exit(2);
    };
    let entry = catalogue().unwrap().into_iter().find(|b| &b.id == id).unwrap_or_else(|| panic!("no translation {:?}", id));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/library/bibles").join(id);
    let options = Options { heading_markers: entry.heading_markers.clone() };
    let mut books = serde_json::Map::new();
    for b in kjv_library::books::BOOKS {
        let Ok(src) = std::fs::read_to_string(root.join(format!("{}.usfm", b.code))) else { continue };
        let book = usfm::parse(&src.replace("\r\n", "\n"), &options).unwrap_or_else(|e| panic!("{} {}: {}", id, b.code, e));
        let verses: Vec<serde_json::Value> =
            usfm::verses(&book).into_iter().map(|v| serde_json::json!([v.chapter, v.number, v.title, v.text])).collect();
        books.insert(b.code.to_string(), serde_json::Value::Array(verses));
    }
    let n: usize = books.values().map(|v| v.as_array().map_or(0, Vec::len)).sum();
    if let Some(dir) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(out, serde_json::to_string(&serde_json::json!({"id": id, "books": books})).unwrap()).unwrap();
    println!("{}: {} books, {} verses", id, books.len(), n);
}
