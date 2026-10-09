"""The Reina-Valera 1909's script never changes a word: every verse's lines rejoin to
the verse as the app has it; and the rules that split a verse do what they say.

    cd drama && python tests/test_script_es.py
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from abible import bible, es, project_es, script_es  # noqa: E402
from abible.glyssen import NARRATOR, Piece  # noqa: E402


def test_every_verse_rejoins_exactly():
    total = 0
    for code in bible.verses("rv1909"):
        assert script_es.check_book(script_es.load(code)) == [], code
        total += len(bible.verses("rv1909")[code])
    assert total == 31_102


def test_every_chapter_is_announced_first():
    for code in bible.verses("rv1909"):
        for ch in script_es.load(code)["chapters"]:
            first = ch["lines"][0]
            assert first.get("kind") == "announce" and first["who"] == NARRATOR, (code, ch["chapter"])
            assert first["text"] == es.announcement(code, ch["chapter"])


def test_announcements_in_spanish():
    assert es.announcement("GEN", 1) == "Génesis. Capítulo uno."
    assert es.announcement("GEN", 50) == "Génesis, capítulo cincuenta."
    assert es.announcement("1SA", 21) == "Primero de Samuel, capítulo veintiuno."
    assert es.announcement("1CO", 13) == "Primera de Corintios, capítulo trece."
    assert es.announcement("PSA", 1) == "Salmos. Salmo uno."
    assert es.announcement("PSA", 119) == "Salmo ciento diecinueve."


def test_overrides_rejoin_too():
    if not script_es.OVERRIDES.exists():
        return
    for ref, fix in json.loads(script_es.OVERRIDES.read_text(encoding="utf-8"))["verses"].items():
        code, cv = ref.split(" ")
        c, v = cv.split(":")
        assert " ".join(line[1] for line in fix["lines"]) == bible.verses("rv1909")[code][(int(c), v)], ref
        assert fix.get("reason"), f"{ref}: every override says why"


def test_speech_opens_at_a_colon_and_a_capital():
    pieces = [Piece(NARRATOR, None, None, "Y dijo Dios:"), Piece("God", None, None, "«Sea la luz»,"),
              Piece(NARRATOR, None, None, "y fue la luz.")]
    segs, notes = project_es.project_verse("Y dijo Dios: Sea la luz: y fué la luz.", pieces)
    assert [(p.who, t) for p, t in segs] == [(NARRATOR, "Y dijo Dios:"), ("God", "Sea la luz:"), (NARRATOR, "y fué la luz.")]
    assert notes == []


def test_a_question_opens_speech_too():
    pieces = [Piece(NARRATOR, None, None, "Entonces Yahvé Dios dijo a la mujer:"), Piece("God", None, None, "«¿Qué es lo que has hecho?»")]
    segs, _ = project_es.project_verse("Entonces Jehová Dios dijo á la mujer: ¿Qué es lo que has hecho?", pieces)
    assert [p.who for p, _ in segs] == [NARRATOR, "God"]


def test_indirect_speech_stays_with_the_narrator():
    pieces = [Piece(NARRATOR, None, None, "Mandó a la multitud"), Piece("Jesus", None, None, "«Siéntense en el suelo»")]
    segs, notes = project_es.project_verse("Entonces mandó á la multitud que se recostase en tierra.", pieces)
    assert [p.who for p, _ in segs] == [NARRATOR]
    assert notes and notes[0].startswith("merged")


def test_quotations_in_the_bridge():
    # A speech that runs on from the verse before, closed here
    text = "Pero hay algunos de vosotros que no creen». Porque Jesús sabía desde el principio quiénes eran."
    segs, _, _ = project_es.segments(text, project_es.start_depth(text, True))
    assert [q for q, _ in segs] == [True, False]
    # The full stop after a closing mark is no part of its own
    segs, _, _ = project_es.segments("Y él respondió: «Oí tu voz en el huerto»." , 0)
    assert [q for q, _ in segs] == [False, True]
    # The WEB's apostrophes aren't quotation marks
    segs, _, _ = project_es.segments("Then his sister said to Pharaoh’s daughter,", 0, ("«‹", "»›"))
    assert [q for q, _ in segs] == [False]


def test_a_narrator_who_speaks_keeps_his_words():
    # Amos narrates his book: "I said, «A plumb line.»" is the narrator's
    web = [Piece(NARRATOR, None, None, "Yahweh said to me,"), Piece("God", None, None, "«Amos, what do you see?»"),
           Piece(NARRATOR, None, None, "I said, «A plumb line.» Then the Lord said,"), Piece("God", None, None, "«Behold…»")]
    blm = "Yahvé me preguntó: «¿Qué ves, Amós?». Y respondí: «Una plomada». Entonces el Señor dijo: «He aquí…»"
    segs, _, _ = project_es.segments(blm, 0)
    pieces, _ = project_es.label(segs, web)
    assert [p.who for p in pieces] == [NARRATOR, "God", NARRATOR, "God"]
    assert "Una plomada" in pieces[2].text


def test_rv1909_counterparts_go_through_the_alignment():
    # RV1909 divides 1 Samuel 23-24 as the Hebrew: its 24:1 is the KJV's 23:29
    assert bible.counterparts("rv1909", ("1SA", 24, "1"), "blm") == [("1SA", 23, "29")]
    lines = [l for ch in script_es.load("1SA")["chapters"] if ch["chapter"] == 23 for l in ch["lines"] if l.get("v") == 29]
    assert lines == [], "an empty verse has nothing to read"


if __name__ == "__main__":
    for name, fn in list(globals().items()):
        if name.startswith("test_"):
            fn()
            print("ok", name)
