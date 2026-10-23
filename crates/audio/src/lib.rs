//! Downloads the audio Bibles when the reader asks for them (docs/AUDIO.md).
//!
//! A recording is fetched book by book from its release of the app's repository on
//! GitHub (`kjv_core::audio::Download`). Each book's file is its chapter files end to end,
//! and each chapter is checked against the size and SHA-256 built into the app as it
//! arrives: only a chapter that matches is kept, under a name that carries its hash
//! (`<recording>/<BOOK>.<CH>.<hash>.ogg` in the downloads folder), and it can be played at
//! once. A download that stopped (cancelled, the connection lost, the app closed) carries
//! on from the first chapter missing, with an HTTP range request. Nothing is fetched
//! unless the reader asks. Shared by the app and the dev server.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::future::Future;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kjv_core::audio::{BookFile, Download};
use kjv_library::books;
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

// ---------------------------------------------------------------- where from

/// Where the recordings are downloaded from.
#[derive(Debug, Clone)]
pub struct Host {
    /// What the reader is told it is: "GitHub"
    name: String,
    /// The releases' address: a book's file is `<base>/<release>/<recording>.<BOOK>.ogg`
    base: String,
    /// The only origins a download may come from, redirects included
    origins: Vec<String>,
}

impl Host {
    /// The app's releases on GitHub, which hand each file over from GitHub's file servers.
    pub fn github() -> Self {
        Host {
            name: "GitHub".into(),
            base: "https://github.com/Divhanthelion/Scriptorium/releases/download".into(),
            origins: ["https://github.com", "https://release-assets.githubusercontent.com", "https://objects.githubusercontent.com"]
                .map(String::from)
                .to_vec(),
        }
    }

    /// A stand-in for GitHub at `base` ("http://localhost:1420/test-releases"): the dev
    /// server's, for the UI tests, or a test's. Only its own origin is allowed.
    pub fn stand_in(base: &str) -> Result<Self, String> {
        let url = Url::parse(base).map_err(|e| format!("{}: {}", base, e))?;
        Ok(Host { name: "the test server".into(), base: base.trim_end_matches('/').into(), origins: vec![origin(&url)] })
    }

    fn url(&self, d: &Download, code: &str) -> String {
        format!("{}/{}/{}", self.base, d.release, d.asset(code))
    }

    fn allows(&self, url: &Url) -> bool {
        self.origins.contains(&origin(url))
    }
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

// ---------------------------------------------------------------- what's here

/// The downloads folder: `<recording>/<BOOK>.<CH>.<hash>.ogg`, each written only once
/// its SHA-256 matched.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Store { root }
    }

    /// A chapter's downloaded file ("bsb-souer/GEN.1.ogg"), if it's here whole: under the
    /// name its SHA-256 gives it, and as long as the manifest says.
    pub fn find(&self, file: &str) -> Option<PathBuf> {
        let (name, size) = kjv_core::audio::stored_file(file)?;
        let path = self.root.join(name);
        (fs::metadata(&path).ok()?.len() == size).then_some(path)
    }

    /// The files in a recording's folder, and their sizes.
    fn listing(&self, d: &Download) -> HashMap<String, u64> {
        let Ok(entries) = fs::read_dir(self.root.join(&d.recording)) else {
            return HashMap::new();
        };
        entries
            .filter_map(|e| {
                let e = e.ok()?;
                Some((e.file_name().to_string_lossy().into_owned(), e.metadata().ok()?.len()))
            })
            .collect()
    }

    /// For each of the recording's books, which of its chapters are here (chapter 1 first).
    pub fn present(&self, d: &Download) -> Vec<Vec<bool>> {
        let found = self.listing(d);
        d.books.iter().map(|b| present_in(b, &found)).collect()
    }

    fn delete(&self, recording: &str) -> std::io::Result<()> {
        match fs::remove_dir_all(self.root.join(recording)) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

fn present_in(book: &BookFile, found: &HashMap<String, u64>) -> Vec<bool> {
    let name = |i: usize| book.chapters[i].stored_name(book.code, i + 1);
    (0..book.chapters.len()).map(|i| found.get(&name(i)) == Some(&book.chapters[i].size)).collect()
}

// ---------------------------------------------------------------- what the reader sees

/// A recording: whether its files came with the app, and what of it is downloaded.
#[derive(Debug, Serialize)]
pub struct Recording {
    pub recording: String,
    pub reader: String,
    pub bibles: Vec<String>,
    /// Its files came with this build of the app
    pub included: bool,
    /// None: it isn't offered for download
    pub download: Option<Status>,
}

#[derive(Debug, Serialize)]
pub struct Status {
    /// The whole recording, and how much of it is on this device, in bytes
    pub size: u64,
    pub have: u64,
    pub books: Vec<BookStatus>,
    /// Downloading now: the book being fetched, and those waiting
    pub running: bool,
    pub book: Option<&'static str>,
    pub queued: Vec<&'static str>,
    /// Bytes of the chapter arriving now
    pub partial: u64,
    /// Why the last download stopped, if it failed
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BookStatus {
    pub code: &'static str,
    /// "1 Samuel"
    pub name: &'static str,
    /// The book, and how much of it is on this device, in bytes
    pub size: u64,
    pub have: u64,
    /// "1" for each chapter on this device and "0" for each not, chapter 1 first
    pub chapters: String,
}

/// What the reader asks to download: a recording, some of its books (codes; all of it if
/// none are named), and where they are, which comes first (that book from that chapter
/// on, then the chapters before it), so what they want to hear arrives first.
#[derive(Debug, Deserialize)]
pub struct Want {
    pub recording: String,
    pub books: Option<Vec<String>>,
    pub from: Option<At>,
}

#[derive(Debug, Deserialize)]
pub struct At {
    pub book: String,
    pub chapter: usize,
}

// ---------------------------------------------------------------- downloading

pub struct Downloads {
    store: Store,
    host: Host,
    client: reqwest::Client,
    /// By recording
    jobs: Mutex<HashMap<String, Job>>,
}

#[derive(Default)]
struct Job {
    /// Books still to fetch, each from the chapter to start at
    queue: VecDeque<(&'static str, usize)>,
    running: bool,
    book: Option<&'static str>,
    partial: u64,
    error: Option<String>,
    cancel: Arc<Cancel>,
}

#[derive(Default)]
struct Cancel {
    asked: AtomicBool,
    wake: Notify,
}

/// Resolves once the download is cancelled.
async fn cancelled(cancel: &Cancel) {
    while !cancel.asked.load(Ordering::SeqCst) {
        cancel.wake.notified().await;
    }
}

enum Stop {
    Cancelled,
    Failed(String),
}

impl Downloads {
    pub fn new(store: Store, host: Host) -> Self {
        let allowed = host.clone();
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            // Between chunks: a stalled connection fails rather than hangs
            .read_timeout(Duration::from_secs(60))
            // Only ever to the host's own servers
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("too many redirects")
                } else if allowed.allows(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirected off the host")
                }
            }))
            .referer(false)
            .user_agent(concat!("Scriptorium/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client builds");
        Downloads { store, host, client, jobs: Mutex::default() }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Every recording the app has, with what of it is downloaded; `included` says whether
    /// a recording's files came with the app.
    pub fn recordings(&self, included: impl Fn(&str) -> bool) -> Vec<Recording> {
        kjv_core::audio::recordings()
            .into_iter()
            .map(|r| Recording {
                included: included(&r.id),
                download: kjv_core::audio::download(&r.id).map(|d| self.status(d)),
                recording: r.id,
                reader: r.reader,
                bibles: r.bibles,
            })
            .collect()
    }

    fn status(&self, d: &'static Download) -> Status {
        let present = self.store.present(d);
        let books: Vec<BookStatus> = d
            .books
            .iter()
            .zip(&present)
            .map(|(b, p)| BookStatus {
                code: b.code,
                name: display(b.code),
                size: b.size(),
                have: b.chapters.iter().zip(p).filter(|(_, here)| **here).map(|(c, _)| c.size).sum(),
                chapters: p.iter().map(|&here| if here { '1' } else { '0' }).collect(),
            })
            .collect();
        let have = books.iter().map(|b| b.have).sum();
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.get(&d.recording);
        Status {
            size: d.size(),
            have,
            books,
            running: job.is_some_and(|j| j.running),
            book: job.and_then(|j| j.book),
            queued: job.map(|j| j.queue.iter().map(|q| q.0).collect()).unwrap_or_default(),
            partial: job.map_or(0, |j| j.partial),
            error: job.and_then(|j| j.error.clone()),
        }
    }

    /// Download what the reader asked for. Returns the work to spawn if the recording isn't
    /// downloading already; one that is takes the books on.
    pub fn start(self: &Arc<Self>, want: &Want) -> Result<Option<impl Future<Output = ()> + Send + 'static>, String> {
        let d = kjv_core::audio::download(&want.recording).ok_or_else(|| format!("{} can't be downloaded", want.recording))?;
        self.queue(d, want.books.as_deref(), want.from.as_ref().map(|at| (at.book.as_str(), at.chapter)))
    }

    fn queue(
        self: &Arc<Self>,
        d: &'static Download,
        books: Option<&[String]>,
        from: Option<(&str, usize)>,
    ) -> Result<Option<impl Future<Output = ()> + Send + 'static>, String> {
        let mut wanted: Vec<(&'static str, usize)> = match books {
            None => d.books.iter().map(|b| (b.code, 1)).collect(),
            Some(codes) => {
                let mut list = Vec::new();
                for code in codes {
                    let book = d.book(code).ok_or_else(|| format!("{} has no book {}", d.recording, code))?;
                    list.push((book.code, 1));
                }
                list
            }
        };
        let first = from.and_then(|(code, n)| d.book(code).map(|b| (b.code, n.clamp(1, b.chapters.len()))));
        wanted.retain(|w| Some(w.0) != first.map(|f| f.0));

        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.entry(d.recording.clone()).or_default();
        job.error = None;
        for w in wanted {
            if job.book != Some(w.0) && !job.queue.iter().any(|q| q.0 == w.0) {
                job.queue.push_back(w);
            }
        }
        if let Some(first) = first {
            job.queue.retain(|q| q.0 != first.0);
            job.queue.push_front(first);
        }
        if job.running {
            // A cancelled download still winding down carries on with these instead
            if job.cancel.asked.load(Ordering::SeqCst) {
                job.cancel = Arc::default();
            }
            return Ok(None);
        }
        job.running = true;
        job.cancel = Arc::default();
        let this = self.clone();
        Ok(Some(async move { this.run(d).await }))
    }

    /// Stop downloading `recording`: the chapters already checked stay; the one arriving
    /// is dropped.
    pub fn cancel(&self, recording: &str) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(recording) {
            job.queue.clear();
            job.cancel.asked.store(true, Ordering::SeqCst);
            job.cancel.wake.notify_one();
        }
    }

    /// Delete everything downloaded of `recording` (not while it's downloading).
    pub fn delete(&self, recording: &str) -> Result<(), String> {
        let d = kjv_core::audio::download(recording).ok_or_else(|| format!("{} can't be downloaded", recording))?;
        self.remove(d)
    }

    fn remove(&self, d: &Download) -> Result<(), String> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.get(&d.recording).is_some_and(|j| j.running) {
            return Err("Cancel the download first.".into());
        }
        jobs.remove(&d.recording);
        self.store.delete(&d.recording).map_err(|e| format!("Couldn't delete the recording: {}", e))
    }

    async fn run(self: Arc<Self>, d: &'static Download) {
        loop {
            let (next, cancel) = {
                let mut jobs = self.jobs.lock().unwrap();
                let job = jobs.get_mut(&d.recording).expect("a running download has its job");
                match job.queue.pop_front() {
                    Some(next) if !job.cancel.asked.load(Ordering::SeqCst) => {
                        job.book = Some(next.0);
                        job.partial = 0;
                        (next, job.cancel.clone())
                    }
                    _ => {
                        *job = Job { error: job.error.take(), ..Job::default() };
                        return;
                    }
                }
            };
            let result = self.fetch_book(d, next.0, next.1, &cancel).await;
            self.tidy(d, next.0);
            let mut jobs = self.jobs.lock().unwrap();
            let job = jobs.get_mut(&d.recording).expect("a running download has its job");
            match result {
                Ok(()) => {}
                // Started again while it stopped: carry on with what was asked for since
                Err(Stop::Cancelled) if !job.cancel.asked.load(Ordering::SeqCst) => {}
                Err(stop) => {
                    let error = match stop {
                        Stop::Failed(message) => Some(message),
                        Stop::Cancelled => None,
                    };
                    *job = Job { error, ..Job::default() };
                    return;
                }
            }
        }
    }

    async fn fetch_book(&self, d: &'static Download, code: &'static str, from: usize, cancel: &Cancel) -> Result<(), Stop> {
        let book = d.book(code).expect("a book of the recording");
        let present = present_in(book, &self.store.listing(d));
        let dir = self.store.root.join(&d.recording);
        fs::create_dir_all(&dir).map_err(|e| io_failed(display(code), e))?;
        for (first, last) in runs(&present, from) {
            self.fetch_run(d, book, first, last, &dir, cancel).await?;
        }
        Ok(())
    }

    /// Chapters `first` to `last` of `book`, in one request.
    async fn fetch_run(
        &self,
        d: &'static Download,
        book: &'static BookFile,
        first: usize,
        last: usize,
        dir: &Path,
        cancel: &Cancel,
    ) -> Result<(), Stop> {
        let name = display(book.code);
        let start = book.offset(first);
        let end = book.offset(last) + book.chapters[last - 1].size - 1;
        let request = self.client.get(self.host.url(d, book.code)).header(RANGE, format!("bytes={}-{}", start, end));
        let mut response = tokio::select! {
            response = request.send() => response.map_err(|e| self.request_failed(name, &e))?,
            () = cancelled(cancel) => return Err(Stop::Cancelled),
        };
        let mut skip = match response.status() {
            StatusCode::PARTIAL_CONTENT => {
                let from = response
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes "))
                    .and_then(|v| v.split_once('-'))
                    .and_then(|(a, _)| a.parse::<u64>().ok());
                if from != Some(start) {
                    return Err(Stop::Failed(format!("{} sent the wrong part of {}.", self.host.name, name)));
                }
                0
            }
            // The whole file, the range ignored: read on to where the run starts
            StatusCode::OK => start,
            StatusCode::NOT_FOUND => return Err(Stop::Failed(format!("{} isn't on {} yet.", name, self.host.name))),
            status => return Err(Stop::Failed(format!("{} answered {} for {}.", self.host.name, status, name))),
        };

        let mut n = first;
        let mut part = Part::create(dir, book, n)?;
        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk,
                () = cancelled(cancel) => {
                    part.discard();
                    return Err(Stop::Cancelled);
                }
            };
            let chunk = match chunk {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(e) => {
                    part.discard();
                    return Err(self.request_failed(name, &e));
                }
            };
            let mut data: &[u8] = &chunk;
            let skipped = skip.min(data.len() as u64);
            data = &data[skipped as usize..];
            skip -= skipped;
            while !data.is_empty() {
                let take = part.room().min(data.len() as u64) as usize;
                part.write(&data[..take])?;
                data = &data[take..];
                if part.room() == 0 {
                    part.finish()?;
                    n += 1;
                    if n > last {
                        self.set_partial(d, 0);
                        return Ok(());
                    }
                    part = Part::create(dir, book, n)?;
                }
            }
            self.set_partial(d, part.received);
        }
        part.discard();
        Err(Stop::Failed(format!("The download of {} stopped part way. Resume to carry on.", name)))
    }

    fn set_partial(&self, d: &Download, bytes: u64) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&d.recording) {
            job.partial = bytes;
        }
    }

    /// Clears a book's leftovers: a chapter that was arriving when the app stopped, and
    /// chapters of an earlier release that this app's manifest no longer has.
    fn tidy(&self, d: &Download, code: &str) {
        let Some(book) = d.book(code) else { return };
        let keep: HashSet<String> = (0..book.chapters.len()).map(|i| book.chapters[i].stored_name(code, i + 1)).collect();
        for name in self.store.listing(d).into_keys() {
            if name.split('.').next() == Some(code) && !keep.contains(&name) {
                let _ = fs::remove_file(self.store.root.join(&d.recording).join(name));
            }
        }
    }

    fn request_failed(&self, name: &str, e: &reqwest::Error) -> Stop {
        let host = &self.host.name;
        Stop::Failed(if e.is_redirect() {
            format!("The download of {} was sent somewhere other than {}, so it was stopped.", name, host)
        } else if e.is_timeout() {
            format!("{} stopped answering. Resume to carry on.", host)
        } else if e.is_connect() {
            format!("Couldn't reach {}. Check your connection.", host)
        } else {
            format!("The download of {} from {} failed. Check your connection, then resume.", name, host)
        })
    }
}

/// The runs of chapters missing, (first, last) from 1: those from chapter `from` on
/// first, then those before it.
fn runs(present: &[bool], from: usize) -> Vec<(usize, usize)> {
    let (mut after, mut before) = (Vec::new(), Vec::new());
    let mut start = None;
    for n in 1..=present.len() + 1 {
        let missing = n <= present.len() && !present[n - 1];
        if let Some(s) = start
            && (!missing || n == from)
        {
            if s >= from { &mut after } else { &mut before }.push((s, n - 1));
            start = None;
        }
        if missing && start.is_none() {
            start = Some(n);
        }
    }
    after.extend(before);
    after
}

/// "Genesis", "1 Samuel"
fn display(code: &'static str) -> &'static str {
    books::by_code(code).map_or(code, |b| b.display)
}

fn io_failed(what: &str, e: std::io::Error) -> Stop {
    Stop::Failed(if e.kind() == ErrorKind::StorageFull {
        format!("There isn't enough space on this device for {}.", what)
    } else {
        format!("Couldn't save {}: {}", what, e)
    })
}

/// A chapter as it arrives: written to `<its name>.part` and hashed, and kept under its
/// name only if it matches.
struct Part {
    file: fs::File,
    path: PathBuf,
    done: PathBuf,
    hasher: Sha256,
    size: u64,
    sha256: [u8; 32],
    received: u64,
    label: String,
}

impl Part {
    fn create(dir: &Path, book: &BookFile, n: usize) -> Result<Part, Stop> {
        let c = &book.chapters[n - 1];
        let label = format!("{} {}", display(book.code), n);
        let done = dir.join(c.stored_name(book.code, n));
        let path = done.with_extension("ogg.part");
        let file = fs::File::create(&path).map_err(|e| io_failed(&label, e))?;
        Ok(Part { file, path, done, hasher: Sha256::new(), size: c.size, sha256: c.sha256, received: 0, label })
    }

    fn room(&self) -> u64 {
        self.size - self.received
    }

    /// (A part left by a failed write is cleared with the book's other leftovers.)
    fn write(&mut self, data: &[u8]) -> Result<(), Stop> {
        self.file.write_all(data).map_err(|e| io_failed(&self.label, e))?;
        self.hasher.update(data);
        self.received += data.len() as u64;
        Ok(())
    }

    /// Keep it if it matches the manifest, under its own name; drop it if it doesn't.
    fn finish(self) -> Result<(), Stop> {
        let Part { file, path, done, hasher, sha256, label, .. } = self;
        let synced = file.sync_all();
        drop(file);
        if <[u8; 32]>::from(hasher.finalize()) != sha256 {
            let _ = fs::remove_file(&path);
            return Err(Stop::Failed(format!("{} didn't match the app's checksum, so it wasn't kept. Try again.", label)));
        }
        synced.and_then(|()| fs::rename(&path, &done)).map_err(|e| {
            let _ = fs::remove_file(&path);
            io_failed(&label, e)
        })
    }

    fn discard(self) {
        let Part { file, path, .. } = self;
        drop(file);
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests;
