"""Spanish: numbers in words and the narrator's chapter announcements.

The RV1909 as eBible prints it gives each book a short name ("Génesis", "1 Samuel",
"San Mateo"); the announcements read those names, with a book's number said in words
as Spanish readers say it ("Primero de Samuel", "Primera de Corintios": the Old
Testament's books are "libros", the letters "epístolas")."""
import re

from . import bible

_UNITS = ("cero uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince "
          "dieciséis diecisiete dieciocho diecinueve veinte veintiuno veintidós veintitrés veinticuatro "
          "veinticinco veintiséis veintisiete veintiocho veintinueve").split()
_TENS = "_ _ _ treinta cuarenta cincuenta sesenta setenta ochenta noventa".split()
_ORDINAL_M = {"1": "Primero", "2": "Segundo"}
_ORDINAL_F = {"1": "Primera", "2": "Segunda", "3": "Tercera"}
_LETTERS = {"1CO", "2CO", "1TH", "2TH", "1TI", "2TI", "1PE", "2PE", "1JN", "2JN", "3JN"}


def number_words(n: int) -> str:
    """150 -> 'ciento cincuenta'; 21 -> 'veintiuno' (as a chapter is counted)."""
    if n < 30:
        return _UNITS[n]
    if n < 100:
        return _TENS[n // 10] + ("" if n % 10 == 0 else " y " + _UNITS[n % 10])
    if n == 100:
        return "cien"
    return "ciento " + number_words(n - 100) if n < 200 else str(n)


def book_name(code: str, translation: str = "rv1909") -> str:
    """The book's name as the edition prints it, a leading number said in words."""
    toc = bible.toc(translation).get(code, code)
    m = re.match(r"([123])\s+(.*)", toc)
    if not m:
        return toc
    ordinal = (_ORDINAL_F if code in _LETTERS else _ORDINAL_M)[m.group(1)]
    return f"{ordinal} de {m.group(2)}"


def announcement(code: str, chapter: int, translation: str = "rv1909") -> str:
    """'Génesis. Capítulo uno.', then 'Génesis, capítulo dos.'; the Psalms: 'Salmos.
    Salmo uno.', then 'Salmo dos.'"""
    name = book_name(code, translation)
    if code == "PSA":
        label = f"Salmo {number_words(chapter)}."
        return f"{name}. {label}" if chapter == 1 else label
    if chapter == 1:
        return f"{name}. Capítulo {number_words(chapter)}."
    return f"{name}, capítulo {number_words(chapter)}."
