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

/// Evenly spaced ranks over `count` cells, so the first is `0.0` and the last
/// `1.0` with no dead zone at either end.
#[inline]
fn even_step(count: usize) -> f32 {
    count.saturating_sub(1).max(1) as f32
}

// ---------------------------------------------------------------------------
// Scanline, the trivial geometric baseline.
// ---------------------------------------------------------------------------

/// Reveal in reading order: top-to-bottom, left-to-right.
///
/// The dullest possible ordering, included as a baseline and as a reference
/// implementation of the [`Ordering`] trait.
#[derive(Clone, Copy, Debug, Default)]
pub struct Scanline;

impl Ordering for Scanline {
    fn rank(&self, art: &Art) -> RankMap {
        let mut map = RankMap::new(art.width(), art.height());
        let denom = even_step(art.ink_count());
        for (i, cell) in art.ink_cells().enumerate() {
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
    /// Left to right: the wipe follows a left-to-right reader's eye.
    pub fn ltr() -> Self {
        Directional(Direction::LeftToRight)
    }

    /// Right to left, for Arabic, Hebrew, Persian, and Urdu layouts.
    pub fn rtl() -> Self {
        Directional(Direction::RightToLeft)
    }

    /// Wipe along the reading direction of the user's locale, so it follows the
    /// reader's eye. Falls back to [`ltr`](Self::ltr) when the locale cannot be
    /// determined.
    ///
    /// The locale comes from `LC_ALL` or `LANG` where those are set, and from the
    /// user's default locale on Windows, where they usually are not. Call
    /// [`ltr`](Self::ltr) or [`rtl`](Self::rtl) directly when your program already
    /// knows its own text direction; that is always more reliable than sniffing.
    pub fn reading() -> Self {
        if locale_is_rtl() {
            Self::rtl()
        } else {
            Self::ltr()
        }
    }
}

/// Language subtags written right to left.
const RTL_LANGS: [&str; 4] = ["ar", "he", "fa", "ur"];

fn locale_is_rtl() -> bool {
    let tagged = |l: &str| {
        let l = l.to_ascii_lowercase();
        RTL_LANGS.iter().any(|p| l.starts_with(p))
    };
    if let Ok(l) = std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")) {
        return tagged(&l);
    }
    system_locale().map(|l| tagged(&l)).unwrap_or(false)
}

/// The user's default locale name, where the platform exposes one outside the
/// environment. Windows does not set `LANG`, so without this every Windows user
/// would be treated as left-to-right regardless of how their system is set up.
#[cfg(windows)]
fn system_locale() -> Option<String> {
    // Declared directly rather than pulled from a crate: the core carries no
    // dependencies, and this is one documented call into kernel32, which std
    // already links.
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, capacity: i32) -> i32;
    }

    // LOCALE_NAME_MAX_LENGTH is 85 wide chars.
    let mut buf = [0u16; 85];
    // SAFETY: the buffer outlives the call and its true capacity is passed.
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 1 {
        return None; // 0 on failure; 1 is just the trailing NUL
    }
    String::from_utf16(&buf[..len as usize - 1]).ok()
}

#[cfg(not(windows))]
fn system_locale() -> Option<String> {
    None
}

impl Ordering for Directional {
    fn rank(&self, art: &Art) -> RankMap {
        let (w, h) = (art.width(), art.height());
        // Terminal cells are about twice as tall as they are wide, so art with
        // more columns than rows can still read as a tall image. Only wipe
        // sideways when it is genuinely wide, more than twice as many columns as
        // rows; otherwise paint top to bottom, which is the intuitive read.
        let dir = match self.0 {
            Direction::Auto if is_wide(w, h) => Direction::LeftToRight,
            Direction::Auto => Direction::TopToBottom,
            other => other,
        };
        let dx = even_step(w as usize);
        let dy = even_step(h as usize);
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

/// True when the art reads as wide rather than tall, correcting for terminal
/// cells being roughly twice as tall as they are wide.
#[inline]
fn is_wide(w: u16, h: u16) -> bool {
    w as u32 > 2 * h as u32
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
    /// engages when the art is actually fragmented (see [`STRICT_CONNECTED_MIN`]),
    /// so it stitches the separate strokes of hand-drawn ASCII into one body
    /// without ever adding shortcuts to art that was already connected. `0`
    /// disables it.
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
/// Every field describes the structure the reveal actually follows. A low
/// `connected_cells / ink_cells` ratio means the ink is fragmented and the reveal
/// leans on the Voronoi inheritance; a `pieces` count above 1 means the skeleton
/// broke into strokes that are painted one after another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeodesicReport {
    /// Total ink cells in the art.
    pub ink_cells: usize,
    /// Size of the largest strictly 8-connected component of the *ink*.
    pub connected_cells: usize,
    /// Cells remaining after thinning, i.e. the length of the drawn centerline.
    pub skeleton_cells: usize,
    /// Separate pieces the skeleton breaks into once bridging has been applied.
    /// Each is traced in turn, in reading order along the dominant axis.
    pub pieces: usize,
    /// Longest geodesic through the largest skeleton piece, in cells: the spine
    /// the reveal actually traces.
    pub spine_length: u32,
}

impl Geodesic {
    /// Inspect the art without building a full rank map.
    pub fn diagnose(&self, art: &Art) -> GeodesicReport {
        let (w, h) = (art.width(), art.height());
        let ink = ink_mask(art);
        let ink_cells = ink.iter().filter(|&&m| m).count();
        if ink_cells == 0 {
            return GeodesicReport {
                ink_cells: 0,
                connected_cells: 0,
                skeleton_cells: 0,
                pieces: 0,
                spine_length: 0,
            };
        }

        let connected_cells = largest_component(&ink, w, h, 0).map_or(0, |(size, _)| size);
        let skel = skeletonize(art);
        let skeleton_cells = skel.iter().filter(|&&m| m).count();
        let bridge = adaptive_bridge(&skel, w, h, self.bridge);

        GeodesicReport {
            ink_cells,
            connected_cells,
            skeleton_cells,
            pieces: components(&skel, w, h, bridge).len(),
            spine_length: spine(&skel, w, h, self.start, self.bridge)
                .map_or(0, |trace| trace.diameter),
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
        let mut val = value;
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
        let denom = even_step(order.len());
        for (i, &(x, y, _, _)) in order.iter().enumerate() {
            map.set(x, y, i as f32 / denom);
        }
        map
    }
}

/// If the largest strictly 8-connected component covers at least this fraction of
/// the mask, it is treated as already whole and traced without bridging.
pub const STRICT_CONNECTED_MIN: f32 = 0.6;

// ---------------------------------------------------------------------------
// Tracing. One implementation, shared by the whole-art spine and the per-piece
// walk inside `skeleton_values`, so the two can never disagree about what a
// "trace" means.
// ---------------------------------------------------------------------------

/// A traced piece: geodesic distance from the chosen start tip to every cell it
/// reaches, and the piece's diameter.
struct Trace {
    /// Distance from the start; `None` for every cell outside the piece.
    dist: Vec<Option<u32>>,
    /// The piece's diameter (its maximum geodesic distance).
    diameter: u32,
}

/// Trace the piece containing `seed` tip to tip: a double breadth-first sweep
/// finds the two ends `(a, b)` of its longest geodesic, then `hint` picks which
/// end the reveal starts from.
fn trace(mask: &[bool], w: u16, h: u16, seed: usize, hint: StartHint, bridge: u16) -> Trace {
    let (_, far_a) = bfs(mask, w, h, seed, bridge);
    let (dist_a, far_b) = bfs(mask, w, h, far_a, bridge);
    let (dist_b, _) = bfs(mask, w, h, far_b, bridge);

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
    Trace { dist, diameter }
}

/// Trace the largest piece of `mask` tip to tip, bridging only if it is genuinely
/// fragmented. `None` when the mask is empty.
fn spine(mask: &[bool], w: u16, h: u16, hint: StartHint, bridge: u16) -> Option<Trace> {
    let bridge = adaptive_bridge(mask, w, h, bridge);
    let (_, seed) = largest_component(mask, w, h, bridge)?;
    Some(trace(mask, w, h, seed, hint, bridge))
}

/// Bridging engages only when the mask is actually fragmented. Stitching gaps in
/// art that was already whole would add shortcuts straight across the body,
/// shortening the spine and cutting corners on the trace.
fn adaptive_bridge(mask: &[bool], w: u16, h: u16, bridge: u16) -> u16 {
    if bridge == 0 {
        return 0;
    }
    let count = mask.iter().filter(|&&m| m).count();
    match largest_component(mask, w, h, 0) {
        Some((strict, _)) if strict as f32 >= STRICT_CONNECTED_MIN * count.max(1) as f32 => 0,
        _ => bridge,
    }
}

// ---------------------------------------------------------------------------
// Internal graph helpers (8-connectivity).
// ---------------------------------------------------------------------------

/// The in-bounds 8-neighbours of a flat grid index.
fn neighbours(index: usize, w: u16, h: u16) -> impl Iterator<Item = usize> {
    offsets(index, w, h, 0)
}

/// In-bounds neighbours within Chebyshev distance `bridge + 1` of `index`, so
/// `bridge = 0` is plain 8-connectivity. Lazy: this sits in the inner loop of
/// every sweep, and materialising a `Vec` per node expansion was the single
/// hottest allocation in the crate.
fn offsets(index: usize, w: u16, h: u16, bridge: u16) -> impl Iterator<Item = usize> {
    let (wi, hi) = (w as i32, h as i32);
    let r = bridge as i32 + 1;
    let (cx, cy) = (index as i32 % wi.max(1), index as i32 / wi.max(1));
    (-r..=r)
        .flat_map(move |dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter_map(move |(dx, dy)| {
            if dx == 0 && dy == 0 {
                return None;
            }
            let (nx, ny) = (cx + dx, cy + dy);
            (nx >= 0 && ny >= 0 && nx < wi && ny < hi).then_some((ny * wi + nx) as usize)
        })
}

/// Member cells within Chebyshev distance `bridge + 1` of `index`.
#[inline]
fn bridged_neighbours(
    mask: &[bool],
    w: u16,
    h: u16,
    index: usize,
    bridge: u16,
) -> impl Iterator<Item = usize> + '_ {
    offsets(index, w, h, bridge).filter(move |&ni| mask[ni])
}

/// A boolean grid: `true` where the art has ink.
fn ink_mask(art: &Art) -> Vec<bool> {
    let (w, h) = (art.width() as usize, art.height() as usize);
    (0..w * h)
        .map(|i| art.is_ink((i % w.max(1)) as u16, (i / w.max(1)) as u16))
        .collect()
}

/// Every connected component of `mask`, each as its list of cells, in the order
/// their first cell appears. With `bridge > 0` a component spans gaps of that many
/// blank cells.
fn components(mask: &[bool], w: u16, h: u16, bridge: u16) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut queue = VecDeque::new();
    let mut out = Vec::new();

    for seed in 0..mask.len() {
        if !mask[seed] || seen[seed] {
            continue;
        }
        let mut cells = Vec::new();
        seen[seed] = true;
        queue.push_back(seed);
        while let Some(cur) = queue.pop_front() {
            cells.push(cur);
            for ni in bridged_neighbours(mask, w, h, cur, bridge) {
                if !seen[ni] {
                    seen[ni] = true;
                    queue.push_back(ni);
                }
            }
        }
        out.push(cells);
    }
    out
}

/// The size of, and a seed cell in, the largest component of `mask`.
fn largest_component(mask: &[bool], w: u16, h: u16, bridge: u16) -> Option<(usize, usize)> {
    components(mask, w, h, bridge)
        .into_iter()
        .map(|c| (c.len(), c[0]))
        .max_by_key(|&(size, _)| size)
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
/// traced tip to tip, and the pieces are ordered along the art's dominant axis, so
/// a multi-letter logo paints letter by letter in reading order while a single
/// shape just traces its centerline. `NaN` off the skeleton.
fn skeleton_values(skel: &[bool], w: u16, h: u16, hint: StartHint, bridge: u16) -> Vec<f32> {
    let mut value = vec![f32::NAN; skel.len()];
    if !skel.iter().any(|&m| m) {
        return value;
    }

    let bridge = adaptive_bridge(skel, w, h, bridge);
    let horizontal = is_wide(w, h);
    let axis = |i: usize| -> u16 {
        if horizontal {
            (i % w as usize) as u16
        } else {
            (i / w as usize) as u16
        }
    };

    // Trace each piece, and note its leading edge along the axis for ordering.
    let mut pieces: Vec<(u16, Vec<usize>, Trace)> = components(skel, w, h, bridge)
        .into_iter()
        .map(|comp| {
            let lead = comp.iter().map(|&c| axis(c)).min().unwrap_or(0);
            let traced = trace(skel, w, h, comp[0], hint, bridge);
            (lead, comp, traced)
        })
        .collect();

    pieces.sort_by_key(|(lead, _, _)| *lead);
    for (index, (_, comp, traced)) in pieces.iter().enumerate() {
        let span = traced.diameter.max(1) as f32;
        for &cell in comp {
            let within = traced.dist[cell].map_or(0.0, |d| d as f32 / span);
            value[cell] = index as f32 + within;
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

    /// `spine_length` must describe the skeleton the reveal actually traces, not
    /// the raw ink: a thick bar thins to a centerline, and that centerline is what
    /// the trace walks.
    #[test]
    fn diagnose_reports_the_traced_skeleton() {
        let art = Art::parse(&"##########\n".repeat(3));
        let report = Geodesic::default().diagnose(&art);
        assert_eq!(report.ink_cells, 30);
        assert_eq!(report.connected_cells, 30);
        assert!(
            report.skeleton_cells < report.ink_cells,
            "thinning should shrink the ink: {report:?}"
        );
        assert_eq!(report.pieces, 1);
        assert!(
            (report.spine_length as usize) < report.ink_cells,
            "spine must be the centerline, not the ink: {report:?}"
        );
    }

    #[test]
    fn diagnose_counts_pieces() {
        let art = Art::parse("##        ##        ##");
        let report = Geodesic::default().diagnose(&art);
        assert_eq!(report.pieces, 3);
    }

    #[test]
    fn diagnose_of_empty_art_is_all_zero() {
        let report = Geodesic::default().diagnose(&Art::parse("   \n   "));
        assert_eq!(report.ink_cells, 0);
        assert_eq!(report.spine_length, 0);
        assert_eq!(report.pieces, 0);
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
        assert_eq!(strict.diagnose(&art).pieces, 2);
        assert_eq!(Geodesic::default().diagnose(&art).pieces, 1);
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
        assert_eq!(report.pieces, 1);
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

    /// Padding must not steer the `Auto` heuristic. A one-column vertical bar is
    /// tall art however much blank space surrounds it, so it wipes top to bottom
    /// and the two cells never share a rank.
    #[test]
    fn padding_does_not_steer_auto() {
        let padded = Directional(Direction::Auto).rank(&Art::parse("      #\n      #"));
        let bare = Directional(Direction::Auto).rank(&Art::parse("#\n#"));
        assert_eq!(padded.rank_at(0, 0), bare.rank_at(0, 0));
        assert_eq!(padded.rank_at(0, 0), Some(0.0));
        assert_eq!(padded.rank_at(0, 1), Some(1.0));
    }

    #[test]
    fn explicit_direction_beats_locale_sniffing() {
        let art = Art::parse("abcd");
        let ltr = Directional::ltr().rank(&art);
        let rtl = Directional::rtl().rank(&art);
        assert_eq!(ltr.rank_at(0, 0), Some(0.0));
        assert_eq!(rtl.rank_at(3, 0), Some(0.0));
    }

    #[test]
    fn scanline_spans_the_whole_bar() {
        let art = Art::parse("ab\ncd");
        let r = Scanline.rank(&art);
        assert_eq!(r.rank_at(0, 0), Some(0.0));
        assert_eq!(r.rank_at(1, 1), Some(1.0));
    }

    /// Share of the ink visible at `progress`.
    fn revealed_share(art: &Art, ranks: &RankMap, progress: f32) -> f32 {
        let mut seen = 0usize;
        for y in 0..art.height() {
            for x in 0..art.width() {
                if art.is_ink(x, y) && ranks.visible_at(x, y, progress) {
                    seen += 1;
                }
            }
        }
        seen as f32 / art.ink_count().max(1) as f32
    }

    /// A reveal has to read as a progress bar: the share of ink on screen tracks
    /// the reported fraction. Individually valid ranks can still add up to a
    /// reveal that dumps half the picture at the start or stalls at the end, and
    /// on real art (dense in some rows, sparse in others) that is exactly what a
    /// naive rank assignment does. This pins the behaviour on the art that
    /// actually ships, under every ordering.
    #[test]
    fn revealed_share_tracks_progress_on_the_bundled_art() {
        let art = [
            ("dragon", Art::parse(include_str!("../assets/dragon.txt"))),
            ("serpent", Art::parse(include_str!("../assets/serpent.txt"))),
            ("inkling", Art::parse(include_str!("../assets/inkling.txt"))),
        ];
        for (name, art) in &art {
            // Scanline ranks cells one by one, and Geodesic ends in a rank
            // transform, so both track the bar exactly. Directional is a
            // geometric wipe: it ranks by position, knows nothing about where
            // the ink is dense, and on the dragon (a sparse crest over a solid
            // body) it is legitimately behind at the halfway mark. That is the
            // ordering's character, not a defect, but it still must not lurch.
            let maps: [(&str, RankMap, f32); 3] = [
                ("directional", Directional::default().rank(art), 0.2),
                ("geodesic", Geodesic::default().rank(art), 0.02),
                ("scanline", Scanline.rank(art), 0.02),
            ];
            for (ordering, ranks, tolerance) in maps {
                let at = |p| revealed_share(art, &ranks, p);
                assert!(
                    at(0.0) < 0.02,
                    "{name}/{ordering}: {:.0}% of the ink is already showing at zero",
                    at(0.0) * 100.0
                );
                for p in [0.25f32, 0.5, 0.75] {
                    let share = at(p);
                    assert!(
                        (share - p).abs() < tolerance,
                        "{name}/{ordering}: {:.0}% of the ink revealed at {:.0}% progress",
                        share * 100.0,
                        p * 100.0
                    );
                }
                assert!(
                    at(1.0) > 0.999,
                    "{name}/{ordering}: the art never finishes filling"
                );
            }
        }
    }

    #[test]
    fn orderings_tolerate_empty_art() {
        let art = Art::parse("");
        for map in [
            Scanline.rank(&art),
            Directional::default().rank(&art),
            Geodesic::default().rank(&art),
        ] {
            assert_eq!(map.ink_count(), 0);
        }
    }
}
