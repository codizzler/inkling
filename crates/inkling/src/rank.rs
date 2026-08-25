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

    /// Flat index of `(x, y)`, or `None` when either axis is out of bounds.
    ///
    /// Both bounds matter: a bare `y * width + x` would let an out-of-range `x`
    /// alias into the next row and hand back a neighbour's rank.
    #[inline]
    fn index(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.width && y < self.height).then(|| y as usize * self.width as usize + x as usize)
    }

    /// Assign `rank` to the cell at `(x, y)`. Out-of-bounds writes are ignored.
    pub fn set(&mut self, x: u16, y: u16, rank: f32) {
        if let Some(i) = self.index(x, y) {
            self.ranks[i] = Some(rank);
        }
    }

    /// The rank of `(x, y)`, or `None` if it is background or out of bounds.
    #[inline]
    pub fn rank_at(&self, x: u16, y: u16) -> Option<f32> {
        self.ranks[self.index(x, y)?]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An out-of-range `x` must not alias into the next row.
    #[test]
    fn out_of_bounds_reads_are_none() {
        let mut map = RankMap::new(4, 3);
        map.set(1, 1, 0.5); // flat index 5

        assert_eq!(map.rank_at(1, 1), Some(0.5));
        assert_eq!(map.rank_at(5, 0), None, "x wrapped into the next row");
        assert!(!map.visible_at(5, 0, 1.0));
        assert_eq!(map.rank_at(0, 3), None);
        assert_eq!(map.rank_at(u16::MAX, u16::MAX), None);
    }

    #[test]
    fn out_of_bounds_writes_are_ignored() {
        let mut map = RankMap::new(4, 3);
        map.set(5, 0, 0.5); // would have landed at flat index 5, i.e. (1, 1)
        assert_eq!(map.rank_at(1, 1), None);
        assert_eq!(map.ink_count(), 0);
    }

    #[test]
    fn empty_map_is_inert() {
        let map = RankMap::new(0, 0);
        assert_eq!(map.rank_at(0, 0), None);
        assert_eq!(map.ink_count(), 0);
    }
}
