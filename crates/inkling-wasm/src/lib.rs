//! WebAssembly bindings for inkling, built on the same pure Rust core.
//!
//! The browser has no terminal, so this exposes the engine's *model* rather than a
//! renderer: parse art, choose an [`Ordering`], and read back the per-cell reveal
//! ranks. JavaScript then draws each frame however it likes (a cell is visible when
//! its rank is `<= progress`), which is exactly how the native renderers work.
//!
//! ```js
//! import init, { Reveal } from "inkling";
//! await init();
//! const reveal = new Reveal(art, "geodesic");
//! const ranks = reveal.ranks();   // Float32Array, -1 for background
//! const glyphs = reveal.glyphs(); // width*height chars, row-major
//! // show cell i when ranks[i] >= 0 && ranks[i] <= progress
//! ```

use inkling_core::ordering::{Direction, Directional, Geodesic, Ordering};
use inkling_core::{frame, Art, RankMap};
use wasm_bindgen::prelude::*;

/// A prepared reveal: art plus the reveal rank of every cell.
#[wasm_bindgen]
pub struct Reveal {
    art: Art,
    rank_map: RankMap,
}

#[wasm_bindgen]
impl Reveal {
    /// Build a reveal for `art`. `ordering` selects how it paints: `"auto"`
    /// (the smart directional default), `"top"`, `"bottom"`, `"left"`, `"right"`,
    /// or `"geodesic"` (trace the spine).
    #[wasm_bindgen(constructor)]
    pub fn new(art: &str, ordering: &str) -> Reveal {
        let art = Art::parse(art);
        let rank_map = match ordering {
            "geodesic" => Geodesic::default().rank(&art),
            "top" => Directional(Direction::TopToBottom).rank(&art),
            "bottom" => Directional(Direction::BottomToTop).rank(&art),
            "left" => Directional(Direction::LeftToRight).rank(&art),
            "right" => Directional(Direction::RightToLeft).rank(&art),
            _ => Directional(Direction::Auto).rank(&art),
        };
        Reveal { art, rank_map }
    }

    /// Grid width in cells.
    pub fn width(&self) -> u16 {
        self.art.width()
    }

    /// Grid height in cells.
    pub fn height(&self) -> u16 {
        self.art.height()
    }

    /// The glyph grid, row-major, as one string of `width * height` chars (spaces
    /// for background, no newlines).
    pub fn glyphs(&self) -> String {
        let (w, h) = (self.art.width(), self.art.height());
        let mut s = String::with_capacity(w as usize * h as usize);
        for y in 0..h {
            for x in 0..w {
                s.push(self.art.glyph(x, y));
            }
        }
        s
    }

    /// Reveal rank of every cell, row-major, length `width * height`. Background
    /// cells are `-1`; ink cells are in `0..=1`.
    pub fn ranks(&self) -> Vec<f32> {
        let (w, h) = (self.art.width(), self.art.height());
        let mut v = Vec::with_capacity(w as usize * h as usize);
        for y in 0..h {
            for x in 0..w {
                v.push(self.rank_map.rank_at(x, y).unwrap_or(-1.0));
            }
        }
        v
    }

    /// The plain-text frame at `progress`, the same pure renderer the tests use.
    pub fn frame(&self, progress: f32) -> String {
        frame::to_string(&self.art, &self.rank_map, progress)
    }
}
