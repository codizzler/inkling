//! The immutable art model: a rectangular grid of glyphs.
//!
//! Whitespace is *background* (never revealed); every other glyph is *ink*.
//! Parsing is total, any string yields valid art.

/// A single glyph of the art at a grid position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub x: u16,
    pub y: u16,
    pub glyph: char,
}

/// A parsed piece of ASCII art: a space-padded rectangular grid of glyphs.
#[derive(Clone, Debug)]
pub struct Art {
    width: u16,
    height: u16,
    rows: Vec<Vec<char>>,
}

impl Art {
    /// Parse text into art. Lines are right-padded with spaces to a common
    /// width; fully blank rows at the top and bottom are trimmed so the canvas
    /// hugs the drawing. Interior blank rows are preserved.
    pub fn parse(text: &str) -> Self {
        let mut rows: Vec<Vec<char>> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).chars().collect())
            .collect();

        let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        for r in &mut rows {
            if r.len() < width {
                r.resize(width, ' ');
            }
        }

        // Trim fully-blank rows at the top and bottom in one O(rows) pass.
        let is_blank = |r: &Vec<char>| r.iter().all(|c| c.is_whitespace());
        let rows: Vec<Vec<char>> = match rows.iter().position(|r| !is_blank(r)) {
            Some(first) => {
                let last = rows.iter().rposition(|r| !is_blank(r)).unwrap();
                rows[first..=last].to_vec()
            }
            None => Vec::new(), // entirely blank
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

    /// The glyph at `(x, y)`, or a space if out of bounds.
    pub fn glyph(&self, x: u16, y: u16) -> char {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_outer_blank_rows_and_pads_width() {
        let art = Art::parse("\n  ab\nc\n\n");
        assert_eq!(art.height(), 2); // leading + trailing blanks dropped
        assert_eq!(art.width(), 4); // widened to "  ab"
        assert_eq!(art.glyph(2, 0), 'a');
        assert_eq!(art.glyph(0, 1), 'c');
        assert!(art.is_ink(2, 0));
        assert!(!art.is_ink(0, 0)); // padding space
    }

    #[test]
    fn ink_count_ignores_whitespace() {
        assert_eq!(Art::parse("a b\n c ").ink_count(), 3);
    }
}
