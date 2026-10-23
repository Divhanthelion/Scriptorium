//! The downloader against a stand-in for GitHub on this machine: every chapter checked,
//! a chapter that doesn't match refused, a download cut off resumed from the first
//! chapter missing (with or without the server honouring ranges), what the reader wants
//! to hear first, redirects kept to the host, cancel, and delete.

use std::io::{Cursor, Read};
use std::thread;
use std::time::Instant;

use kjv_core::audio::ChapterFile;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Answers a range with just that range
    Ranges,
    /// Ignores ranges: the whole file every time
    Whole,
    /// Answers a range, but stops after this many bytes
    CutAfter(usize),
    /// Answers a range, a kilobyte every 10 ms
    Slow,
}

struct Server {
    port: u16,
    /// Each request's Range header, in order
    ranges: Arc<Mutex<Vec<String>>>,
    mode: Arc<Mutex<Mode>>,
}

impl Server {
    fn host(&self, path: &str) -> Host {
        Host::stand_in(&format!("http://127.0.0.1:{}/{}", self.port, path)).unwrap()
    }

    fn ranges(&self) -> Vec<String> {
        self.ranges.lock().unwrap().clone()
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }
}

struct SlowReader(Cursor<Vec<u8>>);

impl Read for SlowReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        thread::sleep(Duration::from_millis(10));
        let n = buf.len().min(1000);
        self.0.read(&mut buf[..n])
    }
}

/// Serves `files` ("<release>/<asset>") at /files/; /hop/ redirects there, /away/ to
/// another origin (localhost rather than 127.0.0.1).
fn serve(files: HashMap<String, Vec<u8>>, mode: Mode) -> Server {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let mode = Arc::new(Mutex::new(mode));
    let (log, modes) = (ranges.clone(), mode.clone());
    let header = |name: &str, value: &str| tiny_http::Header::from_bytes(name, value).unwrap();
    thread::spawn(move || {
        for request in server.incoming_requests() {
            let url = request.url().to_string();
            let redirect = [("/hop/", "127.0.0.1"), ("/away/", "localhost")]
                .iter()
                .find_map(|(prefix, origin)| url.strip_prefix(prefix).map(|rest| format!("http://{}:{}/files/{}", origin, port, rest)));
            if let Some(to) = redirect {
                let _ = request.respond(tiny_http::Response::empty(302).with_header(header("Location", &to)));
                continue;
            }
            let Some(bytes) = url.strip_prefix("/files/").and_then(|path| files.get(path)) else {
                let _ = request.respond(tiny_http::Response::empty(404));
                continue;
            };
            let range = request.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.to_string());
            log.lock().unwrap().push(range.clone().unwrap_or_default());
            let mode = *modes.lock().unwrap();
            let span = range
                .as_deref()
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.split_once('-'))
                .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()))
                .filter(|_| mode != Mode::Whole);
            let (status, body, headers) = match span {
                Some((a, b)) => {
                    let content_range = header("Content-Range", &format!("bytes {}-{}/{}", a, b, bytes.len()));
                    (206, bytes[a..=b].to_vec(), vec![content_range])
                }
                None => (200, bytes.clone(), vec![]),
            };
            // Cut off and slow ones are sent chunked: the client sees the body end early
            let (reader, len): (Box<dyn Read + Send>, Option<usize>) = match mode {
                Mode::CutAfter(k) => (Box::new(Cursor::new(body[..k.min(body.len())].to_vec())), None),
                Mode::Slow => (Box::new(SlowReader(Cursor::new(body))), None),
                _ => {
                    let len = body.len();
                    (Box::new(Cursor::new(body)), Some(len))
                }
            };
            let response = tiny_http::Response::new(status.into(), headers, reader, len, None);
            let _ = request.respond(response);
        }
    });
    Server { port, ranges, mode }
}

/// Chapters of these sizes, each with bytes of its own.
fn chapters(sizes: &[usize]) -> Vec<Vec<u8>> {
    sizes.iter().enumerate().map(|(i, &size)| (0..size).map(|j| (j * 7 + i * 31) as u8).collect()).collect()
}

/// A recording of these books, as the app's manifest would describe it.
fn recording(id: &str, books: &[(&'static str, &[Vec<u8>])]) -> &'static Download {
    Box::leak(Box::new(Download {
        recording: id.into(),
        release: format!("audio-{}-1", id),
        books: books
            .iter()
            .map(|(code, chapters)| BookFile {
                code,
                chapters: chapters.iter().map(|c| ChapterFile { size: c.len() as u64, sha256: Sha256::digest(c).into() }).collect(),
            })
            .collect(),
    }))
}

/// What the release would hold: each book's chapters end to end.
fn release(d: &Download, books: &[(&str, &[Vec<u8>])]) -> HashMap<String, Vec<u8>> {
    books.iter().map(|(code, chapters)| (format!("{}/{}", d.release, d.asset(code)), chapters.concat())).collect()
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kjv-audio-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn start(downloads: &Arc<Downloads>, d: &'static Download, books: Option<&[String]>, from: Option<(&str, usize)>) {
    if let Some(work) = downloads.queue(d, books, from).unwrap() {
        tokio::spawn(work);
    }
}

async fn until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(20), "timed out waiting: {}", what);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits for the download to stop, and says how it ended.
async fn finished(downloads: &Downloads, d: &'static Download) -> Status {
    until("the download to stop", || !downloads.status(d).running).await;
    downloads.status(d)
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir).map(|e| e.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn every_chapter_is_checked_and_kept_under_its_hash() {
    let ruth = chapters(&[5000, 12000, 7000]);
    let d = recording("t-whole", &[("RUT", &ruth)]);
    let server = serve(release(d, &[("RUT", &ruth)]), Mode::Ranges);
    let dir = folder("whole");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!((status.error.as_deref(), status.have, status.size), (None, 24000, 24000));
    assert_eq!(status.books[0].chapters, "111");
    assert_eq!(server.ranges(), ["bytes=0-23999"], "one request for the book");
    for (i, chapter) in ruth.iter().enumerate() {
        let name = d.books[0].chapters[i].stored_name("RUT", i + 1);
        assert_eq!(&fs::read(dir.join("t-whole").join(name)).unwrap(), chapter);
    }
    assert_eq!(files_in(&dir.join("t-whole")).len(), 3, "nothing else left");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chapter_that_does_not_match_is_not_kept() {
    let ruth = chapters(&[5000, 12000, 7000]);
    let d = recording("t-bad", &[("RUT", &ruth)]);
    let mut files = release(d, &[("RUT", &ruth)]);
    files.values_mut().next().unwrap()[9000] ^= 1;
    let server = serve(files, Mode::Ranges);
    let dir = folder("bad");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!(status.books[0].chapters, "100", "Ruth 1 kept, Ruth 2 not, Ruth 3 not reached");
    assert_eq!(status.error.as_deref(), Some("Ruth 2 didn't match the app's checksum, so it wasn't kept. Try again."));
    assert_eq!(files_in(&dir.join("t-bad")), [d.books[0].chapters[0].stored_name("RUT", 1)], "no part left behind");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_cut_off_resumes_from_the_first_chapter_missing() {
    let ruth = chapters(&[5000, 12000, 7000]);
    let d = recording("t-cut", &[("RUT", &ruth)]);
    let server = serve(release(d, &[("RUT", &ruth)]), Mode::CutAfter(8000));
    let dir = folder("cut");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!(status.books[0].chapters, "100");
    assert_eq!(status.error.as_deref(), Some("The download of Ruth stopped part way. Resume to carry on."));

    server.set(Mode::Ranges);
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!((status.books[0].chapters.as_str(), status.error), ("111", None));
    assert_eq!(server.ranges(), ["bytes=0-23999", "bytes=5000-23999"], "resumed at Ruth 2");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_ignores_ranges_still_resumes_in_the_right_place() {
    let ruth = chapters(&[5000, 12000, 7000]);
    let d = recording("t-whole-file", &[("RUT", &ruth)]);
    let server = serve(release(d, &[("RUT", &ruth)]), Mode::Whole);
    let dir = folder("whole-file");
    fs::create_dir_all(dir.join("t-whole-file")).unwrap();
    fs::write(dir.join("t-whole-file").join(d.books[0].chapters[0].stored_name("RUT", 1)), &ruth[0]).unwrap();
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    assert_eq!(downloads.status(d).books[0].chapters, "100", "Ruth 1 is here already");
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!((status.books[0].chapters.as_str(), status.error), ("111", None));
    assert_eq!(server.ranges(), ["bytes=5000-23999"]);
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn what_the_reader_wants_to_hear_comes_first() {
    let ruth = chapters(&[5000, 12000, 7000, 3000]);
    let jude = chapters(&[4000]);
    let books: &[(&str, &[Vec<u8>])] = &[("RUT", &ruth), ("JUD", &jude)];
    let d = recording("t-first", books);
    let server = serve(release(d, books), Mode::Ranges);
    let dir = folder("first");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    // The whole Bible, from Ruth 3
    start(&downloads, d, None, Some(("RUT", 3)));
    let status = finished(&downloads, d).await;
    assert_eq!((status.have, status.error), (31000, None));
    assert_eq!(server.ranges(), ["bytes=17000-26999", "bytes=0-16999", "bytes=0-3999"], "Ruth 3 on, then 1-2, then Jude");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_book_asked_for_during_a_download_comes_next() {
    let ruth = chapters(&[20000, 20000]);
    let jude = chapters(&[4000]);
    let esther = chapters(&[3000]);
    let books: &[(&str, &[Vec<u8>])] = &[("RUT", &ruth), ("EST", &esther), ("JUD", &jude)];
    let d = recording("t-next", books);
    let server = serve(release(d, books), Mode::Slow);
    let dir = folder("next");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    until("Ruth to start", || downloads.status(d).book == Some("RUT")).await;
    // Listen on Jude while Ruth downloads
    assert!(downloads.queue(d, Some(&["JUD".to_string()]), Some(("JUD", 1))).unwrap().is_none(), "the running download takes it on");
    assert_eq!(downloads.status(d).queued, ["JUD", "EST"]);
    let status = finished(&downloads, d).await;
    assert_eq!((status.have, status.error), (47000, None));
    assert_eq!(server.ranges(), ["bytes=0-39999", "bytes=0-3999", "bytes=0-2999"], "Ruth, then Jude, then Esther");
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_stay_with_the_host() {
    let ruth = chapters(&[5000]);
    let d = recording("t-hop", &[("RUT", &ruth)]);
    let server = serve(release(d, &[("RUT", &ruth)]), Mode::Ranges);
    let dir = folder("hop");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("hop")));
    start(&downloads, d, None, None);
    assert_eq!(finished(&downloads, d).await.books[0].chapters, "1", "followed to the host's own file server");

    let dir = folder("away");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("away")));
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!(status.books[0].chapters, "0");
    assert_eq!(status.error.as_deref(), Some("The download of Ruth was sent somewhere other than the test server, so it was stopped."));
    assert_eq!(server.ranges().len(), 1, "never fetched from the other origin");
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(folder("hop"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_book_not_released_is_reported() {
    let ruth = chapters(&[5000]);
    let d = recording("t-missing", &[("RUT", &ruth)]);
    let server = serve(HashMap::new(), Mode::Ranges);
    let dir = folder("missing");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!(status.error.as_deref(), Some("Ruth isn't on the test server yet."));
    // Starting again clears the error
    server.set(Mode::Slow);
    start(&downloads, d, None, None);
    assert_eq!(downloads.status(d).error, None);
    let _ = finished(&downloads, d).await;
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_keeps_the_chapters_checked_and_delete_waits_for_it() {
    let ruth = chapters(&[20000, 20000, 20000]);
    let d = recording("t-cancel", &[("RUT", &ruth)]);
    let server = serve(release(d, &[("RUT", &ruth)]), Mode::Slow);
    let dir = folder("cancel");
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    until("Ruth 1", || downloads.status(d).books[0].chapters.starts_with('1')).await;
    assert_eq!(downloads.remove(d), Err("Cancel the download first.".to_string()));
    downloads.cancel("t-cancel");
    let status = finished(&downloads, d).await;
    assert_eq!((status.books[0].chapters.as_str(), status.error), ("100", None), "cancelled, not failed");
    assert_eq!(files_in(&dir.join("t-cancel")), [d.books[0].chapters[0].stored_name("RUT", 1)], "the part arriving was dropped");

    // Started again, then cancelled and started again at once: it carries on
    server.set(Mode::Ranges);
    downloads.cancel("t-cancel");
    start(&downloads, d, None, None);
    let status = finished(&downloads, d).await;
    assert_eq!((status.books[0].chapters.as_str(), status.error), ("111", None));

    downloads.remove(d).unwrap();
    assert!(!dir.join("t-cancel").exists());
    assert_eq!(downloads.status(d).have, 0);
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn leftovers_of_a_book_are_cleared() {
    let ruth = chapters(&[5000, 6000]);
    let d = recording("t-tidy", &[("RUT", &ruth)]);
    let server = serve(release(d, &[("RUT", &ruth)]), Mode::Ranges);
    let dir = folder("tidy");
    let folder = dir.join("t-tidy");
    fs::create_dir_all(&folder).unwrap();
    // A chapter of an earlier release, a part the app was closed during, another book's
    for name in ["RUT.1.0123456789abcdef.ogg", "RUT.2.0000000000000000.ogg.part", "JUD.1.0123456789abcdef.ogg"] {
        fs::write(folder.join(name), b"old").unwrap();
    }
    let downloads = Arc::new(Downloads::new(Store::new(dir.clone()), server.host("files")));
    start(&downloads, d, None, None);
    assert_eq!(finished(&downloads, d).await.books[0].chapters, "11");
    let mut expected = vec!["JUD.1.0123456789abcdef.ogg".to_string(), d.books[0].chapters[0].stored_name("RUT", 1), d.books[0].chapters[1].stored_name("RUT", 2)];
    expected.sort();
    assert_eq!(files_in(&folder), expected);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_chapters_come_in_runs_from_where_the_reader_is() {
    let (t, f) = (true, false);
    assert_eq!(runs(&[f, f, f], 1), [(1, 3)]);
    assert_eq!(runs(&[f, f, f], 2), [(2, 3), (1, 1)]);
    assert_eq!(runs(&[t, f, f, t, f], 1), [(2, 3), (5, 5)]);
    assert_eq!(runs(&[t, f, f, t, f], 3), [(3, 3), (5, 5), (2, 2)]);
    assert_eq!(runs(&[t, t], 1), []);
    assert_eq!(runs(&[f], 1), [(1, 1)]);
}

#[test]
fn only_the_hosts_own_servers() {
    let github = Host::github();
    for ok in ["https://github.com/x", "https://release-assets.githubusercontent.com/y?z", "https://objects.githubusercontent.com/a"] {
        assert!(github.allows(&Url::parse(ok).unwrap()), "{}", ok);
    }
    for bad in ["http://github.com/x", "https://github.com.evil.example/x", "https://gist.github.com/x", "https://github.com:8443/x"] {
        assert!(!github.allows(&Url::parse(bad).unwrap()), "{}", bad);
    }
    let d = recording("t-url", &[("RUT", &chapters(&[1]))]);
    assert_eq!(github.url(d, "RUT"), "https://github.com/Divhanthelion/Scriptorium/releases/download/audio-t-url-1/t-url.RUT.ogg");
    let local = Host::stand_in("http://localhost:1420/test-releases/").unwrap();
    assert_eq!(local.url(d, "RUT"), "http://localhost:1420/test-releases/audio-t-url-1/t-url.RUT.ogg");
    assert!(!local.allows(&Url::parse("http://127.0.0.1:1420/x").unwrap()));
}
