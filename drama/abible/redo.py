"""Render again any scene the checks found stray words in (decisions-rv1909, E12).

In the Spanish pilot, 4 of 9 renders of Genesis 3 had a few nonsense words just as a
new speaker began ("Manirana Sediru"), never at a predictable place; each was gone when
that scene was rendered again. So after a chapter is checked (qa.check_chapter), a
difference a person would have to hear ("listen": it survived three transcriptions)
whose last transcription has two or more words the chapter's script nowhere has is
taken for stray words. The scene it falls in is set aside (its cached render renamed
*.incomplete.mp3, so render_scene asks Fish again; after two tries a scene is halved
at a narrator line), the chapter is rendered again and checked again, up to ROUNDS
times; what is still flagged stays on the list for a person.
"""
import json
import re

from . import qa, render

ROUNDS = 3


def _words(text: str) -> list:
    return [qa.norm(w) for w in re.split(r"[\s\-—]+", text) if qa.norm(w)]


def stray(report: dict, lines: list) -> list:
    """The times of differences that are words the script doesn't have."""
    vocab = {w for line in lines for w in _words(line["text"])}
    out = []
    for item in report.get("words", {}).get("listen", []):
        heard = item.get("primed") or item.get("second_opinion") or item.get("heard", "")
        new = [w for w in _words(heard) if w not in vocab and not w.isdigit()]
        if len(new) >= 2:
            out.append(item["at"])
    return out


def scenes_at(timing: dict, times: list) -> set:
    """The cached renders (scene keys) the given times fall in."""
    keys = set()
    for t in times:
        line = min(timing["lines"], key=lambda l: 0 if l["start"] <= t <= l["end"] else min(abs(l["start"] - t), abs(l["end"] - t)))
        keys.add(timing["renders"][line["scene"]])
    return keys


def check_and_redo(out_dir, code: str, chapter: int) -> dict:
    """Check a chapter; while it has stray words, render their scenes again and check
    again. Returns the last report, with "redone": the rounds it took."""
    lines = render.spoken_lines(code, chapter)
    report = qa.check_chapter(out_dir, code, chapter)
    rounds = 0
    while rounds < ROUNDS:
        times = stray(report, lines)
        if not times:
            break
        timing = json.loads((out_dir / f"{code}.{chapter}.json").read_text(encoding="utf-8"))
        for key in scenes_at(timing, times):
            mp3 = render.paths.RENDERS / f"{key}.mp3"
            if mp3.exists():
                mp3.replace(mp3.with_name(f"{key}.incomplete.mp3"))
        try:
            render.render_chapter(code, chapter)
        except (SystemExit, Exception) as e:  # noqa: BLE001 - a scene that can't be split again: for a person
            report["redo_failed"] = str(e)
            break
        report = qa.check_chapter(out_dir, code, chapter)
        rounds += 1
    report["redone"] = rounds
    (out_dir / f"{code}.{chapter}.qa.json").write_text(json.dumps(report, indent=1, ensure_ascii=False), encoding="utf-8")
    return report
