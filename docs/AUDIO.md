# Audio Bibles

Scriptorium ships audio recordings of whole Bibles inside the app: nothing is downloaded,
ever. A recording plays chapter by chapter, marks each verse as it's read (and keeps it in
view), starts at any verse, and carries on into the next chapter.

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
- **Recordings** (`app/audio/<recording>/<BOOK>.<CH>.ogg`, not in git: 0.53 GB for the
  BSB, 0.75 GB for the WEB, which is read more slowly): Ogg Opus, mono, 16 kbps, speech-tuned. `bundle.resources` ships the folder with the
  app; on Android it lands in the APK's assets. The app serves the files to the page itself,
  with byte ranges for seeking: the `audio` protocol (`app/src/audio.rs`) reads them from the
  resource folder on desktop and iOS, and through the AssetManager on Android, where Google
  Play's install-time asset packs also appear. Only a recording's own chapter files can be
  opened. (Android's WebView cuts a requested range out of the response body itself, so
  there each response starts at the file's first byte.)
- **Player** (`ui/js/listen.js`): the Listen button by the translation, the player bar, the
  verse bar's Listen, follow-along (Settings, Reading), speeds 0.75× to 2×, and the system's
  media controls.
- The dev server serves `app/audio/` at `/audio/`, so the browser preview plays them too.
- A build without the recordings works: Listen is offered, and the first try says the
  recording isn't included and hides it. (Once a recording has played, a chapter that fails
  only says so.)

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
