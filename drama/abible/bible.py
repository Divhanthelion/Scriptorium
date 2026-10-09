"""The app's own texts and verse alignment, for translations other than the KJV.

Texts come from Scriptorium's library exactly as the app reads them, written by
`cargo run --release -p kjv-import --example verses -- <id> drama/.cache/text/<id>.json`.
The alignment tables are the app's (data/library/alignment/<id>.tsv): every verse a
translation numbers otherwise than the KJV, with its KJV counterparts. Through them a
verse of one translation finds its counterpart in another (RV1909's 1 Samuel 24:1 is
the KJV's 23:29, and so the Biblia libre's 23:29)."""
import json
from functools import lru_cache

from . import paths

TEXT = paths.CACHE / "text"
ALIGNMENT = paths.REPO / "data" / "library" / "alignment"


@lru_cache(maxsize=None)
def verses(id: str) -> dict:
    """{code: {(chapter, number): text}} in order; a Psalm title is number "0"."""
    path = TEXT / f"{id}.json"
    if not path.exists():
        raise FileNotFoundError(f"{path} is missing: run `cargo run --release -p kjv-import --example verses -- {id} {path}`")
    books = json.loads(path.read_text(encoding="utf-8"))["books"]
    return {code: {(c, n): t for c, n, _title, t in rows} for code, rows in books.items()}


@lru_cache(maxsize=None)
def toc(id: str) -> dict:
    """{code: the book's name as the edition prints it (its USFM \\toc1)}."""
    out = {}
    for f in sorted((paths.REPO / "data" / "library" / "bibles" / id).glob("*.usfm")):
        for line in f.read_text(encoding="utf-8").splitlines():
            if line.startswith("\\toc1 "):
                out[f.stem] = line[6:].strip()
                break
    return out


def has(id: str, ref) -> bool:
    code, c, n = ref
    return (c, n) in verses(id).get(code, {})


@lru_cache(maxsize=None)
def _table(id: str):
    to_kjv, from_kjv = {}, {}
    path = ALIGNMENT / f"{id}.tsv"
    if not path.exists():  # the KJV itself
        return to_kjv, from_kjv
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        book, here, there, _how, _score = line.split("\t")
        natives = [(book, int(r.split(":")[0]), r.split(":")[1]) for r in here.split("+")]
        kjv = [] if there == "-" else [(r.split(" ")[0], int(r.split(" ")[1].split(":")[0]), r.split(":")[1]) for r in there.split("+")]
        for n in natives:
            to_kjv[n] = kjv
            for k in kjv:
                from_kjv.setdefault(k, [])
                if n not in from_kjv[k]:
                    from_kjv[k].append(n)
    return to_kjv, from_kjv


def to_kjv(id: str, ref) -> list:
    """The KJV verses verse `ref` of translation `id` corresponds to (as the app maps them)."""
    listed, _ = _table(id)
    if ref in listed:
        return listed[ref]
    return [ref] if has("kjv", ref) else []


def from_kjv(id: str, kref) -> list:
    """The verses of translation `id` that correspond to KJV verse `kref`."""
    listed, back = _table(id)
    out = list(back.get(kref, []))
    if kref not in listed and has(id, kref) and kref not in out:
        out.append(kref)
    number = lambda r: int(r[2].split("-")[0]) if r[2].split("-")[0].isdigit() else 0
    return sorted(out, key=lambda r: (r[0], r[1], number(r), r[2]))


def counterparts(id: str, ref, other: str) -> list:
    """Translation `other`'s verses for verse `ref` of translation `id`, through the KJV."""
    out = []
    for k in to_kjv(id, ref):
        for r in from_kjv(other, k):
            if r not in out:
                out.append(r)
    return out
