# Audio Bibles

The app downloads its recordings when the reader asks for them (docs/AUDIO.md), so its
packages ship none. A build can still pack recordings with the app, as a sideload or
development build does: put them here, one Ogg Opus file per chapter,
`<recording>/<BOOK>.<CH>.ogg` (for example `bsb-souer/JHN.3.ogg`), and `bundle.resources`
ships the folder. The app then plays them as they are and offers no download of them.
Each recording's verse timings, reader, licence, and credit are built into the app from
`data/audio/<recording>.json`.

The files are too large for git and are left out of it. They are made from the pinned
sources by the audio tools (`.cache/audio-tools`: download, transcribe, align, encode); see
docs/AUDIO.md, which also says how a recording is packed and published for download.
