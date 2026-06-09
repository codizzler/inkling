//! [`RankMap`]: the reveal schedule.
//!
//! Every ink cell carries a rank in `0..=1`; a cell is visible when
//! `rank <= progress`. Background cells carry no rank and never appear.

/// A per-cell reveal schedule produced by an [`crate::Ordering`].
#[derive(Clone, Debug)]
pub struct RankMap {
    width: u16,
    height: u16,
    /// `ranks[index] == Some(r)` for ink cells, `None` for background.
    ranks: Vec<Option<f32>>,
}

impl RankMap {
    /// An all-background map of the given size; fill it in via [`set`](Self::set).
    pub fn new(width: u16, height: u16) -> Self {
        RankMap {
            width,
            height,
            ranks: vec![None; width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    #[inline]
    fn index(&self, x: u16, y: u16) -> usize {
        y as usize * self.width as usize + x as usize
    }

    /// Assign `rank` to the cell at `(x, y)`.
    pub fn set(&mut self, x: u16, y: u16, rank: f32) {
        let i = self.index(x, y);
        self.ranks[i] = Some(rank);
    }

    /// The rank of `(x, y)`, or `None` if it is background / out of bounds.
    #[inline]
    pub fn rank_at(&self, x: u16, y: u16) -> Option<f32> {
        self.ranks.get(self.index(x, y)).copied().flatten()
    }

    /// True when `(x, y)` is ink and revealed at `progress`.
    #[inline]
    pub fn visible_at(&self, x: u16, y: u16, progress: f32) -> bool {
        matches!(self.rank_at(x, y), Some(r) if r <= progress)
    }

    /// Number of ranked (ink) cells.
    pub fn ink_count(&self) -> usize {
        self.ranks.iter().filter(|r| r.is_some()).count()
    }
}
