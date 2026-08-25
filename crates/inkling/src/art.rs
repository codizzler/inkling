//! The immutable art model: a rectangular grid of glyphs.
//!
//! Whitespace is *background* (never revealed); every other glyph is *ink*.
//! Parsing is total, any string yields valid art.

/// The largest canvas either axis can hold. Dimensions are `u16`, so anything
/// past this is cropped rather than silently wrapped around.
pub const MAX_DIM: usize = u16::MAX as usize;

/// Columns a tab advances to the next multiple of.
const TAB_WIDTH: usize = 8;

/// A single glyph of the art at a grid position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub glyph: char,
}

/// A parsed piece of ASCII art: a space-padded rectangular grid of glyphs.
#[derive(Clone, Debug, Default)]
pub struct Art {
    width: u16,
    height: u16,
    rows: Vec<Vec<char>>,
}

impl Art {
    /// Parse text into art.
    ///
    /// Tabs expand to the next 8-column stop, lines are right-padded with spaces
    /// to a common width, and the canvas is then cropped to the bounding box of
    /// the ink, so leading indentation and trailing whitespace never enlarge it.
    /// Interior blank rows and columns are preserved. Anything past [`MAX_DIM`]
    /// on either axis is cropped.
    pub fn parse(text: &str) -> Self {
        let mut rows: Vec<Vec<char>> = text
            .split('\n')
            .map(|line| expand_tabs(line.strip_suffix('\r').unwrap_or(line)))
            .collect();

        // Crop rather than wrap: `width as u16` on a longer line would alias.
        rows.truncate(MAX_DIM);
        let width = rows.iter().map(|r| r.len()).max().unwrap_or(0).min(MAX_DIM);
        for r in &mut rows {
            r.truncate(width);
            r.resize(width, ' ');
        }

        // Crop to the bounding box of the ink. Trimming columns as well as rows is
        // what makes the canvas canonical: two files that draw the same picture
        // with different indentation parse to the same `Art`, so the aspect-driven
        // heuristics downstream cannot be swayed by invisible padding.
        let ink_span = |r: &Vec<char>| {
            let first = r.iter().position(|c| !c.is_whitespace())?;
            let last = r.iter().rposition(|c| !c.is_whitespace())?;
            Some((first, last))
        };
        let spans: Vec<Option<(usize, usize)>> = rows.iter().map(ink_span).collect();

        let (rows, width) = match spans.iter().position(|s| s.is_some()) {
            Some(top) => {
                let bottom = spans.iter().rposition(|s| s.is_some()).unwrap();
                let live = &spans[top..=bottom];
                let left = live.iter().flatten().map(|(l, _)| *l).min().unwrap();
                let right = live.iter().flatten().map(|(_, r)| *r).max().unwrap();
                let cropped: Vec<Vec<char>> = rows[top..=bottom]
                    .iter()
                    .map(|r| r[left..=right].to_vec())
                    .collect();
                (cropped, right - left + 1)
            }
            // Entirely blank: an empty canvas, not a zero-height one of some width.
            None => (Vec::new(), 0),
        };

        Art {
            width: width as u16,
            height: rows.len() as u16,
            rows,
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// True when the art holds no cells at all.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The glyph at `(x, y)`, or a space if out of bounds.
    pub fn glyph(&self, x: u16, y: u16) -> char {
        if x >= self.width {
            return ' ';
        }
        self.rows
            .get(y as usize)
            .and_then(|r| r.get(x as usize))
            .copied()
            .unwrap_or(' ')
    }

    /// True when `(x, y)` holds a non-whitespace glyph.
    pub fn is_ink(&self, x: u16, y: u16) -> bool {
        !self.glyph(x, y).is_whitespace()
    }

    /// Row-major flat index of `(x, y)`.
    #[inline]
    pub fn index(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// Total grid cells (`width * height`), ink and background alike.
    pub fn cell_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Every ink cell, in row-major order.
    pub fn ink_cells(&self) -> impl Iterator<Item = Cell> + '_ {
        (0..self.height).flat_map(move |y| {
            (0..self.width).filter_map(move |x| {
                let glyph = self.glyph(x, y);
                (!glyph.is_whitespace()).then_some(Cell { x, y, glyph })
            })
        })
    }

    /// Number of ink cells.
    pub fn ink_count(&self) -> usize {
        self.ink_cells().count()
    }
}

/// Expand tabs to the next [`TAB_WIDTH`] stop. Art files saved by an editor that
/// indents with tabs would otherwise render with one-cell holes and rows that do
/// not line up.
fn expand_tabs(line: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(line.len());
    for c in line.chars() {
        if c == '\t' {
            let stop = (out.len() / TAB_WIDTH + 1) * TAB_WIDTH;
            out.resize(stop, ' ');
        } else if c.is_control() {
            // Art is untrusted text that ends up written straight to a terminal.
            // A control character occupies no columns but can steer the terminal,
            // so an art file could move the cursor or repaint the screen. It is
            // not ink and it is not width; it is background.
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_to_the_ink_bounding_box() {
        let art = Art::parse("\n  ab\n c\n\n");
        assert_eq!(art.height(), 2); // leading and trailing blank rows dropped
        assert_eq!(art.width(), 3); // columns 1..=3 of "  ab" and " c"
        assert_eq!(art.glyph(1, 0), 'a');
        assert_eq!(art.glyph(0, 1), 'c');
        assert!(art.is_ink(1, 0));
        assert!(!art.is_ink(0, 0)); // interior padding is preserved
    }

    #[test]
    fn ink_count_ignores_whitespace() {
        assert_eq!(Art::parse("a b\n c ").ink_count(), 3);
    }

    /// Invisible padding must not change the canvas: two files that draw the same
    /// picture parse identically, so the aspect heuristics cannot be swayed by it.
    #[test]
    fn padding_does_not_change_the_canvas() {
        let bare = Art::parse("##\n##");
        let padded = Art::parse("      ##          \n      ##          ");
        assert_eq!((bare.width(), bare.height()), (2, 2));
        assert_eq!((padded.width(), padded.height()), (2, 2));
    }

    #[test]
    fn interior_blanks_are_preserved() {
        let art = Art::parse("#  #\n\n#  #");
        assert_eq!((art.width(), art.height()), (4, 3));
        assert!(!art.is_ink(1, 0));
        assert_eq!(art.ink_count(), 4);
    }

    #[test]
    fn blank_input_is_an_empty_canvas() {
        for text in ["", "    \n    ", "\n\n\n"] {
            let art = Art::parse(text);
            assert!(art.is_empty(), "{text:?} should parse empty");
            assert_eq!((art.width(), art.height()), (0, 0));
            assert_eq!(art.cell_count(), 0);
        }
    }

    #[test]
    fn tabs_expand_to_eight_column_stops() {
        let art = Art::parse("a\tb");
        assert_eq!(art.width(), 9); // 'a', spaces to column 8, then 'b'
        assert_eq!(art.glyph(0, 0), 'a');
        assert_eq!(art.glyph(8, 0), 'b');
        assert_eq!(art.ink_count(), 2);
    }

    /// Art is untrusted text written straight to a terminal. An escape sequence in
    /// the file must be background, not a zero-width glyph the renderer forwards.
    #[test]
    fn control_characters_are_background() {
        let art = Art::parse("a\x1b[2Jb");
        assert!(!art.glyph(1, 0).is_control());
        assert!(!art.is_ink(1, 0));
        assert_eq!(art.glyph(0, 0), 'a');
        // 'a', a space for the escape, "[2J", then 'b'.
        assert_eq!(art.width(), 6);
        assert_eq!(art.ink_count(), 5);
    }

    /// Oversized input is cropped, never wrapped: `width as u16` on a longer line
    /// would alias 70_000 down to 4_464 and desynchronise every index.
    #[test]
    fn oversized_input_is_cropped_not_wrapped() {
        let art = Art::parse(&"#".repeat(MAX_DIM + 5_000));
        assert_eq!(art.width() as usize, MAX_DIM);
        assert_eq!(art.glyph(art.width() - 1, 0), '#');
    }

    #[test]
    fn glyph_is_a_space_out_of_bounds() {
        let art = Art::parse("ab\ncd");
        assert_eq!(art.glyph(9, 0), ' '); // past the row, not wrapped into row 1
        assert_eq!(art.glyph(0, 9), ' ');
        assert!(!art.is_ink(9, 0));
    }
}
