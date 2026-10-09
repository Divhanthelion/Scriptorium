# Decisions: the dramatized Reina-Valera 1909

The KJV's decisions (KJV Interlinear, `audio/docs/decisions.md`) hold unless this
file says otherwise: one voice per character, Glyssen's speakers, nested quotations
with whoever quotes them, the narrator's announcements, the letters read by their
writers, one Fish request per chapter, three transcription passes and a person's ear.

## Made by the owner (2026-10-09)

| # | Decision | Why |
|---|---|---|
| E1 | Dramatize the **Reina-Valera 1909** | the highest-quality Spanish Bible the app can offer: the classic text Spanish-speaking churches grew up on (the 1960 revision most use today is close to it, but still copyrighted); public domain; the counterpart of the dramatized KJV |
| E2 | **Neutral Latin American Spanish** for every voice | what most Spanish speakers hear in church and media; the Reina-Valera is read this way across Latin America (`cast/policy-rv1909.json`) |
| E3 | **The owner picks the principal voices** in the casting room, as for the KJV; the rest are chosen by the screening and uniqueness checks | the voices people hear most are the owner's call |
| E4 | Built from a copy of the KJV pipeline in Scriptorium (`drama/`), not in KJV Interlinear | the KJV's own work is left alone while it renders |

## Made while building (reversible)

| # | Decision | Why |
|---|---|---|
| E5 | A **new Spanish cast**: the same characters and hand-written briefs as the KJV, with Spanish auditions and the Spanish accent line | an English voice reading Spanish carries its accent; the briefs describe only the voice, so they serve both |
| E6 | Speakers found through the **Santa Biblia libre para el mundo** (BLM), the RV1909's own wording given the WEB's quotation marks; Glyssen's WEB pieces name the speakers; the RV1909's colon and capital place the boundaries (`abible/project_es.py`) | Glyssen's reference scripts are English and Russian only |
| E7 | The RV1909's verses are matched with the BLM's **through the app's verse alignment** (its 1 Samuel 24:1 is the KJV's 23:29) | the RV1909 divides some books as the Hebrew, keeping the KJV's numbers with empty verses |
| E8 | **Empty verses are not read** | they are placeholders: their words are in the next verse |
| E9 | The KJV's **red letter is a check**, through the alignment | the RV1909 prints none |
| E10 | Chapter announcements from the edition's short book names, numbers in words ("Primero de Samuel, capítulo veintiuno."); no long titles | eBible's RV1909 has only short titles, and titles are taken from the edition, not typed |
| E11 | **Names with a circumflex** (Achâb, Ezechîas, Mardochêo: the RV1909's mark that "ch" is a k sound) are respelled for the voice only (Acáb, Ezequías, Mardoquéo); the script and the app keep the RV1909's spelling (`lexicon/rv1909.tsv`, checked on the name reel) | a Spanish voice reads "ch" as in "mucho" |

## Open: for the owner

1. **The pilot.** Genesis 3 with five designed voices, before the cast is designed
   at scale: does the Spanish sound right?
2. **API credit** for about 900 voices (~$10–$30), after the pilot.
3. **The casting session**: the 125 principal voices in Spanish.
4. **The review list**: `script/rv1909/review.json`, 422 verses (78 P1 for a person,
   102 P2, 242 P3), as the KJV's.
