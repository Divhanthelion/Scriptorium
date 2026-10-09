"""Where things live. Everything generated but not committed goes under drama/.cache.

ABIBLE_BIBLE names the translation a run is for ("kjv", the default, or "rv1909"):
its script, cast, voice candidates, lexicon and output are kept apart, while the
casting rules and hand-written briefs are shared (one character, one brief)."""
import os
from pathlib import Path

BIBLE = os.environ.get("ABIBLE_BIBLE", "kjv")
# The language of its words: what the voices speak and what the checks listen for
LANGUAGE = {"kjv": "en", "rv1909": "es"}.get(BIBLE, "en")

AUDIO = Path(__file__).resolve().parent.parent
REPO = AUDIO.parent
CACHE = AUDIO / ".cache"
SOURCES = CACHE / "sources"
RENDERS = CACHE / "renders"
# The KJV's candidates stay where they always were
VOICES = CACHE / "voices" if BIBLE == "kjv" else CACHE / "voices" / BIBLE
OUT = CACHE / "out"

SCRIPT = AUDIO / "script"
CAST = AUDIO / "cast"
LEXICON = AUDIO / "lexicon"


def ensure(*dirs: Path) -> None:
    for d in dirs:
        d.mkdir(parents=True, exist_ok=True)
