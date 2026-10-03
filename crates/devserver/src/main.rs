//! Browser preview of the app: serves `ui/` and answers `POST /api/<command>`
//! with the same dispatcher the app uses. Settings are kept in memory; API keys too, and
//! on Windows in Credential Manager (see `keychain`), so they outlast a restart.
//! AI replies stream back as one JSON event per line.
//!
//! Run from the repository root: `cargo run -p kjv-devserver -- [port]`

use std::collections::HashMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use kjv_ai::Event;
use kjv_ai::assistant::{AskArgs, ModelsArgs};
use kjv_ai::conversations::Conversations;
use kjv_core::bundle::DataBundle;
use kjv_core::dispatch::dispatch_all;
use kjv_library::Library;
use serde::Deserialize;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};
use tokio::sync::Notify;

struct State {
    ui: PathBuf,
    data: Arc<DataBundle>,
    library: Arc<Library>,
    settings: Mutex<Value>,
    keys: Mutex<HashMap<String, String>>,
    running: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    runtime: tokio::runtime::Runtime,
    client: kjv_ai::Client,
    conversations: Conversations,
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ttf") => "font/ttf",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

/// Map a URL path to a file under `root`, refusing anything that escapes it.
fn static_path(root: &Path, url: &str) -> Option<PathBuf> {
    let path = url.split('?').next().unwrap_or("/");
    let rel = Path::new(path.trim_start_matches('/'));
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let full = if path == "/" { root.join("index.html") } else { root.join(rel) };
    full.is_file().then_some(full)
}

fn main() {
    let root = std::env::current_dir().expect("current directory");
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(1420);

    eprintln!("Loading data…");
    let state = Arc::new(State {
        ui: root.join("ui"),
        data: Arc::new(DataBundle::from_sources(&root).expect("run from the repository root")),
        library: Arc::new({
            // Fast compression: the preview rebuilds this on every start
            let (bytes, _) = kjv_library::library::build::archive(&root, &|b| zstd::encode_all(b, 1).expect("compress"))
                .expect("build the library archive");
            Library::open(bytes).expect("open the library archive")
        }),
        settings: Mutex::new(Value::Null),
        keys: Mutex::default(),
        running: Arc::default(),
        runtime: tokio::runtime::Runtime::new().expect("async runtime"),
        client: kjv_ai::client(),
        // A fresh folder each run, like the in-memory settings
        conversations: Conversations::new(
            std::env::temp_dir().join(format!("kjv-devserver-conversations-{}", std::process::id())),
        ),
    });

    let server = Server::http(("127.0.0.1", port)).expect("port is free");
    eprintln!("Preview at http://localhost:{}", port);

    for request in server.incoming_requests() {
        let state = state.clone();
        // A reply can stream for minutes; don't hold up everything else
        std::thread::spawn(move || handle(&state, request));
    }
}

fn handle(state: &State, mut request: Request) {
    let url = request.url().to_string();
    let Some(name) = url.strip_prefix("/api/").map(str::to_string) else {
        let response = if request.method() == &Method::Get {
            match static_path(&state.ui, &url).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((path, bytes)) => Response::from_data(bytes)
                    .with_header(header("Content-Type", content_type(&path)))
                    .with_header(header("Cache-Control", "no-store")),
                None => Response::from_string("not found").with_status_code(404),
            }
        } else {
            Response::from_string("method not allowed").with_status_code(405)
        };
        let _ = request.respond(response);
        return;
    };
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    let args: Value = serde_json::from_str(&body).unwrap_or(Value::Null);

    if name == "ai_chat" {
        // Written by hand, one chunk per event, flushed at once: tiny_http buffers
        // the bodies it encodes itself
        let events = chat(state, args);
        let mut out = request.into_writer();
        // (HTTP lines end in CRLF, written as escapes: a source file's line endings change
        // with the checkout)
        let head = "HTTP/1.1 200 OK\r\n\
                    Content-Type: application/x-ndjson\r\n\
                    Cache-Control: no-store\r\n\
                    Transfer-Encoding: chunked\r\n\
                    Connection: close\r\n\r\n";
        let mut write = |bytes: &[u8]| out.write_all(bytes).and_then(|()| out.flush());
        if write(head.as_bytes()).is_err() {
            return;
        }
        for line in events {
            let mut chunk = format!("{:x}\r\n", line.len()).into_bytes();
            chunk.extend_from_slice(&line);
            chunk.extend_from_slice(b"\r\n");
            if write(&chunk).is_err() {
                return;
            }
        }
        let _ = write(b"0\r\n\r\n");
        return;
    }
    let result = match name.as_str() {
        "settings_load" => Ok(state.settings.lock().unwrap().clone()),
        "settings_save" => {
            *state.settings.lock().unwrap() = args;
            Ok(Value::Null)
        }
        "ai_models" => parse::<ModelsArgs>(args).and_then(|a| {
            let key = key_for(state, &a.provider_id);
            let list = state.runtime.block_on(kjv_ai::models(&state.client, &a.endpoint(key)))?;
            Ok(json!(list))
        }),
        "conversations_list" => state.conversations.list().map(Value::from),
        "conversation_load" => parse::<IdArgs>(args).and_then(|a| state.conversations.load(&a.id)),
        "conversation_save" => parse::<SaveArgs>(args).and_then(|a| state.conversations.save(&a.conversation).map(|()| Value::Null)),
        "conversation_delete" => parse::<IdArgs>(args).and_then(|a| state.conversations.delete(&a.id).map(|()| Value::Null)),
        "ai_cancel" => parse::<IdArgs>(args).map(|a| {
            if let Some(stop) = state.running.lock().unwrap().get(&a.id) {
                stop.notify_one();
            }
            Value::Null
        }),
        "ai_key_status" => parse::<KeyArgs>(args).map(|a| {
            json!({"stored": key_for(state, &a.provider_id).is_some(), "storage": "keychain"})
        }),
        "ai_key_set" => parse::<KeyArgs>(args).map(|a| {
            let key = a.key.unwrap_or_default().trim().to_string();
            let mut keys = state.keys.lock().unwrap();
            if key.is_empty() {
                keys.remove(&a.provider_id);
                keychain::delete(&a.provider_id);
            } else {
                keychain::set(&a.provider_id, &key);
                keys.insert(a.provider_id, key);
            }
            json!("keychain")
        }),
        "ai_key_delete" => parse::<KeyArgs>(args).map(|a| {
            state.keys.lock().unwrap().remove(&a.provider_id);
            keychain::delete(&a.provider_id);
            Value::Null
        }),
        _ => dispatch_all(&state.data, &state.library, &name, args),
    };
    let response = match result {
        Ok(value) => Response::from_string(value.to_string()).with_header(header("Content-Type", "application/json")),
        Err(message) => Response::from_string(message).with_status_code(400),
    };
    let _ = request.respond(response);
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
struct SaveArgs {
    conversation: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyArgs {
    provider_id: String,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Deserialize)]
struct ChatArgs {
    id: String,
    args: AskArgs,
}

/// A provider's API key: from this run's memory, else from the keychain (on Windows).
fn key_for(state: &State, provider_id: &str) -> Option<String> {
    if let Some(k) = state.keys.lock().unwrap().get(provider_id) {
        return Some(k.clone());
    }
    let k = keychain::get(provider_id)?;
    state.keys.lock().unwrap().insert(provider_id.to_string(), k.clone());
    Some(k)
}

/// Keys entered in the preview, kept in Windows Credential Manager under their own
/// service name so they outlast a restart (elsewhere they last as long as the server).
#[cfg(target_os = "windows")]
mod keychain {
    use std::sync::{Arc, OnceLock};

    use keyring_core::{CredentialStore, Entry};

    const SERVICE: &str = "scriptorium-devserver";

    fn entry(id: &str) -> Option<Entry> {
        static STORE: OnceLock<Option<Arc<CredentialStore>>> = OnceLock::new();
        let store = STORE.get_or_init(|| windows_native_keyring_store::Store::new().ok().map(|s| s as Arc<CredentialStore>)).as_ref()?;
        store.build(SERVICE, id, None).ok()
    }

    pub fn get(id: &str) -> Option<String> {
        entry(id)?.get_password().ok()
    }

    pub fn set(id: &str, key: &str) {
        if let Some(e) = entry(id) {
            let _ = e.set_password(key);
        }
    }

    pub fn delete(id: &str) {
        if let Some(e) = entry(id) {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod keychain {
    pub fn get(_: &str) -> Option<String> {
        None
    }
    pub fn set(_: &str, _: &str) {}
    pub fn delete(_: &str) {}
}

fn parse<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|e| format!("bad arguments: {}", e))
}

fn send_line(tx: &Sender<Vec<u8>>, value: Value) {
    let mut bytes = value.to_string().into_bytes();
    bytes.push(b'\n');
    let _ = tx.send(bytes);
}

/// Events as newline-delimited JSON; a failure ends with `{"type":"error"}`. The
/// receiver ends when the answer does.
fn chat(state: &State, args: Value) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel::<Vec<u8>>();
    let reply = rx;
    let a: ChatArgs = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            send_line(&tx, json!({"type": "error", "message": e}));
            return reply;
        }
    };
    let key = key_for(state, &a.args.provider_id);
    let stop = Arc::new(Notify::new());
    state.running.lock().unwrap().insert(a.id.clone(), stop.clone());
    let (data, library, client, running) = (state.data.clone(), state.library.clone(), state.client.clone(), state.running.clone());
    state.runtime.spawn(async move {
        let result = async {
            let request = tokio::task::spawn_blocking(move || kjv_ai::assistant::prepare(&data, &library, &a.args, key))
                .await
                .map_err(|e| e.to_string())??;
            tokio::select! {
                r = kjv_ai::chat(&client, &request, |event: Event| {
                    send_line(&tx, serde_json::to_value(event).unwrap());
                }) => r,
                () = stop.notified() => {
                    send_line(&tx, json!({"type": "done", "reason": "cancelled"}));
                    Ok(())
                }
            }
        }
        .await;
        if let Err(message) = result {
            send_line(&tx, json!({"type": "error", "message": message}));
        }
        running.lock().unwrap().remove(&a.id);
        // Dropping `tx` here ends the response
    });
    reply
}
