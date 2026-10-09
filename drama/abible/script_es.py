"""The Reina-Valera 1909's dramatized script: every verse as lines, each with one speaker.

drama/script/rv1909/<BOOK>.json holds, per chapter, the narrator's announcement, then
each verse's lines in order, in the same form as the KJV's script. Joining a verse's
lines with single spaces gives back the verse exactly, as the app has it (checked here
and by tests/test_script_es.py). How a verse is split: project_es. Corrections a person
makes go in drama/script/rv1909-overrides.json, never in the generated files; verses
checked and found right go in rv1909-reviewed.json; review.json lists the rest.

The RV1909 prints no red letter. The KJV's (data/words_of_jesus.json), through the
verse alignment, is a check: a verse where one has words of Christ and the other
doesn't is listed for review."""
import json
from collections import Counter
from functools import lru_cache

from . import bible, books, es, glyssen, paths, project_es
from .glyssen import NARRATOR, Piece

TRANSLATION = "rv1909"
BRIDGE = "blm"
OUT = paths.SCRIPT / TRANSLATION
OVERRIDES = paths.SCRIPT / f"{TRANSLATION}-overrides.json"
REVIEWED = paths.SCRIPT / f"{TRANSLATION}-reviewed.json"


def _json(path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))["verses"] if path.exists() else {}


@lru_cache(maxsize=None)
def _red_letter() -> set:
    """KJV verses with words of Christ: {(code, chapter, verse)}."""
    data = json.loads((paths.REPO / "data" / "words_of_jesus.json").read_text(encoding="utf-8"))["verses"]
    by_name = {v: k for k, v in books.RED_LETTER_NAME.items()}
    out = set()
    for key in data:
        name, ref = key.rsplit(" ", 1)
        c, v = ref.split(":")
        out.add((by_name[name], int(c), v))
    return out


def _scripture_as(code, c, v):
    for e in glyssen.verse_entries().get((code, c, v), []):
        if e["who"] == "scripture" and e.get("default"):
            return e["default"]
    return None


def priority(notes) -> str:
    """As the KJV's review list: P1 a person settles (no split was found, or Glyssen
    itself is undecided); P2 a person or an assistant with a person checking (an
    ambiguous boundary, the red letter disagreeing); P3 a person checks, most are right
    by rule (indirect speech kept with the narrator, a speech running on)."""
    text = " ".join(notes)
    if "left with the narrator" in text or "Needs Review" in text:
        return "P1"
    if "ambiguous" in text or "Christ" in text:
        return "P2"
    return "P3"


def split_verse(code: str, c: int, n: str, verse: str):
    """([(Piece, text)], notes, the BLM's pieces) for one RV1909 verse."""
    narrator = Piece(NARRATOR, None, None, "")
    # In the same book: the BLM also has Greek Daniel and Esther, which answer for the
    # KJV's Daniel and Esther too, and Glyssen has only the 66 books
    refs = [r for r in bible.counterparts(TRANSLATION, (code, c, n), BRIDGE) if r[0] == code]
    if not refs:
        return [(narrator, verse)], [], []
    blm = bible.verses(BRIDGE)
    web, segs, notes = [], [], []
    for r in refs:
        theirs = glyssen.reference(r[0]).get((r[1], int(r[2].split("-")[0])), [])
        web += theirs
        words = blm[r[0]][(r[1], r[2])]
        opens_in_speech = bool(theirs) and theirs[0].who != NARRATOR
        if not opens_in_speech and not any(m in words for m in project_es.OPEN + project_es.CLOSE):
            # No quotation marks (much of the BLM's 2 Samuel): the Reina-Valera's own marks
            s, more = project_es.colon_segments(words), []
        else:
            s, _, more = project_es.segments(words, project_es.start_depth(words, opens_in_speech))
        segs += s
        notes += more
    merged = []
    for q, t in segs:
        if merged and merged[-1][0] == q:
            merged[-1] = (q, merged[-1][1] + " " + t)
        else:
            merged.append((q, t))
    pieces, more = project_es.label(merged, web)
    # Indirect speech in both Spanish texts ("mandó á la multitud que se recostase"),
    # where the WEB's script quotes: the narrator's, as the KJV keeps its own
    if pieces is None and not any(q for q, _ in merged) and not project_es.speech_starts(verse):
        return [(narrator, verse)], ["merged: indirect speech in the Biblia libre and the RV1909 (the WEB's script has a speaker)"], web
    notes += more
    if pieces is None:
        return [(narrator, verse)], notes + ["left with the narrator until reviewed"], web
    split, more = project_es.project_verse(verse, pieces)
    notes += more
    if split is None:
        return [(narrator, verse)], notes + ["left with the narrator until reviewed"], pieces
    return split, notes, pieces


def build_book(code: str):
    """Returns (book dict, review entries)."""
    text = bible.verses(TRANSLATION)[code]
    fixes, reviewed = _json(OVERRIDES), _json(REVIEWED)
    red = _red_letter()
    chapters, review = {}, []
    for (c, n), verse in text.items():
        lines = chapters.setdefault(c, [{"kind": "announce", "who": NARRATOR, "text": es.announcement(code, c)}])
        v = int(n)
        key = f"{code} {c}:{n}"
        # A placeholder (RV1909's 1 Samuel 23:29: its words are in 24:1): nothing to read
        if not verse.strip():
            continue
        if key in fixes:
            for item in fixes[key]["lines"]:
                line = {"v": v, "who": item[0], "text": item[1]}
                if len(item) > 2 and item[2]:
                    line["delivery"] = item[2]
                lines.append(line)
            continue
        segs, notes, pieces = split_verse(code, c, n, verse)
        # The KJV's red letter, through the alignment, as a check
        christ = any(p.who == "Jesus" or p.who.startswith("Jesus (") for p, _ in segs)
        kjv_red = any(k in red for k in bible.to_kjv(TRANSLATION, (code, c, n)))
        if kjv_red and not christ:
            notes.append("the KJV has words of Christ here; this verse has no line for him")
        elif christ and not kjv_red and code in books.RED_LETTER_NAME:
            notes.append("a line for Christ where the KJV has no red letter")
        if any(p.who == "Needs Review" for p, _ in segs):
            notes.append("Glyssen itself leaves this speaker undecided ('Needs Review'): narrator until settled")
        for p, t in segs:
            line = {"v": v, "who": p.who, "text": t}
            script_as = p.script_as or (_scripture_as(code, c, v) if p.who == "scripture" else None)
            if script_as:
                line["as"] = script_as
            if p.delivery:
                line["delivery"] = p.delivery
            lines.append(line)
        if notes and key not in reviewed:
            review.append({"ref": key, "priority": priority(notes), "notes": notes, "verse": verse,
                           "lines": [[p.who, t] for p, t in segs], "bridge": [[p.who, p.text] for p in pieces]})
    book = {"book": code, "translation": "RV1909", "chapters": [{"chapter": c, "lines": chapters[c]} for c in sorted(chapters)]}
    return book, review


def check_book(book: dict) -> list:
    """Every verse's lines, joined with spaces, must be the verse exactly."""
    text = bible.verses(TRANSLATION)[book["book"]]
    got = {}
    for ch in book["chapters"]:
        for line in ch["lines"]:
            if "v" in line:
                got.setdefault((ch["chapter"], str(line["v"])), []).append(line["text"])
                if not line["text"] or line["text"] != line["text"].strip():
                    return [f"{book['book']} {ch['chapter']}:{line['v']}: empty or padded line"]
    errors = []
    for key, verse in text.items():
        joined = " ".join(got.get(key, []))
        if joined != verse:
            errors.append(f"{book['book']} {key[0]}:{key[1]}: lines give {joined!r}, the verse is {verse!r}")
    errors += [f"{book['book']} {k[0]}:{k[1]}: not a verse" for k in got if k not in text]
    return errors


def build_all(codes=None) -> Counter:
    paths.ensure(OUT)
    stats, review = Counter(), []
    for code in codes or list(bible.verses(TRANSLATION)):
        book, rev = build_book(code)
        errors = check_book(book)
        if errors:
            raise SystemExit("\n".join(errors[:20]))
        (OUT / f"{code}.json").write_text(_dump(book), encoding="utf-8")
        review += rev
        for ch in book["chapters"]:
            for line in ch["lines"]:
                stats["lines"] += 1
                stats["narrator lines" if line["who"] == NARRATOR else "character lines"] += 1
        stats["verses needing review"] += len(rev)
    if not codes:
        (OUT / "review.json").write_text(json.dumps(review, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    return stats


def _dump(book: dict) -> str:
    """One line per script line, so diffs stay readable."""
    out = ['{"book": %s, "translation": "RV1909", "chapters": [' % json.dumps(book["book"])]
    for i, ch in enumerate(book["chapters"]):
        out.append(' {"chapter": %d, "lines": [' % ch["chapter"])
        out.append(",\n".join("  " + json.dumps(line, ensure_ascii=False) for line in ch["lines"]))
        out.append(" ]}" + ("," if i < len(book["chapters"]) - 1 else ""))
    out.append("]}")
    return "\n".join(out) + "\n"


def load(code: str) -> dict:
    return json.loads((OUT / f"{code}.json").read_text(encoding="utf-8"))
