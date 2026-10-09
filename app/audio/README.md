# Audio Bibles

The recordings that ship with the app, one Ogg Opus file per chapter:
`<recording>/<BOOK>.<CH>.ogg` (for example `bsb-souer/JHN.3.ogg`). Each recording's
verse timings, reader, licence, and credit are built into the app from
`data/audio/<recording>.json`.

The files are too large for git and are left out of it. A build without them works;
the app then offers no audio. They are made from the pinned sources by the audio tools
(`.cache/audio-tools`: download, transcribe, align, encode); see docs/AUDIO.md.
