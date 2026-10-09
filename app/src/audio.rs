//! The `audio` protocol: serves the audio Bibles' chapter files to the page, with byte
//! ranges so the player can seek. The files ship with the app (`bundle.resources` puts
//! `audio/` beside it): desktop and iOS read them from the resource folder, Android from
//! the APK's assets (where Google Play's install-time asset packs also land). Only a
//! recording's own chapter files can be asked for (`kjv_core::audio::is_chapter_file`).

use std::io::{Read, Seek, SeekFrom};

use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Runtime};

/// Most bytes sent for one range request; the player asks for the rest as it needs it.
const MAX_RANGE: u64 = 2 * 1024 * 1024;

pub fn handle<R: Runtime>(app: &AppHandle<R>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    // "/bsb-souer%2FGEN.1.ogg": convertFileSrc encodes the slash
    let file = percent_decode(request.uri().path().trim_start_matches('/'));
    if !kjv_core::audio::is_chapter_file(&file) {
        return status(StatusCode::NOT_FOUND);
    }
    let Some(mut source) = open(app, &file) else {
        // A build without its audio (a development build, or one made without the files)
        return status(StatusCode::NOT_FOUND);
    };
    let len = source.len;
    let range = request.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|r| parse_range(r, len));
    let (start, end, partial) = match range {
        // Android's WebView cuts the range out of the body itself (see `read_body`), so
        // there the range is sent as asked: a chapter is at most a few megabytes
        Some((start, end)) if WEBVIEW_APPLIES_RANGE => (start, end, true),
        Some((start, end)) => (start, end.min(start + MAX_RANGE - 1), true),
        None => (0, len.saturating_sub(1), false),
    };
    if len == 0 || start >= len {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{}", len))
            .body(Vec::new())
            .unwrap();
    }
    let Ok(body) = read_body(&mut source, start, end, WEBVIEW_APPLIES_RANGE) else {
        return status(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let mut response = Response::builder()
        .header(header::CONTENT_TYPE, "audio/ogg")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, (end - start + 1).to_string());
    if partial {
        response = response
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, len));
    }
    response.body(body).unwrap()
}

/// Android's WebView answers a range request by skipping, in the body it's handed, to the
/// range's first byte: a body holding only the range is too short, and the request fails.
const WEBVIEW_APPLIES_RANGE: bool = cfg!(target_os = "android");

/// The bytes `start..=end`; or, when the WebView skips to `start` itself, a body that
/// begins at the file's first byte, the bytes before `start` (never sent) left as zeros.
fn read_body(source: &mut Source, start: u64, end: u64, from_file_start: bool) -> std::io::Result<Vec<u8>> {
    let first = if from_file_start { 0 } else { start };
    let mut body = vec![0; (end + 1 - first) as usize];
    source.read_at(start, &mut body[(start - first) as usize..])?;
    Ok(body)
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder().status(code).body(Vec::new()).unwrap()
}

/// "bytes=100-", "bytes=100-199" (one range; the player never asks for more): the first
/// and last byte, inclusive.
fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            Some((len.saturating_sub(n), len.saturating_sub(1)))
        }
        (start, "") => Some((start.parse().ok()?, len.saturating_sub(1))),
        (start, end) => {
            let (s, e): (u64, u64) = (start.parse().ok()?, end.parse().ok()?);
            (s <= e).then_some((s, e.min(len.saturating_sub(1))))
        }
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A chapter file, opened for reading at any offset.
struct Source {
    len: u64,
    reader: Box<dyn ReadSeek>,
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

impl Source {
    fn read_at(&mut self, start: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.reader.seek(SeekFrom::Start(start))?;
        self.reader.read_exact(buf)
    }
}

#[cfg(not(target_os = "android"))]
fn open<R: Runtime>(app: &AppHandle<R>, file: &str) -> Option<Source> {
    use tauri::Manager;
    let path = app.path().resource_dir().ok()?.join("audio").join(file);
    let f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    Some(Source { len, reader: Box::new(f) })
}

#[cfg(target_os = "android")]
fn open<R: Runtime>(_app: &AppHandle<R>, file: &str) -> Option<Source> {
    let asset = android::assets()?.open(&std::ffi::CString::new(format!("audio/{}", file)).ok()?)?;
    let len = asset.length() as u64;
    Some(Source { len, reader: Box::new(asset) })
}

#[cfg(target_os = "android")]
mod android {
    use std::ptr::NonNull;
    use std::sync::OnceLock;

    use ndk::asset::AssetManager;

    struct Shared(NonNull<ndk_sys::AAssetManager>);
    // The native asset manager is safe to use from any thread; its Java object is kept
    // alive (a global reference never released) for the life of the app
    unsafe impl Send for Shared {}
    unsafe impl Sync for Shared {}

    /// The APK's assets, through the activity's AssetManager.
    pub fn assets() -> Option<AssetManager> {
        static MANAGER: OnceLock<Option<Shared>> = OnceLock::new();
        let shared = MANAGER.get_or_init(|| {
            let ctx = ndk_context::android_context();
            let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
            let mut env = vm.attach_current_thread().ok()?;
            let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
            let assets = env
                .call_method(&context, "getAssets", "()Landroid/content/res/AssetManager;", &[])
                .ok()?
                .l()
                .ok()?;
            let global = env.new_global_ref(assets).ok()?;
            let ptr = unsafe { ndk_sys::AAssetManager_fromJava(env.get_raw().cast(), global.as_obj().as_raw().cast()) };
            std::mem::forget(global);
            NonNull::new(ptr).map(Shared)
        });
        shared.as_ref().map(|s| unsafe { AssetManager::from_ptr(s.0) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=100-199", 1000), Some((100, 199)));
        assert_eq!(parse_range("bytes=900-5000", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=200-100", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }

    #[test]
    fn bodies() {
        let source = || Source { len: 10, reader: Box::new(std::io::Cursor::new(b"0123456789".to_vec())) };
        assert_eq!(read_body(&mut source(), 3, 6, false).unwrap(), b"3456");
        assert_eq!(read_body(&mut source(), 0, 9, false).unwrap(), b"0123456789");
        assert_eq!(read_body(&mut source(), 3, 6, true).unwrap(), b"\0\0\x003456");
        assert_eq!(read_body(&mut source(), 0, 9, true).unwrap(), b"0123456789");
        assert!(read_body(&mut source(), 8, 12, false).is_err());
    }

    #[test]
    fn decoding() {
        assert_eq!(percent_decode("bsb-souer%2FGEN.1.ogg"), "bsb-souer/GEN.1.ogg");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%2e%2E%2F"), "../");
    }
}
