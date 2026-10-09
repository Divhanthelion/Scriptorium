"""Screening Voice Design candidates before anyone listens (run with the QA
environment, see README):

  - read: Whisper (large-v3) transcribes the candidate; the share of its line's
    words it said (a candidate that skipped or garbled its line is out);
  - fits: Fish's own reading of the voice (gender, age) against the
    character's (Glyssen's gender, and the age the brief asks for), and the
    measured pitch against the character's gender;
  - apart: a speaker embedding of every candidate, so each one can be compared
    with every other character's candidates and, later, chosen voices.

Writes .cache/voices/<slug>/screen.json and .cache/voices/embeddings.npz.
"""
import difflib
import json
import re

import numpy as np

from . import cast as castmod, paths, qa, spk, voices

EMB = paths.VOICES / "embeddings.npz"


def pitch(pcm, rate=16000) -> float | None:
    """Median fundamental frequency (Hz) of the voiced parts, by autocorrelation."""
    fr, out = 640, []
    for i in range(0, len(pcm) - fr, fr):
        w = pcm[i:i + fr] - pcm[i:i + fr].mean()
        if np.sqrt((w ** 2).mean()) < 0.02:
            continue
        ac = np.correlate(w, w, "full")[fr - 1:]
        lo, hi = rate // 400, rate // 60
        k = lo + int(np.argmax(ac[lo:hi]))
        if ac[k] > 0.3 * ac[0]:
            out.append(rate / k)
    return float(np.median(out)) if out else None


MUFFLED_HZ = 6000  # 99.9% of a candidate's energy below this: muffled (the pilot's Eve, 4.9 kHz)
DULL_HZ = 7000  # below this: dull (shown in the casting room, not a problem)


def bandwidth(wav) -> int:
    """The frequency below which 99.9% of a clip's energy lies. A voice sounds like
    its clip: the pilot's Eve, made from a clip that stopped at 4.9 kHz, spoke with
    almost nothing above 5 kHz, thin and far away (measured 2026-10-09)."""
    import subprocess
    x = np.frombuffer(subprocess.run(["ffmpeg", "-v", "error", "-i", str(wav), "-f", "f32le", "-ac", "1", "-ar", "44100", "-"],
                                     capture_output=True, check=True).stdout, dtype=np.float32)
    spec = np.abs(np.fft.rfft(x)) ** 2
    f = np.fft.rfftfreq(len(x), 1 / 44100)
    c = np.cumsum(spec) / max(spec.sum(), 1e-12)
    return int(f[min(np.searchsorted(c, 0.999), len(f) - 1)])


def pitch_problem(entry, hz) -> str | None:
    """Fish's description of a candidate follows the prompt, not the sound
    (a 'male' Jonathan candidate measured 219 Hz): check the pitch itself.
    Adult men speak at about 85-155 Hz, women 165-255 Hz."""
    if hz is None:
        return None
    g, age = wants_gender(entry), wants_age(entry)
    if g == "male" and age != "young" and hz > 175:
        return f"pitch {hz:.0f} Hz: too high for a man"
    if g == "female" and hz < 145:
        return f"pitch {hz:.0f} Hz: too low for a woman"
    return None


# Spanish letters as the recognizer may write a word that sounds like one: "Heme aquí"
# (the h is silent) came back "M aquí" from every candidate that said it
SPANISH_LETTERS = {"m": "eme", "n": "ene", "l": "ele", "s": "ese", "f": "efe", "r": "erre", "c": "ce", "d": "de",
                   "t": "te", "b": "be", "p": "pe", "v": "uve", "k": "ka", "j": "jota", "g": "ge", "x": "equis"}


def _words(text):
    words = [qa.norm(w) for w in re.sub(r"[-—]", " ", text).split() if qa.norm(w)]
    if paths.LANGUAGE == "es":  # sounds, not spellings: a silent h, a letter's name, a number
        from .es import number_words
        said = []
        for w in words:  # "2300" -> "dos mil trescientos", as the RV1909 writes it
            said += [qa.norm(x) for x in number_words(int(w)).split()] if w.isdigit() and len(w) < 7 else [w]
        words = [{"etc": "etcetera"}.get(SPANISH_LETTERS.get(w, w), SPANISH_LETTERS.get(w, w)) for w in said]
        words = [w[1:] if w.startswith("h") and len(w) > 1 else w for w in words]
    return words


def _brief_voice(entry) -> str:
    """The brief's description of the voice, without a group's name (whose words,
    "slave girl", "ten of eighty", say nothing about the voice)."""
    return re.sub(r"^One voice speaking for a group \(.*?\): ", "", entry.get("brief", "")).lower()


def wants_gender(entry) -> str | None:
    g = (entry.get("gender") or "").lower()
    if g in ("female", "preferfemale"):
        return "female"
    if g in ("male", "prefermale"):
        return "male"
    b = _brief_voice(entry)
    if re.search(r"\b(woman|girl|she|her|mezzo|soprano|alto|contralto)\b", b):
        return "female"
    if re.search(r"\b(man|boy|he|his|baritone|bass|tenor)\b", b):
        return "male"
    return None


def wants_age(entry) -> str | None:
    b = _brief_voice(entry)
    m = re.search(r"of about (\d+)", b)
    if m:  # generated briefs state the age
        n = int(m.group(1))
        return "young" if n <= 17 else "old" if n >= 65 else None
    if re.search(r"\b(a child|a boy|a girl|young girl|young boy)\b", b):
        return "young"
    if re.search(r"\b(elderly|very old|an old man|an old woman|ninety|eighty|seventy|six hundred)\b", b):
        return "old"
    return None


def _proper(word):
    return word[:1].isupper()


def read_share(reference_text: str, heard: str) -> float:
    """Share of the line's words the candidate said, names left out (the
    recognizer spells names its own way; the name reel checks them)."""
    ref = [w for w in re.sub(r"[-—]", " ", reference_text).split() if not _proper(w.strip("‘’'\"(),.;:?!"))
           or reference_text.split()[0] == w]
    # letters, not words: the KJV writes "to day", "every where", "threshingfloor"
    a, b = "".join(_words(" ".join(ref))), "".join(_words(heard))
    return sum(n for _, _, n in difflib.SequenceMatcher(None, a, b, autojunk=False).get_matching_blocks()) / max(1, len(a))


def problems(entry, read, heard_as, hz) -> list:
    out = []
    f = heard_as or {}
    g = wants_gender(entry)
    fg = (f.get("gender") or "").lower()
    if g and fg and fg not in (g, "unknown") and not (fg == "child" and wants_age(entry) == "young"):
        out.append(f"sounds {fg}, should be {g}")
    age = wants_age(entry)
    fa = (f.get("age") or "").lower()
    if age == "old" and not re.search(r"elder|old|senior|60|70|80", fa):
        out.append(f"sounds {fa or 'unspecified'}, should be old")
    if age == "young" and not re.search(r"child|teen|young|kid|adolesc", fa):
        out.append(f"sounds {fa or 'unspecified'}, should be young")
    if read < 0.9:
        out.append(f"said {read:.0%} of its line")
    if pitch_problem(entry, hz):
        out.append(pitch_problem(entry, hz))
    return out


def recheck() -> dict:
    """Re-apply the rules to every screened candidate from what was measured
    (transcript, pitch, Fish's description), without transcribing again."""
    entries = castmod.load()
    counts = {"characters": 0, "clean characters": 0, "candidates flagged": 0}
    for key, entry in entries.items():
        f = paths.VOICES / voices.slug(key) / "screen.json"
        if not f.exists():
            continue
        cands = {c["file"]: c for c in voices.candidates(key)}
        rows = json.loads(f.read_text(encoding="utf-8"))
        for r in rows:
            c = cands.get(r["file"], {})
            r["read"] = round(read_share(c.get("reference_text", ""), r.get("heard", "")), 3)
            r["problems"] = problems(entry, r["read"], c.get("heard_as"), r.get("pitch") or None)
            if r.get("bandwidth") and r["bandwidth"] < MUFFLED_HZ:
                r["problems"].append(f"muffled: nothing above {r['bandwidth'] / 1000:.1f} kHz")
            counts["candidates flagged"] += bool(r["problems"])
        f.write_text(json.dumps(rows, indent=1, ensure_ascii=False), encoding="utf-8")
        counts["characters"] += 1
        counts["clean characters"] += any(not r["problems"] for r in rows)
    return counts


def screen_key(key: str, entry: dict) -> list:
    out = []
    folder = paths.VOICES / voices.slug(key)
    for c in voices.candidates(key):
        wav = folder / c["file"]
        pcm = qa.pcm16k(wav)
        heard = " ".join(w["text"] for w in qa.transcribe(wav, "large-v3"))
        read = read_share(c["reference_text"], heard)
        hz = pitch(pcm)
        bw = bandwidth(wav)
        problems_ = problems(entry, read, c.get("heard_as"), hz)
        if bw < MUFFLED_HZ:
            problems_.append(f"muffled: nothing above {bw / 1000:.1f} kHz")
        out.append({"file": c["file"], "read": round(read, 3), "heard": heard, "pitch": round(hz or 0), "bandwidth": bw,
                    "problems": problems_, "embedding": spk.embed(pcm).tolist()})
    return out


def screen(keys) -> dict:
    entries = castmod.load()
    results = {}
    for key in keys:
        res = screen_key(key, entries[key])
        (paths.VOICES / voices.slug(key) / "screen.json").write_text(
            json.dumps([{k: v for k, v in r.items() if k != "embedding"} for r in res], indent=1, ensure_ascii=False),
            encoding="utf-8")
        results[key] = res
    names, vecs = [], []
    if EMB.exists():
        old = np.load(EMB, allow_pickle=False)
        names, vecs = list(old["names"]), list(old["vecs"])
    keep = {(n, i) for i, n in enumerate(names)}
    for key, res in results.items():
        for r in res:
            tag = f"{key}|{r['file']}"
            if tag in names:
                vecs[names.index(tag)] = np.array(r["embedding"], dtype=np.float32)
            else:
                names.append(tag)
                vecs.append(np.array(r["embedding"], dtype=np.float32))
    np.savez(EMB, names=np.array(names), vecs=np.stack(vecs))
    return results


def nearest_others(key_file_pairs=None):
    """For every candidate: the most similar candidate of another character."""
    data = np.load(EMB, allow_pickle=False)
    names, vecs = list(data["names"]), data["vecs"]
    sims = vecs @ vecs.T
    owner = [n.split("|")[0] for n in names]
    out = {}
    for i, n in enumerate(names):
        mask = np.array([o != owner[i] for o in owner])
        j = int(np.argmax(np.where(mask, sims[i], -9)))
        out[n] = (names[j], float(sims[i, j]))
    return out


def screen_all(follow: bool = False, chunk: int = 10) -> int:
    """Screen every character whose candidates aren't all screened, `chunk` at a time
    (each chunk's embeddings saved as it goes). With `follow`, keep going while Voice
    Design is still adding candidates (its log grew in the last five minutes)."""
    import time
    done = 0
    while True:
        entries = castmod.load()
        todo = []
        for key, e in sorted(entries.items(), key=lambda kv: -kv[1]["words"]):
            cands = voices.candidates(key)
            if e.get("voice_of") or not cands:
                continue
            f = paths.VOICES / voices.slug(key) / "screen.json"
            seen = {r["file"] for r in json.loads(f.read_text(encoding="utf-8"))} if f.exists() else set()
            if any(c["file"] not in seen for c in cands):
                todo.append(key)
        for i in range(0, len(todo), chunk):
            screen(todo[i:i + chunk])
            done += len(todo[i:i + chunk])
            print(f"{time.strftime('%H:%M:%S')} screened {done} characters", flush=True)
        log = paths.VOICES / "design.log"
        designing = log.exists() and time.time() - log.stat().st_mtime < 300
        if not follow or (not todo and not designing):
            return done
        if not todo:
            time.sleep(60)
