"""Checks a rendered chapter without ears (run with the QA environment, see README):

1. words: Whisper (medium.en) transcribes the chapter; the transcript is
   aligned with the script. Every difference that is not a known spelling
   variant is transcribed again on its own, from a 6-second clip, by a second
   model (large-v3). Only a difference both models hear is reported: in the
   tests, the first pass alone produced false alarms ("in the the day").
2. voices: a speaker embedding (WeSpeaker ResNet34) of every line of 1.2 s or
   more is compared with each voice's average in the chapter; a line closer to
   another voice than to its own is reported.
3. sound: stretches far louder than the chapter's speech (noise, blasts) and
   over 25 s without a pause (babble), the faults found in the owner's lecture
   renders.

Writes <chapter>.qa.json next to the MP3 and returns its summary.
"""
import difflib
import glob
import json
import os
import re
import subprocess
import sys
from pathlib import Path

import numpy as np

from . import paths, render

_base = Path(sys.executable).parent.parent / "Lib" / "site-packages" / "nvidia"
for _d in glob.glob(str(_base / "*" / "bin")):
    os.add_dll_directory(_d)
    os.environ["PATH"] = _d + os.pathsep + os.environ["PATH"]

# Spellings Whisper writes differently for the same sounds: never errors.
SAME = {
    "subtil": "subtle", "shew": "show", "shewed": "showed", "sheweth": "showeth", "shewing": "showing",
    "aught": "ought", "naught": "nought", "ancle": "ankle", "throughly": "thoroughly", "enquire": "inquire",
    "enquired": "inquired", "vail": "veil", "sware": "swear", "bare": "bear", "worshipped": "worshiped",
    "rebekah": "rebecca", "milcah": "milka", "intreat": "entreat", "intreated": "entreated", "gaol": "jail",
    "musick": "music", "alway": "always", "stedfast": "steadfast", "stedfastly": "steadfastly", "honour": "honor", "labour": "labor", "saviour": "savior", "neighbour": "neighbor",
    "favour": "favor", "colour": "color", "behaviour": "behavior", "travail": "travel", "thresh": "thrash",
}  # compounds ("threshingfloor", "to day") are matched by joining words, not listed here


def norm(w: str) -> str:
    import unicodedata
    w = w.lower().replace("’", "'").replace("æ", "ae")
    # Accents set aside (the RV1909's "á" and "fué" are the recognizer's "a" and "fue")
    w = "".join(c for c in unicodedata.normalize("NFD", w) if unicodedata.category(c) != "Mn")
    w = re.sub(r"[^a-z0-9']", "", w).strip("'")
    if w.endswith("'s"):
        w = w[:-2] + "s"
    return SAME.get(w, w)


_models = {}


def model(name):
    if name not in _models:
        from faster_whisper import WhisperModel
        _models[name] = WhisperModel(name, device="cuda", compute_type="int8_float16")
    return _models[name]


def pcm16k(path, start=None, dur=None):
    cmd = ["ffmpeg", "-v", "error"]
    if start is not None:
        cmd += ["-ss", f"{max(0.0, start):.3f}"]
    if dur is not None:
        cmd += ["-t", f"{dur:.3f}"]
    cmd += ["-i", str(path), "-f", "f32le", "-ac", "1", "-ar", "16000", "-"]
    return np.frombuffer(subprocess.run(cmd, capture_output=True, check=True).stdout, dtype=np.float32)


# The first pass's model: English-only for an English text, else the multilingual one
FIRST = "medium.en" if paths.LANGUAGE == "en" else "medium"


def transcribe(path, name=None, start=None, dur=None):
    segs, _ = model(name or FIRST).transcribe(pcm16k(path, start, dur), word_timestamps=True, beam_size=5,
                                              condition_on_previous_text=False, language=paths.LANGUAGE)
    off = start or 0.0
    return [{"text": w.word.strip(), "start": round(float(w.start) + off, 2), "end": round(float(w.end) + off, 2)}
            for s in segs for w in s.words]


def _tokens(text):
    """Comparable words: digits spelled out as the narrator says them ("23" ->
    "twenty three"), hyphens as spaces."""
    out = []
    for w in text.replace("-", " ").split():
        t = norm(w)
        if t.isdigit() and len(t) < 7:
            from .books import number_words
            from .es import number_words as numero
            said = number_words(int(t)) if paths.LANGUAGE == "en" else numero(int(t))
            out += [norm(x) for x in said.replace("-", " ").split()]
        elif t:
            out.append(t)
    return out


def _joined(text):
    """'Beth lehem' == 'Bethlehem', 'to night' == 'tonight', '2' == 'two'."""
    return "".join(_tokens(text))


def _skeleton(word):
    """A rough sound key for names the recognizer spells its own way
    (Pharez/Phares, Mahlon/Malon): consonants only, similar ones merged."""
    w = norm(word)
    for a, b in (("ph", "f"), ("ch", "k"), ("c", "k"), ("q", "k"), ("z", "s"), ("v", "f"), ("y", "i"), ("j", "i")):
        w = w.replace(a, b)
    w = w[:1] + re.sub(r"[aeiouhw']", "", w[1:])
    return re.sub(r"(.)\1+", r"\1", w)


def _same_name(said, got):
    s, g = said.split(), got.split()
    return len(s) == len(g) and all(x[:1].isupper() and _skeleton(x) == _skeleton(y) for x, y in zip(s, g))


def _inside(clip: str, context: str) -> bool:
    """A clip's transcript, less its first and last word (cut by the clip's
    edges), found word for word in the script around it: nothing added,
    dropped or changed there."""
    words = _tokens(clip)[1:-1]
    return len(words) >= 4 and "".join(words) in _joined(context)


def _contains(expected: str, heard: str) -> bool:
    e, h = _joined(expected), _joined(heard)
    return bool(e) and e in h


def word_check(mp3, lines):
    """Differences between script and audio, in three tiers (docs/SOP.md, step 6):
    the recognizers favour modern English, so KJV grammar ("art come", "is come
    into") is often misheard. A difference is reported for a person's ear only
    when it survives a second model and a third pass primed with the KJV text."""
    script_words = [w for l in lines for w in l["text"].replace("-", " ").split()]
    heard = transcribe(mp3)
    a, b = [norm(w) for w in script_words], [norm(h["text"]) for h in heard]
    sm = difflib.SequenceMatcher(None, a, b, autojunk=False)
    suspects, names = [], []
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op == "equal":
            continue
        said = " ".join(script_words[i1:i2])
        got = " ".join(h["text"] for h in heard[j1:j2])
        if _joined(said) == _joined(got):
            continue
        at = heard[j1]["start"] if j1 < len(heard) else heard[-1]["end"]
        item = {"at": at, "op": op, "script": said, "heard": got,
                # Whisper's prompt is the text *before* the audio; text after it made it loop
                "context": " ".join(script_words[max(0, i1 - 30):i1]),
                "around": " ".join(script_words[max(0, i1 - 25):i2 + 25])}
        if said and _same_name(said, got):
            item.pop("context"); item.pop("around")
            names.append(item)
        else:
            suspects.append(item)
    listen, primed_ok = [], []
    for s in suspects:
        context = s.pop("context")
        second = " ".join(w["text"] for w in transcribe(mp3, "large-v3", s["at"] - 3.0, 6.0))
        around = s.pop("around")
        if _inside(second, around) or s["script"] and _contains(s["script"], second):
            continue
        segs, _ = model("large-v3").transcribe(pcm16k(mp3, s["at"] - 3.5, 7.0), beam_size=5, language=paths.LANGUAGE,
                                               initial_prompt=context or None, condition_on_previous_text=False)
        third = " ".join(x.text.strip() for x in segs)
        item = dict(s, second_opinion=second, primed=third)
        if _inside(third, around) or s["script"] and _contains(s["script"], third):
            primed_ok.append(item)  # fits the text when the recognizer expects it: spot-check by ear
        else:
            listen.append(item)  # heard otherwise three times: a person must listen
    return {"script_words": len(a), "heard_words": len(b), "ratio": round(sm.ratio(), 4),
            "first_pass_differences": len(suspects) + len(names), "names_spelled_differently": names,
            "fits_when_primed": primed_ok, "listen": listen}


def voice_check(mp3, timing):
    from . import spk
    lines = [t for t in timing["lines"] if t["end"] - t["start"] >= 1.2]
    embs = [spk.embed(pcm16k(mp3, t["start"], t["end"] - t["start"])) for t in lines]
    by = {}
    for t, e in zip(lines, embs):
        by.setdefault(t["voice"], []).append(e)
    cent = {k: (lambda m: m / np.linalg.norm(m))(np.mean(v, axis=0)) for k, v in by.items()}
    flagged = []
    for t, e in zip(lines, embs):
        sims = {k: float(np.dot(e, c)) for k, c in cent.items()}
        best = max(sims, key=sims.get)
        if best != t["voice"] or sims[t["voice"]] < 0.5:
            flagged.append({"v": t.get("v"), "who": t["key"], "start": t["start"], "own": round(sims[t["voice"]], 2),
                            "closest": best, "closest_sim": round(sims[best], 2)})
    return {"lines_checked": len(lines), "voices": len(cent), "flagged": flagged}


def sound_check(mp3):
    x = pcm16k(mp3)
    win = 1600
    rms = np.sqrt(np.mean(x[: len(x) // win * win].reshape(-1, win) ** 2, axis=1))
    speech = np.sort(rms[rms > 0.01])
    if not len(speech):
        return {"silent": True}
    typ = float(speech[len(speech) // 2])
    runs, start = [], None
    for i, h in enumerate(list(rms > 3.5 * typ) + [False]):
        if h and start is None:
            start = i
        elif not h and start is not None:
            if i - start >= 3:
                runs.append([start / 10, i / 10])
            start = None
    w2 = 800
    r2 = np.sqrt(np.mean(x[: len(x) // w2 * w2].reshape(-1, w2) ** 2, axis=1))
    quiet, since, babble = 0, 0, []
    for i, r in enumerate(r2):
        quiet = quiet + 1 if r < 0.12 * typ else 0
        if quiet >= 6:
            if i - since > 25 * 20:
                babble.append([since / 20, i / 20])
            since = i
    return {"typical_rms": round(typ, 4), "loud": runs, "babble": babble}


def check_chapter(out_dir: Path, code: str, chapter: int) -> dict:
    mp3 = out_dir / f"{code}.{chapter}.mp3"
    timing = json.loads((out_dir / f"{code}.{chapter}.json").read_text(encoding="utf-8"))
    lines = render.spoken_lines(code, chapter)
    report = {"chapter": f"{code} {chapter}", "words": word_check(mp3, lines),
              "voices": voice_check(mp3, timing), "sound": sound_check(mp3)}
    (out_dir / f"{code}.{chapter}.qa.json").write_text(json.dumps(report, indent=1, ensure_ascii=False), encoding="utf-8")
    return report
