//! Text folded for searching: case and typography set aside, so "Moses’" finds
//! "moses'", "Caesar" finds "Cæsar", "corazon" finds "corazón", and Greek typed with a
//! keyboard's accents finds it printed with the polytonic ones.

/// Fold one char for case/typography-insensitive search.
/// Curly apostrophes match straight ones and "æ" matches "ae" (Cæsar ↔ Caesar). Greek
/// vowels with an oxia (U+1F71…, as Chrysostom's Greek is printed) are the same letters
/// as with a tonos (U+03AC…, as Greek keyboards type them), so they fold together, and
/// final sigma is sigma (as "Σ" lower-cased is).
pub fn fold_char(c: char, out: &mut String) {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201B}' | '\u{02BC}' => out.push('\''),
        '\u{201C}' | '\u{201D}' => out.push('"'),
        '\u{2010}'..='\u{2014}' => out.push('-'),
        'æ' | 'Æ' => out.push_str("ae"),
        '\u{1F71}' | '\u{1FBB}' => out.push('\u{03AC}'),
        '\u{1F73}' | '\u{1FC9}' => out.push('\u{03AD}'),
        '\u{1F75}' | '\u{1FCB}' => out.push('\u{03AE}'),
        '\u{1F77}' | '\u{1FDB}' => out.push('\u{03AF}'),
        '\u{1F79}' | '\u{1FF9}' => out.push('\u{03CC}'),
        '\u{1F7B}' | '\u{1FEB}' => out.push('\u{03CD}'),
        '\u{1F7D}' | '\u{1FFB}' => out.push('\u{03CE}'),
        '\u{1FD3}' => out.push('\u{0390}'),
        '\u{1FE3}' => out.push('\u{03B0}'),
        '\u{03C2}' => out.push('\u{03C3}'),
        c if c.is_whitespace() => out.push(' '),
        c => out.extend(c.to_lowercase().map(unaccented)),
    }
}

/// A Latin letter without its accent, so "corazon" finds "corazón", "coracao" finds
/// "coração", and "Elohim" finds "Elohîm" (lower case in, lower case out). Letters such
/// as "ø" and "ß" are letters of their own, not accented ones, and stay.
fn unaccented(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ğ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ř' => 'r',
        'ś' | 'š' | 'ş' => 's',
        'ť' | 'ţ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    }
}

/// Fold a whole string for search comparisons.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        fold_char(c, &mut out);
    }
    out
}

/// Byte ranges in `haystack` where the folded `needle` occurs (non-overlapping).
pub fn find_folded(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle = fold(needle);
    if needle.is_empty() {
        return Vec::new();
    }

    // Folded text plus, for each folded byte, the original char's byte range.
    let mut folded = String::with_capacity(haystack.len());
    let mut map: Vec<(usize, usize)> = Vec::with_capacity(haystack.len());
    for (i, c) in haystack.char_indices() {
        let before = folded.len();
        fold_char(c, &mut folded);
        for _ in before..folded.len() {
            map.push((i, i + c.len_utf8()));
        }
    }

    let mut ranges = Vec::new();
    let mut start = 0;
    while let Some(pos) = folded[start..].find(&needle) {
        let abs = start + pos;
        let end = abs + needle.len();
        ranges.push((map[abs].0, map[end - 1].1));
        start = end;
    }
    ranges
}

/// The words of folded text, for the search index: its runs of letters and digits.
/// Every run of letters and digits in a folded query lies inside one of these in any
/// text the query is found in, which is what lets the index rule texts out.
pub fn words(folded: &str) -> impl Iterator<Item = &str> {
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_search_matches_apostrophe_and_ae() {
        assert_eq!(fold("Moses\u{2019} seat"), fold("moses' SEAT"));
        assert_eq!(fold("Cæsar"), "caesar");
    }

    #[test]
    fn greek_oxia_and_tonos_fold_together() {
        // λόγος and Ἀλλά as Chrysostom's text prints them, and as a keyboard types them
        assert_eq!(fold("\u{03BB}\u{1F79}\u{03B3}\u{03BF}\u{03C2}"), fold("\u{03BB}\u{03CC}\u{03B3}\u{03BF}\u{03C2}"));
        assert_eq!(fold("\u{1F08}\u{03BB}\u{03BB}\u{1F71}"), fold("\u{1F08}\u{03BB}\u{03BB}\u{03AC}"));
        // Capitals too
        assert_eq!(fold("\u{1FF9}"), fold("\u{038C}"));
        let text = "\u{03BB}\u{1F79}\u{03B3}\u{03BF}\u{03C2}";
        assert_eq!(find_folded(text, "\u{039B}\u{038C}\u{0393}\u{039F}\u{03A3}"), [(0, text.len())]);
    }

    #[test]
    fn latin_accents_set_aside() {
        assert_eq!(fold("Corazón"), "corazon");
        assert_eq!(fold("CORAÇÃO"), "coracao");
        assert_eq!(fold("Año, pingüino"), "ano, pinguino");
        assert_eq!(fold("Elohîm"), "elohim");
        let text = "Bienaventurados los de limpio corazón: porque ellos verán á Dios.";
        let at = text.find("corazón").unwrap();
        assert_eq!(find_folded(text, "CORAZON"), [(at, at + "corazón".len())]);
        // Letters of their own, not accented ones
        assert_eq!(fold("Ø ß"), "ø ß");
    }

    #[test]
    fn ranges_map_back_to_original_bytes() {
        let text = "unto Cæsar the things which are Cæsar\u{2019}s";
        let r = find_folded(text, "caesar's");
        assert_eq!(r.len(), 1);
        assert_eq!(&text[r[0].0..r[0].1], "Cæsar\u{2019}s");
        let r = find_folded(text, "CAESAR");
        assert_eq!(r.len(), 2);
        assert_eq!(&text[r[0].0..r[0].1], "Cæsar");
    }

    #[test]
    fn words_split_at_everything_but_letters_and_digits() {
        let folded = fold("The LORD’s—house, 3:16; Cæsar");
        let w: Vec<&str> = words(&folded).collect();
        assert_eq!(w, ["the", "lord", "s", "house", "3", "16", "caesar"]);
    }
}
