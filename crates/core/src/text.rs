//! Text helpers for search, highlighting, and display.

use kjv_library::text::{find_folded, fold};

/// Fold a query (or a verse) for search: case, apostrophes, "æ", and the rest as
/// `kjv_library::text` folds them, with runs of whitespace as one space, so "God  so"
/// finds "God so".
pub fn fold_for_search(s: &str) -> String {
    let mut out = fold(s);
    let mut space = false;
    out.retain(|c| {
        let keep = !(c == ' ' && space);
        space = c == ' ';
        keep
    });
    out
}

/// Byte ranges in `haystack` where the folded `needle` occurs (non-overlapping); a run
/// of whitespace in the needle counts as one space.
pub fn find_folded_ranges(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    let mut one = String::with_capacity(needle.len());
    for c in needle.chars() {
        if c.is_whitespace() {
            if !one.ends_with(' ') {
                one.push(' ');
            }
        } else {
            one.push(c);
        }
    }
    find_folded(haystack, &one)
}

/// A run of verse text with how to draw it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Segment {
    pub text: String,
    /// Words of Christ
    pub red: bool,
    /// Matches the search query
    pub hit: bool,
}

/// Split `text` into runs at every boundary of the red-letter and search-hit byte ranges.
pub fn segments(text: &str, red: &[(usize, usize)], hits: &[(usize, usize)]) -> Vec<Segment> {
    let mut cuts = vec![0, text.len()];
    for &(start, end) in red.iter().chain(hits) {
        cuts.push(start);
        cuts.push(end);
    }
    cuts.sort_unstable();
    cuts.dedup();
    let covers = |ranges: &[(usize, usize)], at: usize| ranges.iter().any(|&(s, e)| s <= at && at < e);

    let mut out: Vec<Segment> = Vec::new();
    for span in cuts.windows(2) {
        let (start, end) = (span[0], span[1]);
        if start == end {
            continue;
        }
        let (red, hit) = (covers(red, start), covers(hits, start));
        match out.last_mut() {
            Some(last) if last.red == red && last.hit == hit => last.text.push_str(&text[start..end]),
            _ => out.push(Segment { text: text[start..end].to_string(), red, hit }),
        }
    }
    out
}

/// Tidy a STEP gloss for display: "and/ <obj.>" -> "and (obj.)".
/// Slashes mirror Hebrew prefix/suffix boundaries; <..> marks words best left untranslated.
pub fn format_gloss(gloss: &str) -> String {
    let text = gloss.replace('/', " ").replace('<', "(").replace('>', ")");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_search_matches_apostrophe_and_ae() {
        assert_eq!(fold_for_search("Moses\u{2019} seat"), fold_for_search("moses' SEAT"));
        assert_eq!(fold_for_search("Cæsar"), "caesar");
    }

    #[test]
    fn ranges_map_back_to_original_bytes() {
        let text = "unto Cæsar the things which are Cæsar\u{2019}s";
        let r = find_folded_ranges(text, "caesar's");
        assert_eq!(r.len(), 1);
        assert_eq!(&text[r[0].0..r[0].1], "Cæsar\u{2019}s");
        let r = find_folded_ranges(text, "CAESAR");
        assert_eq!(r.len(), 2);
        assert_eq!(&text[r[0].0..r[0].1], "Cæsar");
    }

    #[test]
    fn query_whitespace_collapses() {
        assert_eq!(fold_for_search("God  so\t loved"), "god so loved");
        let text = "For God so loved the world";
        let r = find_folded_ranges(text, "God  so");
        assert_eq!(r.iter().map(|&(s, e)| &text[s..e]).collect::<Vec<_>>(), ["God so"]);
    }

    #[test]
    fn segments_combine_red_and_hits() {
        let text = "And Jesus said, Follow me.";
        let red = [(16, 26)];
        let hits = [(20, 26)];
        let s = segments(text, &red, &hits);
        let parts: Vec<(&str, bool, bool)> = s.iter().map(|x| (x.text.as_str(), x.red, x.hit)).collect();
        assert_eq!(parts, vec![("And Jesus said, ", false, false), ("Foll", true, false), ("ow me.", true, true)]);
        // No ranges: one plain segment
        assert_eq!(segments("plain", &[], &[]).len(), 1);
    }

    #[test]
    fn gloss_formatting() {
        assert_eq!(format_gloss("and/ <obj.>"), "and (obj.)");
        assert_eq!(format_gloss("in/ beginning"), "in beginning");
        assert_eq!(format_gloss("[The] book"), "[The] book");
    }
}
