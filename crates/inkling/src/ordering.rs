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

/// Reveal by tracing the art's skeleton.
///
/// The ink is first thinned to a one-cell-wide **skeleton** (Zhang-Suen), the
/// centerline a pen would draw. Each connected piece of that skeleton is traced tip
/// to tip by geodesic distance, a double breadth-first sweep finding the two ends of
/// its longest path, and the pieces are ordered along the art's dominant axis. So a
/// snake paints head to tail, a filled dragon paints down its spine, and a
/// multi-letter logo paints letter by letter in reading order, with no per-art tuning.
///
/// Hand-drawn ASCII is usually many separate strokes, not one connected line, so the
/// trace **bridges small gaps** to stitch a broken stroke into one piece; art that is
/// already whole is traced strictly, with no shortcuts (see [`Geodesic::bridge`]).
///
/// The flesh around the skeleton inherits the value of its nearest centerline cell, a
/// Voronoi flood, so detail reveals in step with the part of the spine it hangs from;
/// where the skeleton is a mere dot, as in a solid blob, the fill radiates out from
/// the middle. Finally the values are rank-transformed to evenly spaced ranks, so the
/// reveal keeps its order yet tracks the progress bar with no dead zone at either end.
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
        let mask = ink_mask(art);
        match Spine::solve(&mask, art.width(), art.height(), self.start, self.bridge) {
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
        let (w, h) = (art.width(), art.height());
        let mut map = RankMap::new(w, h);
        if art.ink_count() == 0 {
            return map;
        }

        // Thin the ink to its skeleton, then give every skeleton cell a reveal
        // value: each piece traced tip to tip, the pieces in reading order.
        let skel = skeletonize(art);
        let value = skeleton_values(&skel, w, h, self.start, self.bridge);

        // Voronoi flood: every cell takes the value of its nearest skeleton cell and
        // remembers how far it sits from that centerline. The flesh thus reveals in
        // step with the part of the spine it hangs from; and where the skeleton is a
        // mere dot (a solid blob) the distance term spreads the fill out from the
        // middle rather than all at once.
        let mut val = value.clone();
        let mut depth = vec![0u32; val.len()];
        let mut queue: VecDeque<usize> = (0..val.len()).filter(|&i| !val[i].is_nan()).collect();
        while let Some(cur) = queue.pop_front() {
            for ni in neighbours(cur, w, h) {
                if val[ni].is_nan() {
                    val[ni] = val[cur];
                    depth[ni] = depth[cur] + 1;
                    queue.push_back(ni);
                }
            }
        }

        // Rank-transform: order the ink by (centerline value, distance from it),
        // then assign evenly spaced ranks so the reveal keeps that order but tracks
        // the progress bar, with no dead zone at either end.
        let mut order: Vec<(u16, u16, f32, u32)> = art
            .ink_cells()
            .map(|c| {
                let i = art.index(c.x, c.y);
                (c.x, c.y, val[i], depth[i])
            })
            .collect();
        order.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.3.cmp(&b.3)));
        let denom = order.len().saturating_sub(1).max(1) as f32;
        for (i, &(x, y, _, _)) in order.iter().enumerate() {
            map.set(x, y, i as f32 / denom);
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
    fn solve(mask: &[bool], w: u16, h: u16, hint: StartHint, bridge: u16) -> Option<Self> {
        // If the mask is already mostly one 8-connected piece, trace it strictly;
        // only stitch gaps when it is genuinely fragmented. Bridging then fixes
        // hand-drawn art split into strokes without adding shortcuts across art
        // that was already whole (which would shorten the spine and cut corners).
        let count = mask.iter().filter(|&&m| m).count();
        let strict_seed = largest_component_seed(mask, w, h, 0)?;
        let strict_size = bfs(mask, w, h, strict_seed, 0)
            .0
            .iter()
            .filter(|d| d.is_some())
            .count();
        let bridge = if (strict_size as f32) < STRICT_CONNECTED_MIN * count.max(1) as f32 {
            bridge
        } else {
            0
        };

        // Double sweep → the two ends (A, B) of the component's longest geodesic.
        let seed = if bridge == 0 {
            strict_seed
        } else {
            largest_component_seed(mask, w, h, bridge)?
        };
        let (_, far_a) = bfs(mask, w, h, seed, bridge);
        let (dist_a, far_b) = bfs(mask, w, h, far_a, bridge);
        let (dist_b, _) = bfs(mask, w, h, far_b, bridge);

        // Pick which endpoint to start from.
        let coord = |i: usize| ((i % w as usize) as u16, (i / w as usize) as u16);
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

/// A boolean grid: `true` where the art has ink.
fn ink_mask(art: &Art) -> Vec<bool> {
    let (w, h) = (art.width() as usize, art.height() as usize);
    (0..w * h)
        .map(|i| art.is_ink((i % w) as u16, (i / w) as u16))
        .collect()
}

/// A seed cell in the largest component of `mask` (`None` if empty). With
/// `bridge > 0` a component spans gaps of that many blank cells.
fn largest_component_seed(mask: &[bool], w: u16, h: u16, bridge: u16) -> Option<usize> {
    let mut visited = vec![false; mask.len()];
    let mut queue = VecDeque::new();
    let mut best: Option<(usize, usize)> = None; // (size, seed)

    for seed in 0..mask.len() {
        if !mask[seed] || visited[seed] {
            continue;
        }
        let mut size = 0usize;
        visited[seed] = true;
        queue.push_back(seed);
        while let Some(cur) = queue.pop_front() {
            size += 1;
            for ni in bridged_neighbours(mask, w, h, cur, bridge) {
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

/// BFS from `source` over `mask`, stepping across gaps of up to `bridge` blank
/// cells. Returns the distance to every cell (`None` where unreachable) and the
/// farthest reachable cell.
fn bfs(mask: &[bool], w: u16, h: u16, source: usize, bridge: u16) -> (Vec<Option<u32>>, usize) {
    let mut dist = vec![None; mask.len()];
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
        for ni in bridged_neighbours(mask, w, h, cur, bridge) {
            if dist[ni].is_none() {
                dist[ni] = Some(d + 1);
                queue.push_back(ni);
            }
        }
    }
    (dist, farthest)
}

/// Member cells within Chebyshev distance `bridge + 1` of `index`, so `bridge = 0`
/// is plain 8-connectivity and larger values let a trace step across small gaps.
fn bridged_neighbours(mask: &[bool], w: u16, h: u16, index: usize, bridge: u16) -> Vec<usize> {
    let (wi, hi) = (w as i32, h as i32);
    let r = bridge as i32 + 1;
    let (cx, cy) = (index as i32 % wi, index as i32 / wi);
    let mut out = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx == 0 && dy == 0 {
                continue;
            }
            let (nx, ny) = (cx + dx, cy + dy);
            if nx >= 0 && ny >= 0 && nx < wi && ny < hi {
                let ni = (ny * wi + nx) as usize;
                if mask[ni] {
                    out.push(ni);
                }
            }
        }
    }
    out
}

/// Zhang-Suen thinning: reduce the ink to a one-cell-wide skeleton, its medial
/// axis. A solid shape collapses to the centerline a pen would trace; a shape that
/// is already a line is left unchanged.
fn skeletonize(art: &Art) -> Vec<bool> {
    let (w, h) = (art.width() as i32, art.height() as i32);
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    let mut g = ink_mask(art);
    let val = |g: &[bool], x: i32, y: i32| -> u8 {
        (x >= 0 && y >= 0 && x < w && y < h && g[idx(x, y)]) as u8
    };
    loop {
        let mut removed = false;
        for step in 0..2 {
            let mut marks = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    if !g[idx(x, y)] {
                        continue;
                    }
                    // p2..p9, clockwise from north.
                    let p = [
                        val(&g, x, y - 1),
                        val(&g, x + 1, y - 1),
                        val(&g, x + 1, y),
                        val(&g, x + 1, y + 1),
                        val(&g, x, y + 1),
                        val(&g, x - 1, y + 1),
                        val(&g, x - 1, y),
                        val(&g, x - 1, y - 1),
                    ];
                    let b: u8 = p.iter().sum();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let a = (0..8).filter(|&i| p[i] == 0 && p[(i + 1) % 8] == 1).count();
                    if a != 1 {
                        continue;
                    }
                    let (c1, c2) = if step == 0 {
                        (p[0] * p[2] * p[4], p[2] * p[4] * p[6])
                    } else {
                        (p[0] * p[2] * p[6], p[0] * p[4] * p[6])
                    };
                    if c1 == 0 && c2 == 0 {
                        marks.push(idx(x, y));
                    }
                }
            }
            if !marks.is_empty() {
                removed = true;
                for i in marks {
                    g[i] = false;
                }
            }
        }
        if !removed {
            break;
        }
    }
    g
}

/// A reveal value for every skeleton cell. Each connected piece of the skeleton is
/// traced tip to tip (geodesic distance), and the pieces are ordered along the
/// art's dominant axis, so a multi-letter logo paints letter by letter in reading
/// order while a single shape just traces its centerline. `NaN` off the skeleton.
fn skeleton_values(skel: &[bool], w: u16, h: u16, hint: StartHint, bridge: u16) -> Vec<f32> {
    let mut value = vec![f32::NAN; skel.len()];
    let count = skel.iter().filter(|&&m| m).count();
    if count == 0 {
        return value;
    }

    // Adaptive bridge: stitch a fragmented skeleton, but never add shortcuts to one
    // that is already whole (which would cut corners on the trace).
    let bridge = match largest_component_seed(skel, w, h, 0) {
        Some(seed)
            if bfs(skel, w, h, seed, 0)
                .0
                .iter()
                .filter(|d| d.is_some())
                .count() as f32
                >= STRICT_CONNECTED_MIN * count as f32 =>
        {
            0
        }
        _ => bridge,
    };

    // Label connected components.
    let mut comp_id = vec![usize::MAX; skel.len()];
    let mut comps: Vec<Vec<usize>> = Vec::new();
    for i in 0..skel.len() {
        if !skel[i] || comp_id[i] != usize::MAX {
            continue;
        }
        let id = comps.len();
        let mut cells = Vec::new();
        let mut queue = VecDeque::new();
        comp_id[i] = id;
        queue.push_back(i);
        while let Some(cur) = queue.pop_front() {
            cells.push(cur);
            for ni in bridged_neighbours(skel, w, h, cur, bridge) {
                if comp_id[ni] == usize::MAX {
                    comp_id[ni] = id;
                    queue.push_back(ni);
                }
            }
        }
        comps.push(cells);
    }

    let horizontal = w as u32 > 2 * h as u32;
    let axis = |i: usize| -> u16 {
        if horizontal {
            (i % w as usize) as u16
        } else {
            (i / w as usize) as u16
        }
    };
    let coord = |i: usize| ((i % w as usize) as u16, (i / w as usize) as u16);

    // Trace each piece, and note its leading edge along the axis for ordering.
    let mut pieces: Vec<(u16, Vec<(usize, f32)>)> = comps
        .iter()
        .map(|comp| {
            let (_, far_a) = bfs(skel, w, h, comp[0], bridge);
            let (dist_a, far_b) = bfs(skel, w, h, far_a, bridge);
            let (dist_b, _) = bfs(skel, w, h, far_b, bridge);
            let (ax, ay) = coord(far_a);
            let (bx, by) = coord(far_b);
            let start_is_a = match hint {
                StartHint::Topological => true,
                StartHint::TopLeft => (ay, ax) <= (by, bx),
                StartHint::Bottom => ay >= by,
            };
            let dist = if start_is_a { dist_a } else { dist_b };
            let diameter = dist.iter().flatten().copied().max().unwrap_or(0).max(1) as f32;
            let lead = comp.iter().map(|&c| axis(c)).min().unwrap_or(0);
            let within = comp
                .iter()
                .map(|&c| (c, dist[c].map_or(0.0, |d| d as f32 / diameter)))
                .collect();
            (lead, within)
        })
        .collect();

    pieces.sort_by_key(|(lead, _)| *lead);
    for (piece, (_, within)) in pieces.iter().enumerate() {
        for &(cell, w) in within {
            value[cell] = piece as f32 + w;
        }
    }
    value
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

    /// A solid block has no real structure, but the reveal must still use the whole
    /// bar (no dead zone at either end) rather than dump everything at once.
    #[test]
    fn solid_block_reveals_across_the_whole_bar() {
        let art = Art::parse(&"########\n".repeat(8));
        let r = Geodesic::default().rank(&art);
        let ranks: Vec<f32> = (0..8)
            .flat_map(|y| (0..8u16).map(move |x| (x, y)))
            .map(|(x, y)| r.rank_at(x, y).unwrap())
            .collect();
        let lo = ranks.iter().cloned().fold(f32::MAX, f32::min);
        let hi = ranks.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            lo < 0.02 && hi > 0.98,
            "block did not use the whole bar: {lo}..{hi}"
        );
    }

    /// Separate pieces (the strokes of a logo) reveal one after another in reading
    /// order, each traced, rather than all at once or out of order.
    #[test]
    fn separate_pieces_reveal_in_reading_order() {
        let art = Art::parse("##        ##\n##        ##\n##        ##");
        let r = Geodesic::default().rank(&art);
        let left = r.rank_at(0, 1).unwrap();
        let right = r.rank_at(11, 1).unwrap();
        assert!(
            left < right,
            "left piece {left} should precede right {right}"
        );
        assert!(
            left < 0.5 && right > 0.5,
            "pieces out of order: {left} {right}"
        );
    }

    /// A thin line keeps a pure spine trace: the directional blend stays out of the
    /// way, so the two ends are the first and last cells revealed.
    #[test]
    fn thin_line_stays_a_trace() {
        let art = Art::parse("==============");
        let r = Geodesic::default().rank(&art);
        let row: Vec<f32> = (0..art.width()).map(|x| r.rank_at(x, 0).unwrap()).collect();
        let lo = row.iter().cloned().fold(f32::MAX, f32::min);
        let hi = row.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            lo < 0.01 && hi > 0.99,
            "line did not trace end to end: {row:?}"
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
