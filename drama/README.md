# Dramatized audio Bibles: the Reina-Valera 1909

A full-cast audio Bible made with Fish Audio: every word exactly as the app prints
it, every speaking character a voice of their own. This is the pipeline built for
the dramatized KJV in KJV Interlinear (`audio/` on its branch `audio/dramatized-kjv`),
copied here on 2026-10-09 from its working tree and taught Spanish, so the KJV's
own work isn't touched while it renders. The procedure is the same
(`docs/SOP.md`); what differs for the RV1909 is in `docs/decisions-rv1909.md`.

| | |
|---|---|
| `docs/SOP.md`, `docs/fish-audio.md`, `docs/casting.md` | the KJV's procedure and notes, as copied |
| `docs/decisions-rv1909.md` | the owner's decisions for the Spanish Bible, and what is open |
| `abible/script_es.py`, `abible/project_es.py` | the RV1909's script: who says each word |
| `abible/bible.py`, `abible/es.py` | the app's texts and verse alignment; Spanish numbers and announcements |
| `script/rv1909/` | the script (generated) and `review.json` |
| `cast/policy-rv1909.json` | the Spanish cast's accent |
| `cast/cast-rv1909.json` | the Spanish cast (generated) |
| `lexicon/rv1909.tsv` | names respelled for the voice (generated, then corrected by ear) |

`ABIBLE_BIBLE` names the translation a command is for (`kjv` by default, as copied).

## Running it

Python 3.12+; the script needs only the standard library.

```bash
# the app's texts, exactly as it reads them (from the repository root)
cargo run --release -p kjv-import --example verses -- rv1909 drama/.cache/text/rv1909.json
cargo run --release -p kjv-import --example verses -- blm drama/.cache/text/blm.json
cargo run --release -p kjv-import --example verses -- kjv drama/.cache/text/kjv.json

cd drama
python -m abible sources fetch               # Glyssen's files, pinned
python -m abible script rv1909               # script/rv1909/ and its review list
python tests/test_script_es.py               # every verse rejoins exactly; the rules
ABIBLE_BIBLE=rv1909 python -m abible lexicon rv1909
ABIBLE_BIBLE=rv1909 python -m abible cast    # cast/cast-rv1909.json
```

Voices and audio need the Fish Audio key (`FISH_API_KEY`, or in `drama/.env` or the
owner's `picture_ingest/.env`; never committed), and the same account's request limit
is shared with the KJV's work: don't run both at once.

```bash
ABIBLE_BIBLE=rv1909 python -m abible voices design narrator God Eve Adam serpent
ABIBLE_BIBLE=rv1909 python -m abible render GEN 3
```
