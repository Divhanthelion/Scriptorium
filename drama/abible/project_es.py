"""Split a verse of the Reina-Valera 1909 into speakers.

The RV1909, like the KJV, prints no quotation marks. Glyssen's reference script is
the WEB in English, which can't be compared with Spanish word for word. But the
Santa Biblia libre para el mundo (BLM) is the RV1909's own wording, revised and
given the WEB's quotation marks (Genesis 3:1: "la cual dijo a la mujer: “¿Conque
Dios os ha dicho: ‘No comáis…’?”"). So, verse by verse:

1. Its counterparts in the BLM (through the app's alignment tables: RV1909's
   1 Samuel 24:1 is the BLM's 23:29) are split at their quotation marks, the
   depth carried from verse to verse, so a speech that runs on is still a speech.
2. The parts are given their speakers from Glyssen's WEB pieces for the same
   verses, in order (narrator, speech, narrator…), which the BLM's marks follow.
3. Those parts are projected onto the RV1909's words (`project_verse`), as the
   KJV's are from the WEB: Spanish to Spanish, word for word, each boundary
   snapped to a gap after punctuation. The RV1909 opens direct speech with a colon
   and a capital ("Y dijo Dios: Sea la luz"), after a verb of speaking.

Where a rule can't settle a verse it stays with the narrator and is reported.
"""
import difflib
import re
import unicodedata

from .glyssen import NARRATOR, Piece

OPEN = "“«‘‹"
CLOSE = "”»’›"

# The same word in the RV1909's spelling and the BLM's, or two habits of translation
EQUIV = {"yahve": "jehova", "empero": "pero", "mas": "pero", "crio": "creo"}

SPEECH = re.compile(
    r"\b(dij\w*|dic\w*|dec\w*|respond\w*|habl\w*|clam\w*|grit\w*|pregunt\w*|mand\w*|llam\w*|or[oa]\w*|"
    r"jur\w*|bendij\w*|bendec\w*|maldij\w*|escrit\w*|escrib\w*|envi\w*|cont[oa]\w*|cant\w*|replic\w*|"
    r"rog\w*|exclam\w*|profetiz\w*|anunci\w*|declar\w*|predic\w*|amonest\w*|requiri\w*|suplic\w*|"
    r"lament\w*|conjur\w*|voz|asi)\b[^,:;.?!]{0,45}[,:;]\s*$",
    re.I)


def plain(s: str) -> str:
    """Lower case, accents aside."""
    s = unicodedata.normalize("NFD", s.lower())
    return "".join(c for c in s if unicodedata.category(c) != "Mn")


def toks(text: str):
    """[(normalized word, start, end)] over the text."""
    out = []
    for m in re.finditer(r"\w+(?:-\w+)*", text):
        w = plain(m.group())
        out.append((EQUIV.get(w, w), m.start(), m.end()))
    return out


def gaps(text: str):
    """Places a verse can be split: (offset after the space, punctuation, a capital
    follows: "¿Qué", "¡Oh" count)."""
    out = []
    for m in re.finditer(r"([,;:.?!)])\s+(?=(\S))", text):
        nxt = text[m.end():m.end() + 2]
        cap = nxt[:1].isupper() or (nxt[:1] in "¿¡" and nxt[1:2].isupper())
        out.append((m.end(), m.group(1), cap))
    return out


def start_depth(text: str, opens_in_speech: bool) -> int:
    """How deep in quotations a verse starts. Not carried over from the verse before:
    a speech running over several paragraphs opens each with a mark and closes only
    the last, so counting marks drifts. A verse starts as deep as it has closing
    marks it never opened; or inside one speech when the script has it open with
    one and the verse doesn't begin with a mark of its own."""
    depth = low = 0
    for ch in text:
        depth += (ch in OPEN) - (ch in CLOSE)
        low = min(low, depth)
    if -low:
        return -low
    return 1 if opens_in_speech and not text.lstrip()[:1] in OPEN else 0


def segments(text: str, depth: int, marks: tuple = (OPEN, CLOSE)):
    """Split by quotation marks: ([(inside a quotation, text)], depth at the end).
    Nested quotations stay with whoever quotes them, so only the first level counts.
    What has no words of its own (the full stop after a closing mark) goes with the
    part before it."""
    opening, closing = marks
    out, buf, notes = [], "", []
    for ch in text:
        if ch in opening:
            if depth == 0 and buf.strip():
                out.append((False, buf.strip()))
                buf = ""
            depth += 1
            buf += ch
        elif ch in closing:
            buf += ch
            depth -= 1
            if depth < 0:
                notes.append("a quotation mark closes nothing")
                depth = 0
            if depth == 0:
                out.append((True, buf.strip()))
                buf = ""
        else:
            buf += ch
    if buf.strip():
        out.append((depth > 0, buf.strip()))
    merged = []
    for q, t in out:
        wordless = not any(ch.isalnum() for ch in t)
        if merged and (merged[-1][0] == q or wordless):
            merged[-1] = (merged[-1][0], merged[-1][1] + ("" if wordless and not t[:1].isspace() else " ") + t)
        else:
            merged.append((q, t))
    return merged, depth, notes


def speech_starts(text: str) -> list:
    """Where direct speech opens: a colon (or comma) and a capital after a verb of speaking."""
    return [g[0] for g in gaps(text) if g[1] in ":," and g[2] and SPEECH.search(plain(text[max(0, g[0] - 50):g[0]]))]


def colon_segments(text: str):
    """A verse with no quotation marks split as the Reina-Valera marks speech: it opens
    at a colon and a capital after a verb of speaking ("Y dijo Absalom: Llama…") and
    runs to the clause that brings in the next speech, or to the verse's end."""
    starts = [g[0] for g in gaps(text) if g[1] == ":" and g[2] and SPEECH.search(plain(text[max(0, g[0] - 50):g[0]]))]
    if not starts:
        return [(False, text.strip())]
    out, at = [], 0
    for i, s in enumerate(starts):
        # The narration before this speech starts after the last full stop (or ;, ?, !)
        # before its verb of speaking, if a speech came before it
        lead = at
        if i > 0:
            stops = [g[0] for g in gaps(text) if g[1] in ".;?!" and at < g[0] < s]
            lead = stops[-1] if stops else s
            if lead > at:
                out.append((True, text[at:lead].strip()))
        if text[lead:s].strip():
            out.append((False, text[lead:s].strip()))
        at = s
    out.append((True, text[at:].strip()))
    return [(q, t) for q, t in out if t]


def label(segs, web):
    """Speakers for the BLM's parts from the WEB's pieces of the same verses, in
    order: ([Piece with the BLM's text], notes) or (None, notes)."""
    merged = []
    for p in web:
        if merged and merged[-1].who == p.who and merged[-1].delivery == p.delivery:
            last = merged[-1]
            merged[-1] = Piece(p.who, p.delivery, last.script_as or p.script_as, last.text + " " + p.text)
        else:
            merged.append(p)
    # A quotation Glyssen leaves with the narrator ("I said, «A plumb line.»": Amos
    # narrates his own book) is a quotation in the BLM too: split the narrator's pieces
    # at their marks, so the patterns compare, and keep those parts the narrator's
    parts = []  # (inside a quotation, Piece)
    for p in merged:
        if p.who != NARRATOR:
            parts.append((True, p))
            continue
        # (the WEB's script marks quotations with guillemets; ’ is an apostrophe there)
        for q, t in segments(p.text, 0, ("«‹", "»›"))[0]:
            parts.append((q, Piece(NARRATOR, p.delivery, p.script_as, t)))
    narrators_quote = any(q and p.who == NARRATOR for q, p in parts)
    kinds_web = [q for q, _ in parts]
    kinds_blm = [q for q, _ in segs]
    text = " ".join(t for _, t in segs)
    if not any(p.who != NARRATOR for p in merged):
        return [Piece(NARRATOR, None, None, text)], []
    if narrators_quote:
        if kinds_blm != kinds_web:
            return None, [f"the Biblia libre's quotations ({''.join('Q' if k else 'n' for k in kinds_blm)}) don't match "
                          f"the WEB's ({''.join('Q' if k else 'n' for k in kinds_web)}), some of them the narrator's"]
        out = []
        for (_, p), (_, t) in zip(parts, segs):
            if out and out[-1].who == p.who == NARRATOR:
                out[-1] = Piece(NARRATOR, out[-1].delivery, out[-1].script_as, out[-1].text + " " + t)
            else:
                out.append(Piece(p.who, p.delivery, p.script_as, t))
        return out, []
    kinds_web = [p.who != NARRATOR for p in merged]
    # One speaker for the whole verse (Moses in Deuteronomy, a prophet): what the BLM
    # marks inside it are quotations within the speech, which stay with the speaker
    if len(merged) == 1:
        p = merged[0]
        return [Piece(p.who, p.delivery, p.script_as, text)], []
    if kinds_blm == kinds_web:
        return [Piece(p.who, p.delivery, p.script_as, t) for p, (_, t) in zip(merged, segs)], []
    # One character, however the BLM divides the quotation ("decían: «Tiene a
    # Beelzebul», y «Por el príncipe…»"): every quotation is theirs
    chars = {p.who for p in merged if p.who != NARRATOR}
    if len(chars) == 1 and any(kinds_blm):
        p = next(p for p in merged if p.who != NARRATOR)
        return [Piece(p.who, p.delivery, p.script_as, t) if q else Piece(NARRATOR, None, None, t) for q, t in segs], []
    if not any(kinds_blm):
        return None, ["the Biblia libre has no quotation here, the WEB's script has a speaker"]
    return None, [f"the Biblia libre's quotations ({''.join('Q' if k else 'n' for k in kinds_blm)}) don't match "
                  f"the WEB's speakers ({''.join('Q' if k else 'n' for k in kinds_web)})"]


def _is_char(p: Piece) -> bool:
    return p.who != NARRATOR


def project_verse(rv: str, pieces: list):
    """Returns ([(Piece, RV1909 text)], notes) or (None, notes) when it can't."""
    if len(pieces) == 1 or len({p.who for p in pieces}) == 1:
        return [(pieces[0], rv)], []
    blm = " ".join(p.text for p in pieces)
    bt, rt = toks(blm), toks(rv)
    sm = difflib.SequenceMatcher(None, [w for w, _, _ in bt], [w for w, _, _ in rt], autojunk=False)
    pairs = [(a + i, b + i) for a, b, n in sm.get_matching_blocks() for i in range(n)]
    bounds, pos = [], 0
    for p in pieces[:-1]:
        pos += len(toks(p.text))
        bounds.append(pos)
    cands = gaps(rv)
    notes, cuts = [], []
    for i, bw in enumerate(bounds):
        before = [rb for wb, rb in pairs if wb < bw]
        after = [rb for wb, rb in pairs if wb >= bw]
        lo = (before[-1] + 1) if before else 0
        hi = after[0] if after else len(rt)
        lo_c = rt[lo - 1][2] if lo > 0 else 0
        hi_c = rt[hi][1] if hi < len(rt) else len(rv)
        into_quote = pieces[i + 1].who != pieces[i].who and _is_char(pieces[i + 1])
        # "dice Jehová" interrupts a speech that carries on: no capital needed
        resumes = into_quote and i > 0 and pieces[i - 1].who == pieces[i + 1].who and not _is_char(pieces[i])
        floor = cuts[-1] if cuts else 0
        ok = [c for c in cands if lo_c <= c[0] <= hi_c + 1 and c[0] > floor]
        if into_quote and not resumes:
            # Direct speech opens with a capital after a verb of speaking ("dijo
            # Dios: Sea"); the BLM may word the introduction otherwise, so look a few
            # words either side. With no such place the RV1909 has indirect speech,
            # and the merge below keeps it with the narrator.
            wide_lo = rt[max(lo - 4, 0)][1] if rt else 0
            wide_hi = rt[min(hi + 3, len(rt) - 1)][2] if rt else len(rv)
            near = [c for c in cands if wide_lo <= c[0] <= wide_hi + 1 and c[0] > floor and c[2]]
            spoken = [c for c in near if SPEECH.search(plain(rv[max(0, c[0] - 50):c[0]]))]
            ok = [c for c in ok if c[2] and c in spoken] or [c for c in ok if c[2]] or spoken
        if len(ok) == 1:
            cuts.append(ok[0][0])
        elif ok:
            mid = (lo_c + hi_c) / 2
            cuts.append(min(ok, key=lambda c: abs(c[0] - mid))[0])
            notes.append(f"ambiguous: {len(ok)} places for the boundary before {pieces[i + 1].who}")
        else:
            a, b = pieces[i], pieces[i + 1]
            if into_quote and _is_char(a):
                return None, notes + [f"no place in the RV1909 for the change from {a.who} to {b.who}"]
            keep = a if into_quote or _is_char(a) else b
            merged = Piece(keep.who, keep.delivery, keep.script_as, a.text + " " + b.text)
            segs, more = project_verse(rv, pieces[:i] + [merged] + pieces[i + 2:])
            why = "indirect speech in the RV1909" if into_quote else "the RV1909's speech runs on"
            gone = b.who if into_quote else a.who if keep is b else b.who
            return segs, notes + [f"merged: {gone} kept with {keep.who} ({why})"] + more
    segs, start = [], 0
    for p, c in zip(pieces, cuts + [len(rv)]):
        segs.append((p, rv[start:c].strip()))
        start = c
    merged = []
    for p, t in segs:
        if merged and merged[-1][0].who == p.who:
            merged[-1] = (merged[-1][0], merged[-1][1] + " " + t)
        else:
            merged.append((p, t))
    if any(not t for _, t in merged):
        return None, notes + ["a speaker would get no words"]
    return merged, notes
