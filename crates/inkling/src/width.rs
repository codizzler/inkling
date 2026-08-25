//! How many terminal columns a glyph occupies.
//!
//! Every renderer measures cells through this one function, so a hidden cell
//! reserves exactly as many columns as the glyph will need once it appears. That
//! is what keeps a row from shifting sideways as the reveal crosses a wide glyph.

/// Display columns a glyph occupies: `0` for zero-width and combining marks, `2`
/// for wide glyphs (CJK and many emoji), `1` otherwise.
///
/// With the `unicode` feature, on by default, this is the real East Asian width
/// from [`unicode-width`](https://crates.io/crates/unicode-width). Without it
/// every glyph counts as one column, which is exact for the ASCII the crate is
/// named for and keeps the core free of dependencies.
#[inline]
pub fn glyph_cols(c: char) -> u16 {
    #[cfg(feature = "unicode")]
    {
        unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) as u16
    }
    #[cfg(not(feature = "unicode"))]
    {
        let _ = c;
        1
    }
}

/// Total display columns of a string.
pub fn str_cols(s: &str) -> u16 {
    s.chars().map(glyph_cols).fold(0u16, u16::saturating_add)
}

/// Truncate `s` to at most `max` display columns, dropping whole glyphs so a wide
/// glyph is never split across the edge.
pub fn truncate_to_cols(s: &str, max: u16) -> String {
    let mut out = String::new();
    let mut used = 0u16;
    for c in s.chars() {
        let w = glyph_cols(c);
        if used + w > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

/// Replace control characters with spaces.
///
/// Captions arrive from anywhere: the CLI reads them off a pipe, so `make 2>&1 |
/// inkling` puts another program's output on the caption line. A control
/// character costs no display columns but can move the cursor or repaint the
/// screen, which would let that output steer the terminal it was only meant to
/// label. Neutralize them at the door rather than at every write.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_display_width() {
        assert_eq!(truncate_to_cols("abc", 2), "ab");
        assert_eq!(truncate_to_cols("abc", 0), "");
    }

    #[test]
    fn sanitize_neutralizes_escape_sequences() {
        // A caption that would otherwise clear the screen and move the cursor.
        assert_eq!(sanitize("done\x1b[2J\x1b[H"), "done [2J [H");
        assert_eq!(sanitize("plain caption"), "plain caption");
        // One glyph in, one glyph out: a control character becomes a space rather
        // than vanishing, so anything measured alongside it keeps its alignment.
        let caption = "a\x1b[31mb";
        assert_eq!(sanitize(caption).chars().count(), caption.chars().count());
        assert!(!sanitize(caption).chars().any(char::is_control));
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn wide_glyphs_count_two() {
        assert_eq!(glyph_cols('a'), 1);
        assert_eq!(glyph_cols('世'), 2);
        assert_eq!(str_cols("a世"), 3);
        assert_eq!(truncate_to_cols("a世", 3), "a世"); // 1 + 2 == 3 fits
        assert_eq!(truncate_to_cols("世界", 3), "世"); // 2 + 2 > 3, drop the second
    }
}
