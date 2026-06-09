//! Orderings turn [`Art`] into a [`RankMap`].
//!
//! This is the single seam where "reveal the art in a way that *depends on the
//! art*" lives. Implement [`Ordering`] and you control the choreography; the
//! rest of the engine (rendering, easing, diffing) is oblivious to how ranks
//! were chosen.

use std::collections::VecDeque;

use crate::{art::Art, rank::RankMap};

/// Assigns every ink cell a reveal rank in `0..=1`.
pub trait Ordering {
    fn rank(&self, art: &Art) -> RankMap;
}

// ---------------------------------------------------------------------------
// Scanline, the trivial geometric baseline.
// ---------------------------------------------------------------------------

/// Reveal in reading order: top-to-bottom, left-to-right.
///
/// The dullest possible ordering, included as a baseline and as a deterministic
/// tie-breaker for richer strategies.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scanline;

impl Ordering for Scanline {
    fn rank(&self, art: &Art) -> RankMap {
        let mut map = RankMap::new(art.width(), art.height());
        let cells: Vec<_> = art.ink_cells().collect();
        let denom = cells.len().saturating_sub(1).max(1) as f32;
        for (i, cell) in cells.iter().enumerate() {
            map.set(cell.x, cell.y, i as f32 / denom);
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Directional, a clean wipe along one axis.
// ---------------------------------------------------------------------------

/// The direction a [`Directional`] reveal sweeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Row by row from the top. Good for tall art. (default)
    #[default]
    TopToBottom,
    /// Row by row from the bottom.
    BottomToTop,
    /// Column by column from the left.
    LeftToRight,
    /// Column by column from the right.
    RightToLeft,
    /// Top to bottom unless the art reads much wider than tall. The smart default.
    Auto,
}

/// Reveal the art as a clean directional wipe, ranking each cell by its position
/// along one axis. Predictable and intuitive: a tall dragon paints from the top, a
/// wide serpent from the left, and nothing shows until the wipe reaches it. This is
/// the [`Loader`](crate::Loader) default.
#[derive(Clone, Copy, Debug)]
pub struct Directional(pub Direction);

impl Default for Directional {
    /// `Auto`: top to bottom unless the art reads much wider than it is tall.
    fn default() -> Self {
        Directional(Direction::Auto)
    }
}

impl Directional {
    /// Left to right, or right to left under a right-to-left locale (read from
    /// `LC_ALL` or `LANG`), so the wipe follows the reader's eye.
    pub fn reading() -> Self {
        let rtl = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .map(|l| {
                let l = l.to_ascii_lowercase();
                ["ar", "he", "fa", "ur"].iter().any(|p| l.starts_with(p))
            })
            .unwrap_or(false);
        Directional(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        })
    }
}

impl Ordering for Directional {
    fn rank(&self, art: &Art) -> RankMap {
        let (w, h) = (art.width(), art.height());
        // Terminal cells are about twice as tall as they are wide, so art with
        // more columns than rows can still read as a tall image. Only wipe
        // sideways when it is genuinely wide, more than twice as many columns as
        // rows; otherwise paint top to bottom, which is the intuitive read.
        let dir = match self.0 {
            Direction::Auto if w as u32 > 2 * h as u32 => Direction::LeftToRight,
            Direction::Auto => Direction::TopToBottom,
            other => other,
        };
        let dx = w.saturating_sub(1).max(1) as f32;
        let dy = h.saturating_sub(1).max(1) as f32;
        let mut map = RankMap::new(w, h);
        for cell in art.ink_cells() {
            let rank = match dir {
                Direction::BottomToTop => (h - 1 - cell.y) as f32 / dy,
                Direction::LeftToRight => cell.x as f32 / dx,
                Direction::RightToLeft => (w - 1 - cell.x) as f32 / dx,
                _ => cell.y as f32 / dy, // TopToBottom
            };
            map.set(cell.x, cell.y, rank);
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Geodesic, trace the spine and reveal along it.
// ---------------------------------------------------------------------------

/// Reveal along the "spine" of the art.
///
/// The ink forms a graph under 8-connectivity. We take its **largest connected
/// component** (so a stray fleck can never hijack the reveal), find the two ends
/// of that component's longest geodesic, a double breadth-first sweep, the
/// standard graph-diameter trick, and rank each cell by geodesic distance from
/// the chosen start, normalised to `0..=1`. A serpent therefore paints from one
/// tip to the other along its body, around every coil, with no per-art tuning.
///
/// Hand-drawn ASCII is usually many separate strokes, not one connected line.
/// When the ink is fragmented the spine **bridges small gaps** so the whole body
/// still traces as one path, head to tail; when it is already mostly connected it
/// is traced strictly, with no shortcuts. The switch is automatic (see
/// [`Spine::solve`] and [`Geodesic::bridge`]).
///
/// Detached ink (shading, flecks, a signature) does **not** dump at the end.
/// Every cell inherits the rank of the nearest spine cell, a geodesic Voronoi
/// computed by a multi-source flood, so detail reveals in step with the body it
/// sits beside. Both behaviours fall out of one metric; neither is a special
/// case, so imperfect hand-drawn art still reveals gracefully.
#[derive(Clone, Copy, Debug)]
pub struct Geodesic {
    /// Which tip of the spine the reveal begins from.
    pub start: StartHint,
    /// The largest gap, in blank cells, the spine may step across. Bridging only
    /// engages when the art is actually fragmented (see [`Spine::solve`]), so it
    /// stitches the separate strokes of hand-drawn ASCII into one body without ever
    /// adding shortcuts to art that was already connected. `0` disables it.
    pub bridge: u16,
}

impl Default for Geodesic {
    /// Start at the top-left tip and bridge single-cell gaps when the art is
    /// fragmented, which is what most hand-drawn ASCII needs.
    fn default() -> Self {
        Geodesic {
            start: StartHint::default(),
            bridge: 1,
        }
    }
}

/// Which end of the spine the [`Geodesic`] reveal starts at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StartHint {
    /// The tip nearest the top-left. Deterministic and reads like text. (default)
    #[default]
    TopLeft,
    /// The tip nearest the bottom of the canvas.
    Bottom,
    /// Whichever diameter endpoint the sweep happens to find, purely topological.
    Topological,
}

/// Diagnostics describing how well a piece of art suits geodesic reveal.
///
/// A low `connected_cells / ink_cells` ratio means the art is fragmented and
/// will lean on the Voronoi inheritance rather than the spine itself.
#[derive(Clone, Copy, Debug)]
pub struct GeodesicReport {
    pub ink_cells: usize,
    /// Size of the largest connected component (the spine's component).
    pub connected_cells: usize,
    /// Length of the spine in cells (the component's graph diameter).
    pub spine_length: u32,
}

impl Geodesic {
    /// Inspect the art without building a full rank map.
    pub fn diagnose(&self, art: &Art) -> GeodesicReport {
        match Spine::solve(art, self.start, self.bridge) {
            Some(spine) => GeodesicReport {
                ink_cells: art.ink_count(),
                connected_cells: spine.dist.iter().filter(|d| d.is_some()).count(),
                spine_length: spine.diameter,
            },
            None => GeodesicReport {
                ink_cells: 0,
                connected_cells: 0,
                spine_length: 0,
            },
        }
    }
}

impl Ordering for Geodesic {
    fn rank(&self, art: &Art) -> RankMap {
        let w = art.width();
        let h = art.height();
        let mut map = RankMap::new(w, h);

        let Some(spine) = Spine::solve(art, self.start, self.bridge) else {
            return map; // no ink
        };

        let diameter = spine.diameter as f32;
        let norm = |d: u32| {
            if diameter > 0.0 {
                d as f32 / diameter
            } else {
                0.0
            }
        };

        // Multi-source flood over the *whole grid*, seeded with the spine's
        // ranks. Every cell, including detached islands, inherits the rank of
        // its nearest spine cell (a geodesic Voronoi). Detail thus reveals in
        // step with the body part it sits beside, rather than all at the end.
        let cells = w as usize * h as usize;
        let mut inherited: Vec<Option<f32>> = vec![None; cells];
        let mut queue = VecDeque::new();
        for (i, dist) in spine.dist.iter().enumerate() {
            if let Some(d) = dist {
                inherited[i] = Some(norm(*d));
                queue.push_back(i);
            }
        }
        while let Some(cur) = queue.pop_front() {
            let rank = inherited[cur];
            for ni in neighbours(cur, w, h) {
                if inherited[ni].is_none() {
                    inherited[ni] = rank;
                    queue.push_back(ni);
                }
            }
        }

        for cell in art.ink_cells() {
            let rank = inherited[art.index(cell.x, cell.y)].unwrap_or(0.0);
            map.set(cell.x, cell.y, rank);
        }
        map
    }
}

/// If the largest strictly 8-connected component covers at least this fraction of
/// the ink, the art is treated as already whole and traced without bridging.
const STRICT_CONNECTED_MIN: f32 = 0.6;

/// The traced spine of the art's largest connected component.
struct Spine {
    /// Geodesic distance from the chosen start within the largest component;
    /// `None` for every cell outside it.
    dist: Vec<Option<u32>>,
    /// The component's diameter (maximum geodesic distance).
    diameter: u32,
}

impl Spine {
    fn solve(art: &Art, hint: StartHint, bridge: u16) -> Option<Self> {
        // If the art is already mostly one 8-connected piece, trace it strictly;
        // only stitch gaps when it is genuinely fragmented. Bridging then fixes
        // hand-drawn art split into strokes without adding shortcuts across art
        // that was already whole (which would shorten the spine and cut corners).
        let strict_seed = largest_component_seed(art, 0)?;
        let strict_size = bfs(art, strict_seed, 0)
            .0
            .iter()
            .filter(|d| d.is_some())
            .count();
        let bridge = if (strict_size as f32) < STRICT_CONNECTED_MIN * art.ink_count().max(1) as f32
        {
            bridge
        } else {
            0
        };

        // Double sweep → the two ends (A, B) of the component's longest geodesic.
        let seed = if bridge == 0 {
            strict_seed
        } else {
            largest_component_seed(art, bridge)?
        };
        let (_, far_a) = bfs(art, seed, bridge);
        let (dist_a, far_b) = bfs(art, far_a, bridge);
        let (dist_b, _) = bfs(art, far_b, bridge);

        // Pick which endpoint to start from.
        let w = art.width() as usize;
        let coord = |i: usize| ((i % w) as u16, (i / w) as u16);
        let (ax, ay) = coord(far_a);
        let (bx, by) = coord(far_b);
        let start_is_a = match hint {
            StartHint::Topological => true,
            StartHint::TopLeft => (ay, ax) <= (by, bx),
            StartHint::Bottom => ay >= by,
        };

        let dist = if start_is_a { dist_a } else { dist_b };
        let diameter = dist.iter().flatten().copied().max().unwrap_or(0);
        Some(Spine { dist, diameter })
    }
}

// ---------------------------------------------------------------------------
// Internal graph helpers (8-connectivity).
// ---------------------------------------------------------------------------

/// The in-bounds 8-neighbours of a flat grid index.
fn neighbours(index: usize, w: u16, h: u16) -> impl Iterator<Item = usize> {
    let (w, h) = (w as i32, h as i32);
    let (cx, cy) = ((index as i32 % w), (index as i32 / w));
    (-1..=1)
        .flat_map(move |dy| (-1..=1).map(move |dx| (dx, dy)))
        .filter_map(move |(dx, dy)| {
            if dx == 0 && dy == 0 {
                return None;
            }
            let (nx, ny) = (cx + dx, cy + dy);
            (nx >= 0 && ny >= 0 && nx < w && ny < h).then_some((ny * w + nx) as usize)
        })
}

/// A seed cell in the largest ink component (`None` if no ink). With `bridge > 0`
/// the component spans gaps of that many blank cells.
fn largest_component_seed(art: &Art, bridge: u16) -> Option<usize> {
    let (w, h) = (art.width(), art.height());
    let mut visited = vec![false; w as usize * h as usize];
    let mut queue = VecDeque::new();
    let mut best: Option<(usize, usize)> = None; // (size, seed)

    for cell in art.ink_cells() {
        let seed = art.index(cell.x, cell.y);
        if visited[seed] {
            continue;
        }
        let mut size = 0usize;
        visited[seed] = true;
        queue.push_back(seed);
        while let Some(cur) = queue.pop_front() {
            size += 1;
            for ni in bridged_neighbours(art, cur, bridge) {
                if !visited[ni] {
                    visited[ni] = true;
                    queue.push_back(ni);
                }
            }
        }
        if best.map_or(true, |(best_size, _)| size > best_size) {
            best = Some((size, seed));
        }
    }
    best.map(|(_, seed)| seed)
}

/// BFS from `source` over ink cells, stepping across gaps of up to `bridge` blank
/// cells. Returns the distance to every cell (`None` where unreachable) and the
/// farthest reachable cell.
fn bfs(art: &Art, source: usize, bridge: u16) -> (Vec<Option<u32>>, usize) {
    let (w, h) = (art.width(), art.height());
    let mut dist = vec![None; w as usize * h as usize];
    let mut queue = VecDeque::new();

    dist[source] = Some(0);
    queue.push_back(source);
    let (mut farthest, mut far_d) = (source, 0u32);

    while let Some(cur) = queue.pop_front() {
        let d = dist[cur].unwrap();
        if d > far_d {
            far_d = d;
            farthest = cur;
        }
        for ni in bridged_neighbours(art, cur, bridge) {
            if dist[ni].is_none() {
                dist[ni] = Some(d + 1);
                queue.push_back(ni);
            }
        }
    }
    (dist, farthest)
}

/// Ink cells within Chebyshev distance `bridge + 1` of `index`, so `bridge = 0`
/// is plain 8-connectivity and larger values let the spine step across small gaps
/// between the separate strokes of hand-drawn art.
fn bridged_neighbours(art: &Art, index: usize, bridge: u16) -> Vec<usize> {
    let (w, h) = (art.width() as i32, art.height() as i32);
    let r = bridge as i32 + 1;
    let (cx, cy) = (index as i32 % w, index as i32 / w);
    let mut out = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (cx + dx, cy + dy);
            if nx >= 0 && ny >= 0 && nx < w && ny < h {
                let ni = (ny * w + nx) as usize;
                if is_ink_index(art, ni) {
                    out.push(ni);
                }
            }
        }
    }
    out
}

#[inline]
fn is_ink_index(art: &Art, index: usize) -> bool {
    let w = art.width() as usize;
    art.is_ink((index % w) as u16, (index / w) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight horizontal stroke must reveal strictly along its length, i.e.
    /// ranks increase monotonically (in one direction) and reach 1.0.
    #[test]
    fn straight_line_reveals_along_itself() {
        let art = Art::parse("=========");
        let ranks = Geodesic::default().rank(&art);
        let row: Vec<f32> = (0..art.width())
            .map(|x| ranks.rank_at(x, 0).unwrap())
            .collect();
        let increasing = row.windows(2).all(|w| w[0] <= w[1]);
        let decreasing = row.windows(2).all(|w| w[0] >= w[1]);
        assert!(
            increasing || decreasing,
            "spine reveal was not monotone: {row:?}"
        );
        assert!((row.iter().cloned().fold(0.0_f32, f32::max) - 1.0).abs() < 1e-6);
    }

    /// A lone fleck at the top-left must not become the spine; the long bar does.
    #[test]
    fn spine_traces_largest_component() {
        let art = Art::parse(".\n\n   ========");
        let report = Geodesic::default().diagnose(&art);
        assert_eq!(report.ink_cells, 9);
        assert_eq!(report.connected_cells, 8); // the bar, not the 1-cell fleck
    }

    /// Islands inherit the rank of the nearest spine tip: an island by the start
    /// reveals early, one by the finish reveals late, not both dumped at the end.
    #[test]
    fn islands_inherit_nearest_spine_rank() {
        let art = Art::parse(".  ======  .");
        let ranks = Geodesic::default().rank(&art);
        let left = ranks.rank_at(0, 0).unwrap();
        let right = ranks.rank_at(11, 0).unwrap();
        assert!(left < right, "left {left} should precede right {right}");
        assert!(left < 0.25 && right > 0.75, "left={left} right={right}");
    }

    #[test]
    fn diagnose_counts_connectivity() {
        let report = Geodesic::default().diagnose(&Art::parse("==========    ."));
        assert_eq!(report.ink_cells, 11);
        assert_eq!(report.connected_cells, 10); // the bar; the '.' is an island
    }

    /// Fragmented art (two strokes one blank cell apart) reveals as one body: the
    /// default bridges the gap, while `bridge: 0` keeps the strokes separate.
    #[test]
    fn bridges_small_gaps_when_fragmented() {
        let art = Art::parse("== ==");
        let strict = Geodesic {
            start: StartHint::TopLeft,
            bridge: 0,
        };
        assert_eq!(strict.diagnose(&art).connected_cells, 2);
        assert_eq!(Geodesic::default().diagnose(&art).connected_cells, 4);
    }

    /// Already-connected art must not be bridged: shortcuts would cut across the
    /// body and shrink the spine, so a clean stroke keeps its full-length trace.
    #[test]
    fn connected_art_is_not_bridged() {
        // A zigzag whose passes sit two rows apart; bridging would short-circuit
        // it, but since it is one strict component the spine stays long.
        let art = Art::parse("####\n   #\n####\n#\n####");
        let report = Geodesic::default().diagnose(&art);
        assert_eq!(report.connected_cells, report.ink_cells);
        assert!(
            report.spine_length >= 9,
            "spine was {}",
            report.spine_length
        );
    }

    /// `Auto` weights for terminal cells being about twice as tall as wide: art
    /// that is wider than tall in cells but reads tall still paints top to bottom;
    /// only genuinely wide art wipes sideways.
    #[test]
    fn directional_auto_accounts_for_cell_aspect() {
        // 5 wide by 4 tall: more columns than rows, yet reads tall -> top to bottom.
        let tall = Art::parse("#####\n#####\n#####\n#####");
        let r = Directional(Direction::Auto).rank(&tall);
        assert!(
            r.rank_at(0, 0).unwrap() < r.rank_at(0, 3).unwrap(),
            "top first"
        );
        assert_eq!(
            r.rank_at(0, 0),
            r.rank_at(4, 0),
            "same row reveals together"
        );

        // 10 wide by 2 tall: genuinely wide -> left to right.
        let wide = Art::parse("##########\n##########");
        let rw = Directional(Direction::Auto).rank(&wide);
        assert!(
            rw.rank_at(0, 0).unwrap() < rw.rank_at(9, 0).unwrap(),
            "left first"
        );
        assert_eq!(
            rw.rank_at(0, 0),
            rw.rank_at(0, 1),
            "same column reveals together"
        );
    }
}
