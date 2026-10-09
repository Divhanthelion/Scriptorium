"""Voice one chapter: script lines -> Fish requests -> MP3 + timings.

A chapter is one request when its voices fit (Fish takes about three minutes
of reference audio per request) and it is not huge; otherwise it is split at
narrator lines into scenes. One request keeps every voice conditioned on what
came before (Fish's condition_on_previous_chunks): no cold starts, no joins.

Requests are cached by a hash of everything that changes the audio, so only
changed scenes are voiced again. Each cached render keeps Fish's word timings,
from which every line and verse gets its start and end.
"""
import hashlib
import json
import re
import subprocess

from . import books, cast as castmod, fish, lexicon, paths, script

SPEED = 1.0
MAX_CHARS = 14000  # about 15 minutes of speech: one request (measured 36 chars/s rendering)


def voice_map(placeholders: dict | None = None) -> dict:
    """{cast key: (Fish voice id, reference seconds, age)} for every key."""
    entries = castmod.load()
    out = {}
    for key in entries:
        base, aged = castmod.resolve(key, entries)
        if placeholders is not None:  # {key: (library voice id, its reference seconds)}, for tests
            vid, secs = placeholders.get(base) or placeholders.get(key) or (None, 0.0)
        else:
            v = entries.get(base, {}).get("voice") or {}
            vid, secs = v.get("id"), v.get("seconds", 12.0)
        out[key] = (vid, secs, aged)
    return out


AGE_CUE = {"old": "[elderly, slower, a little frail]"}


def spoken_lines(code: str, chapter: int) -> list:
    """The chapter's lines with their cast key resolved, in order."""
    book = script.load(code)
    ch = next(c for c in book["chapters"] if c["chapter"] == chapter)
    return [dict(line, key=castmod.voice_key(line, code)) for line in ch["lines"]]


def scenes(lines: list, voices: dict) -> list:
    """Split lines into requests within Fish's reference-audio limit and MAX_CHARS.
    A chapter is cut only where the narrator speaks: it is grouped into beats
    (a narrator line and the speeches after it), and beats are packed in order."""
    beats = []
    for line in lines:
        if voices[line["key"]][0] is None:
            raise SystemExit(f"no voice for {line['key']!r} yet (cast it first, or use placeholders)")
        if not beats or line["who"] == "narrator" and beats[-1][-1]["who"] != "narrator":
            beats.append([])
        beats[-1].append(line)
    out, cur, ids, chars = [], [], {}, 0
    for beat in beats:
        need = {voices[l["key"]][0]: voices[l["key"]][1] for l in beat}
        if sum(need.values()) > fish.MAX_REFERENCE_SECONDS:
            raise SystemExit(f"one beat needs {sum(need.values()):.0f} s of reference audio: "
                             f"{[l['key'] for l in beat]}")
        merged = {**ids, **need}
        size = sum(len(l["text"]) for l in beat)
        if cur and (sum(merged.values()) > fish.MAX_REFERENCE_SECONDS or chars + size > MAX_CHARS):
            out.append(cur)
            cur, ids, chars = [], {}, 0
            merged = dict(need)
        cur += beat
        ids, chars = merged, chars + size
    if cur:
        out.append(cur)
    return out


def request_text(lines: list, voices: dict, deliveries=True):
    """The text Fish reads (speaker tags, delivery cues, phoneme tags) and the voice id list."""
    order, parts, prev = [], [], None
    for line in lines:
        vid, _, aged = voices[line["key"]]
        if vid not in order:
            order.append(vid)
        cue = []
        if deliveries and delivery_cue(line.get("delivery")):
            cue.append(delivery_cue(line["delivery"]))
        if aged in AGE_CUE:
            cue.append(AGE_CUE[aged])
        text = lexicon.apply(line["text"])
        tag = f"<|speaker:{order.index(vid)}|>" if vid != prev or cue else ""
        parts.append(tag + "".join(cue) + (" " if cue else "") + text)
        prev = vid
    text = " ".join(parts)
    if len(order) == 1:  # one voice: no speaker tags (measured harmless, but not needed)
        text = re.sub(r"<\|speaker:\d+\|>", "", text)
    return text, order


def body_for(text, ids):
    # normalize_loudness (the S2 default, stated so it can't change under us) evens out each
    # request's level; quality-guard is Fish's own check on a synthesis (free, no slower: 2026-10-09).
    return {"text": text, "reference_id": ids if len(ids) > 1 else ids[0], "format": "mp3", "mp3_bitrate": 128,
            "latency": "normal", "normalize": True, "chunk_length": 300, "temperature": 0.7, "top_p": 0.7,
            "prosody": {"speed": SPEED, "normalize_loudness": True}, "features": ["quality-guard"]}


def cache_key(body, attempt=0) -> str:
    raw = json.dumps({"model": fish.MODEL, "body": body, "attempt": attempt}, sort_keys=True)
    return hashlib.sha256(raw.encode()).hexdigest()[:24]


def coverage(lines, words) -> float:
    """Share of the script's words that Fish's timed words account for."""
    import difflib
    a = [norm(w) for line in lines for w in line["text"].split()]
    b = [norm(w["text"]) for w in words]
    matched = sum(n for _, _, n in difflib.SequenceMatcher(None, a, b, autojunk=False).get_matching_blocks())
    return matched / max(1, len(a))


MIN_COVERAGE = 0.97  # a stream can end early with no error: once, 50 words of a 500-word scene


def render_scene(lines, voices, deliveries=True, attempts=2):
    text, ids = request_text(lines, voices, deliveries)
    body = body_for(text, ids)
    paths.ensure(paths.RENDERS)
    for attempt in range(attempts):
        key = cache_key(body, attempt)
        mp3, meta = paths.RENDERS / f"{key}.mp3", paths.RENDERS / f"{key}.json"
        failed = paths.RENDERS / f"{key}.incomplete.mp3"
        if failed.exists():
            continue  # this attempt already came back short; don't ask again
        if not mp3.exists():
            audio, words = fish.tts_with_timestamps(body)
            meta.write_text(json.dumps({"words": words, "text": text, "ids": ids}, ensure_ascii=False), encoding="utf-8")
            mp3.write_bytes(audio)
        words = collapse_phonemes(json.loads(meta.read_text(encoding="utf-8"))["words"])
        if coverage(lines, words) >= MIN_COVERAGE:
            return key, mp3, words
        mp3.rename(failed)  # keep the evidence; the next attempt has its own key
    raise Incomplete(f"Fish kept returning incomplete audio for a scene starting {lines[0]['text'][:40]!r}")


class Incomplete(fish.FishError):
    pass


def collapse_phonemes(words):
    """Fish's timings list a phoneme-tagged word as 'phoneme start EH1 ... phoneme end':
    fold those back into one word."""
    out, i = [], 0
    while i < len(words):
        w = words[i]
        if w["text"].lower() == "phoneme" and i + 1 < len(words) and words[i + 1]["text"].lower() == "start":
            j = i + 2
            while j + 1 < len(words) and not (words[j]["text"].lower() == "phoneme" and words[j + 1]["text"].lower() == "end"):
                j += 1
            out.append({"text": "•", "start": w["start"], "end": words[min(j + 1, len(words) - 1)]["end"]})
            i = j + 2
        else:
            out.append(w)
            i += 1
    return out


def norm(w: str) -> str:
    import unicodedata
    # Accents set aside, not dropped with their letters ("á", "corazón", "preñeces")
    w = "".join(c for c in unicodedata.normalize("NFD", w.lower()) if unicodedata.category(c) != "Mn")
    return re.sub(r"[^a-z0-9]", "", w)


def line_times(lines, words):
    """Start and end of each line: the script's words are aligned with Fish's
    timed words (they differ now and then: hyphens, phoneme tags), and each
    line spans its first to last aligned word."""
    import difflib
    script_words, owner = [], []
    for i, line in enumerate(lines):
        for w in line["text"].split():
            script_words.append(norm(w))
            owner.append(i)
    sm = difflib.SequenceMatcher(None, script_words, [norm(w["text"]) for w in words], autojunk=False)
    first, last = {}, {}
    for a, b, n in sm.get_matching_blocks():
        for k in range(n):
            i = owner[a + k]
            first.setdefault(i, words[b + k]["start"])
            last[i] = words[b + k]["end"]
    out = []
    for i in range(len(lines)):
        out.append((first.get(i), last.get(i)))
    return out


# Glyssen's delivery notes that describe the voice, as Fish cues. Notes about
# staging ("to crowd", "giving orders") are left out: the words carry them.
DELIVERY = [
    (r"weep|crying|sob|lament|mourn|wail", "[weeping]"),
    (r"shout|calling out|cry(ing)? out|loud", "[calling out loudly]"),
    (r"whisper", "[whispering]"),
    (r"pray", "[prayerful]"),
    (r"angr|rage|furious|quarrel|rebuk", "[angry]"),
    (r"mock|sneer|taunt|scorn|sarcas|insult", "[mocking]"),
    (r"trembl|afraid|fear|terrif", "[trembling, afraid]"),
    (r"plead|begging|beseech|desperate", "[pleading]"),
    (r"laugh", "[laughing]"),
    (r"in his heart|thinking|thought|to (him|her)self", "[quietly, to himself]"),
    (r"sing|song|chant", "[singing softly]"),
    (r"joy|rejoic|excited|surprised|amazed", "[amazed]"),
    (r"dying|faint|weak", "[weak, strained]"),
]


def delivery_cue(note):
    if not note:
        return None
    for pat, cue in DELIVERY:
        if re.search(pat, note, re.I):
            return cue
    return None


def decode(mp3, rate=44100):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(mp3), "-f", "s16le", "-ac", "1", "-ar", str(rate), "-"],
                         capture_output=True, check=True).stdout
    return raw


def split_scene(scene: list) -> list:
    """Two halves, cut before the narrator line nearest the middle."""
    cuts = [i for i in range(1, len(scene)) if scene[i]["who"] == "narrator" and scene[i - 1]["who"] != "narrator"]
    if not cuts:
        raise SystemExit(f"cannot split a scene of {len(scene)} lines any further")
    mid = min(cuts, key=lambda i: abs(i - len(scene) / 2))
    return [scene[:mid], scene[mid:]]


def render_scenes(scene, voices, deliveries=True):
    """Render a scene; when Fish refuses its references as too long, or keeps
    stopping early (both seen with library voices, whose real reference audio
    is longer than it looks), halve it and try each half."""
    try:
        return [(scene, render_scene(scene, voices, deliveries=deliveries))]
    except fish.FishError as e:
        if "Reference audio too long" not in str(e) and not isinstance(e, Incomplete):
            raise
        out = []
        for half in split_scene(scene):
            out += render_scenes(half, voices, deliveries)
        return out


MAX_VOICE_GAIN_DB = 12.0


def level_voices(pcm: bytes, timeline: list, rate=44100):
    """Bring every voice to the chapter's common speech level, so no character
    is quieter than another. Each voice gets one gain per scene (Fish request),
    from the median loudness of its lines (speech frames only), so a character
    still whispers and shouts within that level. Gains are capped at 12 dB and
    eased in and out over 30 ms at line edges. Returns (pcm, {voice: dB})."""
    import numpy as np

    x = np.frombuffer(pcm, dtype=np.int16).astype(np.float32)
    frame = rate // 100  # 10 ms

    def speech_level(a, b):
        seg = x[int(a * rate):int(b * rate)]
        if len(seg) < frame * 10:
            return None
        rms = np.sqrt(np.mean(seg[: len(seg) // frame * frame].reshape(-1, frame) ** 2, axis=1))
        loud = rms[rms > 0.1 * rms.max()]
        return float(np.median(loud)) if len(loud) else None

    # one gain per voice per scene: each Fish request is levelled on its own, so the same
    # voice can sit a little higher in one scene than in the next
    by_voice = {}
    for t in timeline:
        lvl = speech_level(t["start"], t["end"])
        if lvl:
            by_voice.setdefault((t.get("scene", 0), t["voice"]), []).append(lvl)
    if len(by_voice) < 2:
        return pcm, {}  # one voice in one scene: nothing to balance
    voice_level = {v: float(np.median(ls)) for v, ls in by_voice.items()}
    target = float(np.median([l for ls in by_voice.values() for l in ls]))
    gains = {v: float(np.clip(20 * np.log10(target / l), -MAX_VOICE_GAIN_DB, MAX_VOICE_GAIN_DB))
             for v, l in voice_level.items()}
    env = np.ones_like(x)
    for t in timeline:
        g = 10 ** (gains.get((t.get("scene", 0), t["voice"]), 0.0) / 20)
        env[int(t["start"] * rate):int(t["end"] * rate)] = g
    ramp = int(0.03 * rate)
    env = np.convolve(env, np.ones(ramp) / ramp, mode="same")  # no clicks at line edges
    y = np.clip(x * env, -32768, 32767).astype(np.int16)
    return y.tobytes(), {k: round(g, 1) for k, g in gains.items()}


def render_chapter(code: str, chapter: int, placeholders: dict | None = None, deliveries=True, tag="") -> dict:
    voices = voice_map(placeholders)
    lines = spoken_lines(code, chapter)
    parts, timeline, offset = [], [], 0.0
    gap = b"\x00\x00" * int(44100 * 0.45)  # a breath between scenes
    renders = []
    done = [pair for sc in scenes(lines, voices) for pair in render_scenes(sc, voices, deliveries)]
    for n, (scene, (key, mp3, words)) in enumerate(done):
        renders.append(key)
        pcm = decode(mp3)
        for line, t in zip(scene, line_times(scene, words)):
            if t[0] is None:
                continue
            timeline.append({k: line.get(k) for k in ("v", "kind", "who", "key")} |
                            {"voice": voices[line["key"]][0], "scene": n,
                             "start": round(t[0] + offset, 3), "end": round(t[1] + offset, 3)})
        parts.append(pcm)
        offset += len(pcm) / 2 / 44100
        parts.append(gap)
        offset += len(gap) / 2 / 44100
    out_dir = paths.OUT / (paths.BIBLE + tag)
    paths.ensure(out_dir)
    name = f"{code}.{chapter}"
    wav = out_dir / f"{name}.wav"
    pcm, gains = level_voices(b"".join(parts[:-1]), timeline)
    for t in timeline:
        t["gain_db"] = gains.get((t.get("scene", 0), t["voice"]), 0.0)
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "s16le", "-ar", "44100", "-ac", "1", "-i", "-",
                    "-af", "loudnorm=I=-18:TP=-1.5:LRA=11", "-ar", "44100", "-c:a", "libmp3lame", "-b:a", "96k",
                    str(out_dir / f"{name}.mp3")], input=pcm, check=True)
    verses, seen = [], set()
    for t in timeline:
        if t.get("v") is not None and t["v"] not in seen:
            seen.add(t["v"])
            verses.append([str(t["v"]), t["start"]])
    timing = {"verses": verses, "duration": round(len(pcm) / 2 / 44100, 3), "lines": timeline, "renders": renders,
              "title": books.announcement(code, chapter)}
    (out_dir / f"{name}.json").write_text(json.dumps(timing, ensure_ascii=False, indent=0), encoding="utf-8")
    wav.unlink(missing_ok=True)
    return timing
