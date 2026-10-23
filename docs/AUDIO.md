# Audio Bibles

Scriptorium plays audio recordings of whole Bibles. They are too large to ship inside the
app (1.3 GB together; Google Play takes at most 500 MB in an app's base module), so each is
downloaded when the reader asks for it, a book or the whole Bible at a time, from the app's
releases on GitHub. Every chapter is checked against a SHA-256 built into the app before it
is kept. Nothing else in the app goes online. A recording plays chapter by chapter, marks
each verse as it's read (and keeps it in view), starts at any verse, and carries on into the
next chapter.

| Recording | Reads | Reader | Licence | Source |
|---|---|---|---|---|
| `bsb-souer` | Berean Standard Bible (`bsb`) | Bob Souer | CC0 1.0 | [openbible.com/audio/souer](https://openbible.com/audio/souer/) |
| `web-henson` | World English Bible Classic (`webclassic`) | Winfred W. Henson | Public domain | [ebible.org/eng-web/audio](https://ebible.org/eng-web/audio/) |

Henson's reading follows the WEB Classic ("Yahweh" in the Old Testament), not the 2020
WEB, so it is offered with that edition. He recorded an earlier revision of the WEB,
though: in about 50 chapters a phrase is worded a little differently from the text shown
(2 Corinthians 12:2 reads "I know a man in Christ fourteen years ago … such a one caught up
into the third heaven"). The verse timings are unaffected; his credit says so.

How well the timings fit (October 2026): Bible Hub publishes verse timings for Souer's
reading, made independently. Leaving out each chapter's verse 1 (Bible Hub starts it inside
the chapter announcement), ours differ from theirs by 0.11 s at the median; 90% of verses
are within 0.4 s and 99% within 0.9 s, and 149 of 29,897 differ by more than a second, mostly
in lists of names.

## In the app

- **Timings** (`data/audio/<recording>.json`, committed): the reader, licence, credit, and
  each chapter's length and verse starts. `kjv-core` builds them in (`crates/core/build.rs`,
  `src/audio.rs`); the commands `audio_recordings` and `audio_chapter` serve them.
- **Recordings**: one Ogg Opus file per chapter, mono, 16 kbps, speech-tuned: 0.53 GB for
  the BSB, 0.75 GB for the WEB, which is read more slowly.
- **Manifests** (`data/audio/downloads/<recording>.json`, committed, built in by `kjv-core`):
  the GitHub release that holds a recording, and each chapter's size and SHA-256, one
  chapter a line. A recording is offered for download only if it has one (and its
  catalogue is built in). They are written by `audio-pack` (below), never by hand.
- **Downloads** (`crates/audio`, `kjv-audio`): fetches a book's file from
  `https://github.com/Divhanthelion/Scriptorium/releases/download/<release>/<recording>.<BOOK>.ogg`,
  following redirects only to GitHub's own servers (github.com,
  release-assets.githubusercontent.com, objects.githubusercontent.com). Each chapter is
  written to a `.part` file and hashed as it arrives, and kept only if its size and SHA-256
  match the manifest, as `<BOOK>.<CH>.<first 16 hex digits of its SHA-256>.ogg`: it can play
  at once. The app plays a downloaded chapter only under the name its manifest gives it, so
  a chapter changed in a later release is downloaded again rather than played with timings
  that no longer fit it. A download that stopped (cancelled, the connection lost, the app
  closed) carries on from the first chapter missing, with an HTTP range request (or, from a
  server that ignores ranges, by reading on to it). What the reader wants to hear comes
  first: Listen on Psalm 119 fetches Psalms 119 to 150, then 1 to 118. Nothing is fetched
  unless the reader asks, not even to resume.
- **Where they're kept**: `audio/` in the app's local data folder:
  `%LOCALAPPDATA%\io.github.divhanthelion.scriptorium` on Windows (inside the package's own
  folder when installed from the Store), `~/Library/Application Support/io.github.divhanthelion.scriptorium`
  on macOS, `~/.local/share/io.github.divhanthelion.scriptorium` on Linux, and the app's
  private storage on Android, left out of cloud backups and device transfers
  (`res/xml/backup_rules.xml`, `data_extraction_rules.xml`): they would take a backup past
  Android's 25 MB, which stops it backing up anything. On iOS, the Caches folder, which iOS
  never backs up; it may clear it when the phone runs short of space, and the app then shows
  the recording as not downloaded. (Application Support marked "don't back up" would keep
  them; that needs native code built and tested on a Mac.) Uninstalling removes them on
  Android and iOS and from a Microsoft Store install; the Windows installer's uninstaller
  offers to; on macOS and Linux they stay until deleted (Settings, Audio Bibles).
- **Shipped with a build** (`app/audio/<recording>/<BOOK>.<CH>.ogg`, not in git): a build can
  still pack recordings with the app, as a sideload or development build does.
  `bundle.resources` ships the folder; on Android it lands in the APK's assets. Such a
  recording shows as "Comes with this app" and isn't downloaded.
- **Serving them**: the `audio` protocol (`app/src/audio.rs`) serves a chapter to the page,
  with byte ranges for seeking: downloaded, or else shipped with the app (from the resource
  folder on desktop and iOS, through the AssetManager on Android). Only a recording's own
  chapter files can be opened. (Android's WebView cuts a requested range out of the
  response body itself, so there each response starts at the file's first byte.)
- **Player** (`ui/js/listen.js`): the Listen button by the translation, the player bar, the
  verse bar's Listen, speeds 0.75× to 2×, and the system's media controls. Listen on a book
  not downloaded asks: download the book, or the whole Bible (each with its size), or not
  now; the player then opens, says how the download is going, and plays as soon as the
  chapter is here. At the end of a chapter it carries on into the next one if that is here
  or on its way; it never downloads a book unasked.
- **Settings, Audio Bibles** (`ui/js/panels.js`, `downloads.js`): each recording's size
  and how much is on the device; Download, Cancel, Download the rest, and Delete (which asks
  first). A download that fails says why ("Genesis isn't on GitHub yet.", "Couldn't reach
  GitHub. Check your connection.", "Genesis 5 didn't match the app's checksum, so it wasn't
  kept. Try again."). Follow-along is here too.
- **Dev server**: the same commands, with downloads in a fresh folder each run. It downloads
  from GitHub, or with `SCRIPTORIUM_AUDIO_HOST=test` from its own stand-in at
  `/test-releases/` (`tests/ui/fixtures/audio-releases/`: 3 John, for the UI tests), or from
  any address `SCRIPTORIUM_AUDIO_HOST` gives. It serves chapters at `/audio/`: downloaded,
  or from `app/audio/`.
- **Trying downloads on a phone before anything is published**: build a debug app with
  `SCRIPTORIUM_AUDIO_HOST` set to a server it can reach (the dev server's stand-in through
  `adb reverse tcp:1420 tcp:1420`, as `http://127.0.0.1:1420/test-releases`). A release build
  ignores it and only ever downloads from GitHub.

## Publishing a recording

A recording's release is made once and never changed: every version of the app keeps
downloading the files its manifest names. Uploading publishes the files, so it waits for the
owner's go-ahead.

1. Pack it, from its chapter files (`app/audio/<recording>/` by default, or the folder
   given):
   ```bash
   cargo run --release -p kjv-import -- audio-pack bsb-souer audio-bsb-souer-1
   ```
   This writes the release's 66 book files (each book's chapter files end to end, which also
   plays as one chained Ogg file), `SHA256SUMS`, and `notes.md` (the credit and source) to
   `target/audio-release/audio-bsb-souer-1/`, and the manifest to
   `data/audio/downloads/bsb-souer.json`. It checks that every chapter the catalogue lists
   has its file, and nothing else is there, and reads each book back against the manifest.
2. Commit the manifest; the app builds it in.
3. Publish the release, not marked latest (app releases are; their tags start with `v`, and
   `release.yml` builds only those):
   ```bash
   gh release create audio-bsb-souer-1 --repo Divhanthelion/Scriptorium --latest=false --title "Audio: BSB read by Bob Souer" --notes-file target/audio-release/audio-bsb-souer-1/notes.md target/audio-release/audio-bsb-souer-1/*.ogg target/audio-release/audio-bsb-souer-1/SHA256SUMS
   ```
   A release holds at most 1,000 files, each under 2 GiB; a recording is 66 (and a book
   under 50 MB). GitHub doesn't limit their total size or the bandwidth.
4. To check it: a debug build or the dev server downloads it whole, checking every chapter.
5. A recording whose audio changes (a chapter read again, timings redone) gets a new release,
   `audio-<recording>-2`, packed the same way; the old one stays for the versions of the app
   that use it. A chapter that didn't change keeps its name on the device and isn't
   downloaded again.

The dramatized KJV (`kjv-drama`) is offered the same way once it may be published: its
catalogue moves to `data/audio/`, it is packed as `audio-kjv-drama-1`, and its release is
uploaded. Until then it has no manifest, and the app doesn't offer it.

## How a recording is made

Everything runs from `.cache/audio-tools/` (git-ignored; a Python 3.12 environment made with
`uv`, faster-whisper on the GPU, ffmpeg). Sources go in `.cache/sources/audio/`.

1. **Download**, and record each file's SHA-256 (the BSB's Old Testament zip is Deflate64:
   it is extracted with `zipfile-deflate64`, every CRC checked).
2. **Inventory** (`inventory.py`): every file mapped to its book and chapter; each recording
   must have exactly the 1,189 chapters (the WEB has two byte-identical duplicates, dropped).
3. **Transcribe** (`asr.py`): Whisper medium.en with word timestamps, batched (about 50× real
   time on an RTX 3070).
4. **Align** (`align.py`): each chapter's transcript is matched word for word with the text
   the app shows (from the dev server's API, footnotes left out, line breaks as spaces).
   A verse starts where the voice resumes after the last pause before its first word ends:
   the recogniser's word ends are reliable, its starts after a pause aren't.
5. **Repair** (`gaps.py`, `asr.py --careful`): the batched recogniser sometimes drops a stretch
   of speech; chapters with a long unheard run (and, for the BSB, any verse more than 2 s from
   Bible Hub's published timings) are transcribed again in one careful pass, and the aligner
   uses whichever transcript hears more.
6. **Check** (`check_bh.py`): Bible Hub publishes verse timings for Souer's reading; ours agree
   to a median of about 0.1 s (verse 1 aside: Bible Hub starts it inside the chapter
   announcement).
7. **Export** (`export.py`) the timings to `data/audio/`, and **encode** (`encode.py`) the
   chapters to `app/audio/`, checking each file is as long as its source.
