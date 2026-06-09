//! Pure, dependency-free rendering of a single reveal frame to text.
//!
//! This is the canonical, side-effect-free view of the engine: given art, a
//! [`RankMap`] and a progress value it returns exactly what is visible. The
//! terminal renderer ([`crate::render`]) is an optimised, colourful superset of
//! this, but this function is the ground truth the tests and any non-TTY output
//! rely on.

use crate::{art::Art, rank::RankMap};

/// Render the frame at `progress` as plain text: ink whose rank is
/// `<= progress` is shown, everything else is a space. Trailing spaces on each
/// line are trimmed. The result always has exactly `art.height()` lines.
pub fn to_string(art: &Art, ranks: &RankMap, progress: f32) -> String {
    let mut out = String::with_capacity(art.cell_count() + art.height() as usize);
    for y in 0..art.height() {
        let mut line = String::with_capacity(art.width() as usize);
        for x in 0..art.width() {
            if ranks.visible_at(x, y, progress) {
                line.push(art.glyph(x, y));
            } else {
                line.push(' ');
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
}
