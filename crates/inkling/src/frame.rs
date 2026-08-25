//! Pure, dependency-free description of a single reveal frame.
//!
//! This module owns the answer to "what does cell `(x, y)` look like at this
//! progress". Every renderer in the crate, the plain-text one below, the diffing
//! terminal one in [`crate::render`], and the live loader, walks frames through
//! [`row`] rather than re-deriving visibility for itself. Colour is layered on
//! top by the terminal renderers; the shape of the frame is decided here, once.

use crate::{art::Art, rank::RankMap, width::glyph_cols};

/// How one cell of a frame appears.
///
/// `cols` is the display width the cell occupies either way, so a hidden wide
/// glyph reserves the same two columns it will take once revealed and the row
/// never shifts sideways as the reveal crosses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// Background, or ink not yet revealed.
    Blank { cols: u16 },
    /// Revealed ink.
    Ink { glyph: char, cols: u16 },
}

impl Paint {
    /// Display columns this cell occupies.
    #[inline]
    pub fn cols(self) -> u16 {
        match self {
            Paint::Blank { cols } | Paint::Ink { cols, .. } => cols,
        }
    }

    /// The revealed glyph, if any.
    #[inline]
    pub fn glyph(self) -> Option<char> {
        match self {
            Paint::Ink { glyph, .. } => Some(glyph),
            Paint::Blank { .. } => None,
        }
    }
}

/// The appearance of one cell at `progress`.
#[inline]
pub fn cell(art: &Art, ranks: &RankMap, progress: f32, x: u16, y: u16) -> Paint {
    let glyph = art.glyph(x, y);
    let cols = glyph_cols(glyph).max(1);
    if ranks.visible_at(x, y, progress) {
        Paint::Ink { glyph, cols }
    } else {
        Paint::Blank { cols }
    }
}

/// Walk row `y` of the frame at `progress`, yielding each cell's grid column, the
/// display column it starts at, and how it paints.
///
/// This is the walk every renderer shares.
pub fn row<'a>(
    art: &'a Art,
    ranks: &'a RankMap,
    progress: f32,
    y: u16,
) -> impl Iterator<Item = (u16, u16, Paint)> + 'a {
    let mut col = 0u16;
    (0..art.width()).map(move |x| {
        let paint = cell(art, ranks, progress, x, y);
        let at = col;
        col = col.saturating_add(paint.cols());
        (x, at, paint)
    })
}

/// Display columns the widest row of `art` occupies.
pub fn art_cols(art: &Art) -> u16 {
    (0..art.height())
        .map(|y| {
            (0..art.width())
                .map(|x| glyph_cols(art.glyph(x, y)).max(1))
                .fold(0u16, u16::saturating_add)
        })
        .max()
        .unwrap_or(0)
}

/// Render the frame at `progress` as plain text: ink whose rank is `<= progress`
/// is shown, everything else is padded with spaces to the same display width.
/// Trailing spaces on each line are trimmed. The result always has exactly
/// `art.height()` lines.
pub fn to_string(art: &Art, ranks: &RankMap, progress: f32) -> String {
    let mut out = String::with_capacity(art.cell_count() + art.height() as usize);
    let mut line = String::with_capacity(art.width() as usize);
    for y in 0..art.height() {
        line.clear();
        for (_, _, paint) in row(art, ranks, progress, y) {
            match paint {
                Paint::Ink { glyph, .. } => line.push(glyph),
                // Reserve the glyph's full width so the row does not shift
                // sideways as the reveal crosses a wide glyph.
                Paint::Blank { cols } => (0..cols).for_each(|_| line.push(' ')),
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ordering::{Geodesic, Ordering};

    #[test]
    fn empty_at_zero_full_at_one() {
        let art = Art::parse("/\\__/\\\n\\____/");
        let ranks = Geodesic::default().rank(&art);

        // Rank 0 exists, so progress 0.0 reveals at least the start cell but not
        // the whole picture; progress 1.0 reveals everything.
        let none = to_string(&art, &ranks, -0.001);
        let all = to_string(&art, &ranks, 1.0);

        assert!(none.trim().chars().all(|c| c.is_whitespace()));
        assert_eq!(all.replace([' ', '\n'], "").len(), art.ink_count());
    }

    #[test]
    fn reveal_is_monotonic() {
        let art = Art::parse("####\n#  #\n####");
        let ranks = Geodesic::default().rank(&art);
        let mut last = 0;
        for i in 0..=10 {
            let shown = to_string(&art, &ranks, i as f32 / 10.0)
                .chars()
                .filter(|c| !c.is_whitespace())
                .count();
            assert!(shown >= last, "reveal went backwards at step {i}");
            last = shown;
        }
        assert_eq!(last, art.ink_count());
    }

    #[test]
    fn always_has_one_line_per_row() {
        let art = Art::parse("#\n#\n#");
        let ranks = Geodesic::default().rank(&art);
        assert_eq!(to_string(&art, &ranks, 0.5).lines().count(), 3);
    }

    /// A hidden cell reserves the columns its glyph will need, so every row keeps
    /// a constant display width for the whole reveal and nothing shifts sideways.
    #[cfg(feature = "unicode")]
    #[test]
    fn row_width_is_constant_across_the_reveal() {
        use crate::width::str_cols;
        let art = Art::parse("世a界b");
        let ranks = Geodesic::default().rank(&art);
        let widths: Vec<u16> = (0..=10)
            .map(|i| {
                let text = to_string(&art, &ranks, i as f32 / 10.0);
                // Measure before the trailing trim, which is cosmetic.
                let padded: String = row(&art, &ranks, i as f32 / 10.0, 0)
                    .map(|(_, _, p)| match p {
                        Paint::Ink { glyph, .. } => glyph.to_string(),
                        Paint::Blank { cols } => " ".repeat(cols as usize),
                    })
                    .collect();
                assert!(text.lines().count() == 1);
                str_cols(&padded)
            })
            .collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "row width drifted during the reveal: {widths:?}"
        );
        assert_eq!(widths[0], 6); // two wide glyphs plus two narrow
    }

    #[test]
    fn art_cols_counts_display_width() {
        let art = Art::parse("ab\nabc");
        assert_eq!(art_cols(&art), 3);
    }
}
