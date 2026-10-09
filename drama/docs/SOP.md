# Standard operating procedure: a dramatized audio Bible, every voice its own

How we turn a Bible translation into a full-cast audio Bible with Fish Audio,
so that every word of the text is heard exactly as printed and every speaking
character has a voice of their own, the same voice wherever they speak.
Written while producing the KJV (October 2026); every other translation we
package follows the same steps. What the KJV taught us is marked
**(KJV lesson)**.

The quality bar is the app's: every word checked, nothing sampled where it can
be counted. Audio adds one thing text never needed, a person's ear, and the
procedure says exactly where a person listens.

```
 text ──► 1 sources ──► 2 script ──► 3 cast ──► 4 lexicon ──► 5 render ──► 6 check ──► 7 package
 (pinned)              (who says    (a voice    (names,        (Fish,        (words,      (MP3 +
                        each word)   for each)   heteronyms)    by chapter)   voices,      timings,
                                                                              sound)       credits)
```

---

## 0. Before starting a translation

Answer these and write the answers in `docs/decisions.md`:

1. **May we record it?** The text's licence must allow an audio rendering and
   distribution (public domain, CC BY, CC BY-SA, or written permission).
   CC BY-ND forbids derivatives: an audio recording is arguably one. Ask
   before recording any ND or NC text.
2. **May we distribute the audio?** See Fish's terms (docs/fish-audio.md,
   "Licence").
3. **Which quotation style?** This decides step 2:
   - **A. Quotation marks** (WEB, BSB, ASV with marks, most modern texts):
     the translation marks its own direct speech.
   - **B. No quotation marks** (KJV, and other older texts): speech has to
     be located.
4. **What is outside the verses?** Book titles, chapter numbers, Psalm titles,
   section headings, colophons, acrostic letters. Decide which are read
   (KJV: titles, chapters, Psalm titles as verse 0, Psalm 119's letters and
   the epistles' colophons; no editorial section headings, the KJV has none).

## 1. Sources

Every input is pinned: URL, and SHA-256 of the exact bytes
(`sources/sources.json`, `python -m abible sources pin|fetch`). A changed file
stops the build until someone looks at what changed.

| Source | What for | Licence |
|---|---|---|
| The translation's text, as the app ships it | the words | per translation |
| SIL **Glyssen** (`CharacterVerse.txt`, `CharacterDetail.txt`, English reference script) at a pinned commit | who speaks in each verse; what each character is like; FCBH's dramatized script of the WEB | MIT (SIL and Faith Comes By Hearing) |
| eBible.org USFM of the translation | titles and other non-verse text | per translation |
| Clear Bible **speaker-quotations** (optional) | word-level quotations for 8 modern translations, mapped to the Hebrew and Greek | CC BY 4.0 |

Glyssen is what Faith Comes By Hearing uses to cast its dramatized Bibles in
hundreds of languages. Its English reference script is the World English
Bible already split into blocks, one speaker each, with a delivery note where
it matters ("weeping", "calling out from the sky"). It identifies 1,281
characters and groups, distinguishes people who share a name ("Abimelech 1,
king of the Philistines (in Gerar)" versus "Abimelech, son of Gideon"), and
marks age ("David (old)", "Jesus (child)").

## 2. Script: who says each word

The script lists, for every chapter: the announcement, then every verse as
one or more lines, each with one speaker (`script/<translation>/<BOOK>.json`).

**The rule that is never broken:** the lines of a verse, joined with single
spaces, are the verse exactly, character for character. `tests/test_script.py`
checks all of them, every build.

### 2A. Translations with quotation marks

1. Split each verse at the translation's own opening and closing marks. Text
   at the first level is the narrator's; each quotation is a speech.
2. Give each speech its speaker from Glyssen's `CharacterVerse.txt` (by verse
   and order; where Glyssen lists alternatives, take its default).
3. **Nested quotations stay with whoever is quoting** (the servant retelling
   Abraham's words in Genesis 24 speaks them all). This is FCBH's convention
   and ours.
4. Cross-check with the reference script (2B's projection): where the two
   disagree, the verse goes to review.

### 2B. Translations without quotation marks (the KJV)

`abible/project.py` projects the reference script onto the translation, verse
by verse:

1. Align the WEB piece words with the verse's words (word-level diff, with
   era spellings mapped: you/ye/thee, has/hath, Yahweh/LORD).
2. Map each boundary between pieces into the verse, then snap it to a gap
   after punctuation.
3. **(KJV lesson)** The KJV marks direct speech with a capital after a verb of
   speaking: "And God said, Let there be light". A boundary into a speech
   must land before a capital, after a verb of speaking within a few words.
4. **(KJV lesson)** Where the KJV has no such place, the KJV has *indirect*
   speech ("he charged them that they should tell no man"; Glyssen's script
   turns it into "Tell no one"). Those words stay with the narrator: we never
   put words in a character's mouth that the translation gives to the
   narrator. Out of a speech, the reverse: the speech runs on, so the words
   stay with the speaker.
5. **(KJV lesson)** "Saith the LORD" interrupts a speech that then carries on
   in lower case ("In that day, saith the LORD, that there shall be…"): when
   the same speaker resumes after a narrator interjection, no capital is
   required.
6. **Red letter** (KJV, and any translation that marks Christ's words): the
   translation's own markup is a second, independent source for one speaker.
   It outranks the projection, character by character, except that red text
   inside another speaker's words stays with that speaker (Paul quoting
   Christ in Acts 22). Verses the red letter changes go to review, because the
   markup itself can be wrong **(KJV lesson: eBible marks "And the common
   people heard him gladly", Mark 12:37, as Christ's words)**.

Measured on the KJV (31,218 verses with the Psalm titles; 38,780 lines):
the projection placed every speaker change it could find a place for, and the
KJV's own red letter agreed with it in about 98% of the 2,028 verses it marks.
293 verses were left for review. Every P1 and P2 verse is settled: 50
corrected (`script/kjv-overrides.json`, two of them from a P3 sample), 37
confirmed right as made (`script/kjv-reviewed.json`); 206 P3 verses remain.

### 2C. Review

`script/<translation>/review.json` lists every verse a rule could not settle,
with the projection, the reference pieces and the reason. Work it in order:

| Priority | Kind | Who settles it |
|---|---|---|
| P1 | projection failed; Glyssen itself undecided ("Needs Review") | a person |
| P2 | ambiguous boundary; red letter changed the projection | a person, or an LLM with a person checking every answer |
| P3 | merged as indirect speech or run-on; nested quotes of Christ; no red letter | a person, every verse: most are right by rule, but a random 15 of the KJV's had 2 wrong |

A correction goes in `script/<translation>-overrides.json` with the verse's
lines and a one-line reason; never in the generated files. The overrides are
checked by the same rule (lines rejoin to the verse). A verse checked and
found right goes in `script/<translation>-reviewed.json`, which takes it off
the list.

### 2D. Non-verse text

Spoken by the narrator, never mixed into verse lines: the chapter
announcement ("The First Book of Moses, called Genesis. Chapter one." then
"Genesis, chapter two."; "Psalm twenty-three."), Psalm 119's letters, the
colophons. Titles are taken from the edition's USFM, not typed.

## 3. Cast: a voice for every speaker

### 3A. Who needs a voice

`python -m abible cast` lists every casting key in the script with how much
they say, in which books, their Glyssen attributes (gender, age, group size)
and lines they really say (`cast/cast.json`). The rules in
`cast/policy.json`:

- **One voice per character for the whole Bible**, and for every translation:
  Moses sounds the same in Exodus and Deuteronomy, in the KJV and the WEB.
  A new translation reuses the cast and designs only characters it adds.
- **Age:** "(old)" shares the character's voice and plays it older;
  "(child)" and "(young)" get voices of their own age.
- **Joint speakers** ("Deborah/Barak"): led by FCBH's choice or the first.
- **Groups** (Israelites, the crowd before Pilate): one voice speaks for the
  group for now; ensemble mixing (3E) comes later.
- **Scripture quotations** in the narration ("that it might be fulfilled
  which was spoken by the prophet, saying, …") are voiced by the prophet
  Glyssen names.
- **Author voices** (an owner decision, see decisions.md): the letters read
  by their writers (Paul, Peter, James, John, Jude), Revelation by John,
  Lamentations by Jeremiah, Ecclesiastes by the Preacher (The Word of
  Promise cast the writers of the books). Titles and colophons stay with the
  narrator.

KJV: 999 keys, 909 voices to make (the rest share).

### 3B. Briefs

A brief is the description Voice Design turns into a voice: age, pitch,
texture, pace, temperament, character. Two kinds:

- **By hand** (`cast/principals.json`) for everyone who speaks a lot or
  matters: the KJV's 125 hand briefs cover the narrator, God, Jesus and the
  speakers of about 93% of all words. Each has the brief, the castings that
  inspired it, and the scripture behind it ("I am slow of speech", Exodus
  4:10, for Moses; "the voice is Jacob's voice, but the hands are the hands
  of Esau", Genesis 27:22, so the twins must not sound alike).
- **From attributes** for the rest: gender, age, group, role words in the
  name ("king", "prophet", "soldier"), and pitch/texture/pace/temperament
  spread so that neighbours differ.

**Never in a brief:** a real person's name, "sounds like…", an imitation of
an actor, a regional or ethnic caricature. Castings inspire; they are not
copied. The accent policy is one line, appended to every brief.

### 3C. Designing and choosing

1. `voices design KEY` asks Voice Design for 4 candidates; each reads a line
   the character really says (150 characters at most), so the audition is in
   character. $0.01 a request.
2. **Principals** (hand briefs): the owner listens on the casting page
   (`voices audition`) and picks: `voices choose KEY FILE`.
3. **Everyone else**: chosen automatically, the candidate that said its line
   correctly (transcribed) and is least like every voice already cast.
4. **Screening, before anyone listens** (`screen.py`): the candidate must read
   its line (transcribed), sound the right age by Fish's description, and
   have the right pitch for its gender (men about 85–155 Hz, women 165–255).
   **(KJV lesson)** Fish's description follows the prompt, not the sound: a
   "male" Jonathan candidate measured 219 Hz. 6 of 496 principal candidates
   failed on pitch, 9 on reading their line.
5. **Uniqueness, measured not assumed:** a speaker embedding of every chosen
   voice; no two voices in one book above 0.60 similarity, none anywhere above
   0.72. Measured on the 496 principal candidates: one voice rendered twice,
   0.78–0.85; four candidates of one brief, median 0.51; candidates of
   different characters, median 0.26, 99th percentile 0.65. A candidate over
   the line is rejected and designed again.
6. The chosen clip (about 10 s, with its transcript and Fish's design
   signature) becomes a private Fish voice. Keep the clip: it can rebuild the
   voice anywhere.

### 3D. What the owner hears before anything is rendered at scale

The casting page for the principals, and a "cast reel": every voice reading
its line, in order of appearance, book by book.

### 3E. Later: ensembles

Crowds and groups as several voices at once (3–4 voices from a pool of extras,
mixed with small offsets) for short shouts ("Crucify him", "Hosanna"); long
group speeches keep one spokesperson.

## 4. Lexicon: names and words the voice gets wrong

`lexicon/<translation>.tsv`: a whole-word pattern (with context where a word
has two readings), its CMU Arpabet, and the source. The renderer turns each
match into a Fish phoneme tag.

1. **Every proper name** in the translation, listed from the text. For each:
   CMU dictionary if it has it, else the International Standard Bible
   Encyclopedia's (1915, public domain) respelling ("Ab-ed'-ne-go") turned
   into Arpabet. Names the voice already reads well need no entry.
2. **Heteronyms** with the reading decided by context: lead (the metal),
   wind, tear, bow, read, live, close, wound, minute, Job/job (KJV: the
   metal "lead" in Exodus 15:10, Numbers 31:22, Job 19:24, Jeremiah 6:29,
   Ezekiel 22 and 27, Zechariah 5).
3. **Words outside modern English** that the voice may modernize or misread:
   -est/-eth/-edst forms, shew, wist, wot, holpen, ensample.
4. **The name reel:** every name once, in a short sentence, for the owner to
   hear. Fix, re-render the reel, until it's right.

**(KJV lesson)** Unaided, the voice read "lead" (metal) as "leed" and mangled
Zaphnathpaaneah, Chedorlaomer and MENE, MENE, TEKEL, UPHARSIN; it read
capitals ("I AM THAT I AM", "JEHOVAH", "HOLINESS TO THE LORD") correctly as
words.

## 5. Render

`python -m abible render BOOK CHAPTER` (or a whole book): one chapter at a time.

- **One request per chapter** when the chapter's voices fit (about 20 voices
  made from 10 s clips) and it is under ~14,000 characters; else scenes cut
  only at narrator lines. Within a request Fish conditions every line on what
  came before: no cold starts, no joins.
- Settings: docs/fish-audio.md. Model `s2.1-pro-free` until 2026-11-30.
- Delivery cues only at the start of a line, only from the list in
  `render.DELIVERY`; Glyssen's staging notes ("to crowd") are not cues.
- Everything Fish returns is cached by a hash of the request: change one
  line and only its scene is voiced again.
- Up to 5 chapters at a time (the free account's limit). The KJV is about 6–7
  hours of rendering, ~70 hours of audio at speed 1.0.

**(KJV lesson)** A stream can end early with no error. Every render must
account for 97% of the scene's words or it is asked for again; a scene that
keeps coming back short, or is refused for its reference audio, is halved at
a narrator line.

## 6. Check

`qa.check_chapter` writes `<BOOK>.<CH>.qa.json` beside every chapter.

1. **Words.** The whole chapter is transcribed (Whisper medium.en, on the
   GPU, free) and aligned with the script. Spelling-only differences
   ("Beth-lehem"/"Bethlehem", "to day"/"today", "2"/"two") are ignored; names
   spelled another way go to a list for the name reel. Every other
   difference is heard twice more: a 6-second clip by a second model
   (large-v3), then again primed with the KJV text. **(KJV lesson)** The
   recognizers expect modern English: unprimed, they heard "and art come" as
   "hath come" and "is come into" as "coming to"; primed, both heard the KJV.
   Only differences that survive all three are "listen" items, and a person
   listens to every one. (Pilot, Genesis 3 and Ruth, 3,296 words: 41
   first-pass differences, 2 left to hear, both names in Ruth 4's genealogy.)
2. **Voices.** Every line of 1.2 s or more is compared (speaker embedding)
   with each voice's average in the chapter; a line nearer another voice is
   flagged. (Pilot: 0 of 155 lines flagged.)
3. **Sound.** Stretches far louder than the chapter's speech (noise,
   blasts), and over 25 s without a pause (babble): the faults the owner's
   lecture renders produced. Flagged chapters are rendered again.
4. **A person listens** to: every "listen" item; the casting page and the
   name reel; the first chapter of every book; a random 2% of the rest; any
   chapter a check flagged. Notes go in `docs/listening.md`; a fix is a script,
   lexicon or cast change, never an audio edit.

### Volume, standardized in three places

Voice Design returns clips anywhere from −38 to −9 LUFS (measured on the 496
principal candidates), which makes quiet candidates sound worse than they are.

1. **Auditions:** the casting room plays every candidate levelled to −18 LUFS
   (spread after levelling: about 3 dB).
2. **The voice's sample:** levelled to −18 LUFS before it becomes a Fish
   voice. Fish evens out its speech largely by itself (a voice made from a
   −38 LUFS clip spoke at −18.7 LUFS; from the same clip levelled, −15.5),
   but not entirely.
3. **Each chapter:** every voice gets one gain for the chapter, bringing its
   median speech level to the chapter's (`render.level_voices`, at most
   12 dB, eased over 30 ms), so a character still whispers and shouts within
   their own level; then the chapter is normalized to −18 LUFS. Pilot: the
   spread between characters fell from 8.0 to 1.0 dB (Genesis 3) and 4.7 to
   0.8 dB (Ruth 4).

## 7. Package

Per chapter: `<BOOK>.<CH>.mp3` (loudness −18 LUFS, peaks −1.5 dB) and
`<BOOK>.<CH>.json` in the timing format the apps already read
(`{"verses": [["1", 4.29], …], "duration": …}`), plus every line's speaker,
voice, start and end, so the app can show who is speaking.

Credits, shown in the app and in the files' tags:

- the translation and its licence;
- "Voices generated with Fish Audio" (AI disclosure; Fish asks for it and
  listeners deserve it);
- "Speaker assignments adapted from Glyssen (SIL and Faith Comes By Hearing,
  MIT)" and, if used, "MACULA Quotation and Speaker Data, © 2023 Clear Bible,
  Inc, CC BY 4.0".

## 8. Applying this to another translation

| Step | Quotation marks (A) | No quotation marks (B) |
|---|---|---|
| 1 | pin the text and its USFM | same |
| 2 | split at its marks, speakers from Glyssen, cross-check | project the reference script; red letter if marked; capital rule only where the language uses it |
| 2C | review disagreements | review the queue |
| 3 | reuse the cast; design only new keys | same |
| 4 | reuse the lexicon; add the translation's spellings ("Elijah" vs "Elias", "Yahweh") | same |
| 5–7 | same | same |

The WEB needs no projection at all: Glyssen's reference script *is* the WEB's
dramatized script, so its step 2 is a direct conversion and a check that the
text matches the app's WEB word for word.

## 9. Timeline and cost (KJV)

| Item | Cost | Time |
|---|---|---|
| Script and review | — | done except review (293 verses) |
| ~909 voices by Voice Design | $10–$30 of API credit | a day, plus the owner's casting session |
| Lexicon and name reel | — | days, mostly listening |
| Rendering | $0 until 2026-11-30 (~$65 after) | 6–7 hours |
| Checking | $0 (local GPU) | a night |
| Listening | — | the owner's time: the listen list, first chapters, 2% sample |
