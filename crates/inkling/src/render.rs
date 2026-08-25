//! Terminal rendering.
//!
//! Two renderers live here and they share one walk. `queue_row` writes a whole
//! row of the reveal, honouring display width, run-length colour, colour depth,
//! and a column budget; the live loader draws every frame through it. [`Reveal`]
//! is the diffing session for frame-by-frame control: it makes the same per-cell
//! decision via [`crate::frame::cell`] but repaints only the cells that moved, in
//! practice the frontier band plus whatever ink just settled.
//!
//! The glowing frontier is not an effect bolted on; it falls out of the model. A
//! cell `feather` rank-units behind `progress` is at the frontier; one further
//! behind has settled. Colour is interpolated across that band, so the bright
//! "head" of the reveal slides along the spine for free.

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::{Print, ResetColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::frame::{self, Paint};
use crate::{art::Art, easing::Easing, guard, rank::RankMap};

/// Number of quantised brightness steps across the frontier. The frontier band
/// repaints as it moves; level `GLOW_LEVELS` is "settled" and paints just once.
const GLOW_LEVELS: u8 = 8;

/// DEC private mode 2026, *synchronized output*. A terminal that understands it
/// buffers everything between begin and end and presents the frame as one atomic
/// update, so a reveal never tears mid-paint; terminals that do not recognise the
/// mode silently ignore both markers, so it is always safe to emit.
pub(crate) const SYNC_BEGIN: &str = "\x1b[?2026h";
pub(crate) const SYNC_END: &str = "\x1b[?2026l";

// ---------------------------------------------------------------------------
// Colour depth
// ---------------------------------------------------------------------------

/// How many colours the output can carry.
///
/// The frontier glow is a gradient, so it is the part of the design that suffers
/// most on a limited terminal. Rather than emit 24-bit escapes everywhere and let
/// the terminal do something arbitrary, the palette is mapped down explicitly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorDepth {
    /// No colour at all. Chosen when `NO_COLOR` is set or `TERM=dumb`.
    Mono,
    /// The 16 ANSI colours.
    Ansi16,
    /// The 256-colour cube. The default when the terminal does not say otherwise.
    #[default]
    Ansi256,
    /// 24-bit colour, the full palette.
    TrueColor,
}

impl ColorDepth {
    /// What the current terminal advertises.
    ///
    /// Honours [`NO_COLOR`](https://no-color.org), then `COLORTERM`, then `TERM`,
    /// and treats Windows Terminal as truecolor. Falls back to
    /// [`Ansi256`](Self::Ansi256), which is near-universal and keeps the gradient
    /// readable.
    pub fn detect() -> Self {
        let var = |k: &str| std::env::var(k).unwrap_or_default().to_ascii_lowercase();

        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return ColorDepth::Mono;
        }
        let term = var("TERM");
        if term == "dumb" {
            return ColorDepth::Mono;
        }
        let colorterm = var("COLORTERM");
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return ColorDepth::TrueColor;
        }
        // Windows Terminal, and conhost on Windows 10 1703 and later, are 24-bit.
        if std::env::var_os("WT_SESSION").is_some() || (cfg!(windows) && term.is_empty()) {
            return ColorDepth::TrueColor;
        }
        if term.contains("256color") {
            return ColorDepth::Ansi256;
        }
        if term.contains("16color") || term == "linux" {
            return ColorDepth::Ansi16;
        }
        ColorDepth::Ansi256
    }

    /// True when anything at all should be coloured.
    #[inline]
    pub fn is_color(self) -> bool {
        self != ColorDepth::Mono
    }

    /// Map an RGB triple onto the closest colour this depth can show.
    pub fn quantize(self, (r, g, b): (u8, u8, u8)) -> Option<Fg> {
        match self {
            ColorDepth::Mono => None,
            ColorDepth::TrueColor => Some(Fg::Rgb(r, g, b)),
            ColorDepth::Ansi256 => Some(Fg::Indexed(ansi256(r, g, b))),
            ColorDepth::Ansi16 => Some(Fg::Basic(ansi16(r, g, b))),
        }
    }
}

/// A foreground colour, resolved to whatever the terminal can show.
///
/// Written as its own SGR sequence rather than through a `crossterm` command:
/// crossterm routes colour through the Win32 console API when virtual-terminal
/// processing is unavailable, which cannot work when the sink is an arbitrary
/// writer, and this crate is VT-only anyway (the alternate screen and DEC 2026
/// synchronized output have no console-API equivalent). Emitting the bytes
/// directly also makes every renderer testable against a plain `Vec<u8>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fg {
    /// 24-bit colour.
    Rgb(u8, u8, u8),
    /// An index into the 256-colour palette.
    Indexed(u8),
    /// One of the 16 basic colours.
    Basic(u8),
}

/// Return the foreground to the terminal's default without disturbing any other
/// attribute the surrounding program may have set.
pub(crate) const FG_RESET: &str = "\x1b[39m";

impl std::fmt::Display for Fg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Fg::Rgb(r, g, b) => write!(f, "\x1b[38;2;{r};{g};{b}m"),
            Fg::Indexed(i) => write!(f, "\x1b[38;5;{i}m"),
            // 30..=37 for the first eight, 90..=97 for the bright half.
            Fg::Basic(i @ 0..=7) => write!(f, "\x1b[{}m", 30 + i as u16),
            Fg::Basic(i) => write!(f, "\x1b[{}m", 90 + (i.min(15) - 8) as u16),
        }
    }
}

/// Nearest index in the xterm 256-colour palette: the 6x6x6 cube, or the 24-step
/// grey ramp when the channels are close enough to neutral for it to be a better
/// match (the ramp is much finer than the cube's 51-unit spacing).
fn ansi256(r: u8, g: u8, b: u8) -> u8 {
    let (lo, hi) = (r.min(g).min(b) as i32, r.max(g).max(b) as i32);
    if hi - lo < 12 {
        let level = ((r as i32 + g as i32 + b as i32) / 3 - 8).clamp(0, 238);
        return 232 + (level * 23 / 238) as u8;
    }
    // The cube's levels are 0, 95, 135, 175, 215, 255: not evenly spaced, so map
    // through the same steps xterm uses rather than dividing by 51.
    let step = |v: u8| -> u8 {
        match v {
            0..=47 => 0,
            48..=114 => 1,
            115..=154 => 2,
            155..=194 => 3,
            195..=234 => 4,
            _ => 5,
        }
    };
    16 + 36 * step(r) + 6 * step(g) + step(b)
}

/// Nearest of the 16 ANSI colours, by squared distance in RGB.
fn ansi16(r: u8, g: u8, b: u8) -> u8 {
    const PALETTE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    let dist = |&(pr, pg, pb): &(u8, u8, u8)| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(pr, r) + d(pg, g) + d(pb, b)
    };
    PALETTE
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| dist(c))
        .map(|(i, _)| i as u8)
        .unwrap_or(7)
}

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// How revealed ink is coloured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Palette {
    /// A warm frontier glow: bright `head` at the leading edge easing to `body`.
    #[default]
    Glow,
    /// A position-based rainbow, in the spirit of `lolcat`.
    Rainbow,
}

/// Visual options for the reveal.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Width of the soft leading edge, in rank units. The band of cells within
    /// `feather` of the frontier is the glowing "head". `0.0` disables the glow.
    pub feather: f32,
    /// Colour of settled (fully revealed) ink, under the `Glow` palette.
    pub body: (u8, u8, u8),
    /// Colour at the very frontier, blended toward `body` across the feather.
    pub head: (u8, u8, u8),
    /// Colour resolution to emit. Defaults to what the terminal advertises, and
    /// to [`ColorDepth::Mono`] when `NO_COLOR` is set.
    pub depth: ColorDepth,
    /// How revealed cells are coloured.
    pub palette: Palette,
    /// Caption colour, for the line beneath the art.
    pub caption: (u8, u8, u8),
}

impl Default for Style {
    /// The dark-terminal palette: cool slate ink with a warm gold leading edge.
    fn default() -> Self {
        Style {
            feather: 0.07,
            body: (120, 134, 168),
            head: (255, 226, 138),
            depth: ColorDepth::detect(),
            palette: Palette::Glow,
            caption: (120, 134, 168),
        }
    }
}

impl Style {
    /// A rainbow palette in the spirit of `lolcat`: each glyph takes its hue from
    /// its position, so the art reveals in diagonal bands of colour.
    pub fn rainbow() -> Self {
        Style {
            palette: Palette::Rainbow,
            ..Style::default()
        }
    }

    /// Tuned for a light terminal background.
    ///
    /// The default palette assumes a dark ground: its slate body sits at roughly
    /// 3:1 against white, below the readable threshold, and the gold frontier all
    /// but vanishes. This darkens both ends and keeps the same warm-cool
    /// relationship.
    pub fn light() -> Self {
        Style {
            body: (72, 84, 112),
            head: (176, 106, 12),
            caption: (96, 106, 130),
            ..Style::default()
        }
    }

    /// No colour: glyphs only. What `NO_COLOR` selects.
    pub fn monochrome() -> Self {
        Style {
            depth: ColorDepth::Mono,
            ..Style::default()
        }
    }

    /// True when this style emits any colour at all.
    #[inline]
    pub fn is_color(&self) -> bool {
        self.depth.is_color()
    }
}

// ---------------------------------------------------------------------------
// Colour of a single cell
// ---------------------------------------------------------------------------

/// Linear interpolation between two RGB colours; `s == 0` yields `a`, `s == 1` yields `b`.
fn blend(a: (u8, u8, u8), b: (u8, u8, u8), s: f32) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * s)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

/// How far behind the frontier a cell sits, `0.0` at the leading edge and `1.0`
/// once settled.
#[inline]
fn settle(style: &Style, progress: f32, rank: f32) -> f32 {
    if style.feather <= 0.0 {
        1.0
    } else {
        ((progress - rank) / style.feather).clamp(0.0, 1.0)
    }
}

/// The colour an ink cell shows at `progress`: `head` at the frontier, easing to
/// `body` once it has settled `feather` behind.
pub(crate) fn frontier_rgb(style: &Style, progress: f32, rank: f32) -> (u8, u8, u8) {
    blend(style.head, style.body, settle(style, progress, rank))
}

/// The colour of a revealed cell, honouring the style's palette. `t` is elapsed
/// seconds, which animates the rainbow; pass `0.0` for a still frame.
pub(crate) fn cell_rgb(
    style: &Style,
    progress: f32,
    rank: f32,
    x: u16,
    y: u16,
    t: f32,
) -> (u8, u8, u8) {
    match style.palette {
        Palette::Glow => frontier_rgb(style, progress, rank),
        Palette::Rainbow => rainbow_rgb(x, y, t),
    }
}

/// A `lolcat` style hue from a cell's position, drifting over time.
fn rainbow_rgb(x: u16, y: u16, t: f32) -> (u8, u8, u8) {
    let hue = (x as f32 * 0.05 + y as f32 * 0.12 + t * 0.4).rem_euclid(1.0);
    hsl_to_rgb(hue, 0.95, 0.62)
}

/// HSL to RGB, with hue in `0..1`.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

// ---------------------------------------------------------------------------
// The shared row writer
// ---------------------------------------------------------------------------

/// What is being drawn: the picture, its ranks, and how it is coloured.
///
/// These three always travel together, in every renderer, and are fixed for the
/// life of a reveal. Naming the triple keeps the row writer's signature about the
/// row rather than about plumbing.
#[derive(Clone, Copy)]
pub(crate) struct Scene<'a> {
    pub art: &'a Art,
    pub ranks: &'a RankMap,
    pub style: &'a Style,
}

/// Write row `y` of the reveal at `progress` into `out`.
///
/// This is the one place a row of the reveal is turned into bytes. It clips at
/// `budget` display columns without ever splitting a wide glyph, coalesces runs
/// of one colour into a single escape, and trims trailing blanks (so callers
/// clear the line first when a previous frame may have left ink there).
///
/// `t` is elapsed seconds, which animates the rainbow palette.
pub(crate) fn queue_row<W: Write>(
    out: &mut W,
    scene: Scene<'_>,
    progress: f32,
    t: f32,
    y: u16,
    budget: u16,
) -> io::Result<()> {
    let Scene { art, ranks, style } = scene;
    // Collect first so trailing blanks can be dropped rather than emitted.
    let mut cells: Vec<(u16, Paint)> = Vec::with_capacity(art.width() as usize);
    for (x, at, paint) in frame::row(art, ranks, progress, y) {
        if at.saturating_add(paint.cols()) > budget {
            break;
        }
        cells.push((x, paint));
    }
    while matches!(cells.last(), Some((_, Paint::Blank { .. }))) {
        cells.pop();
    }

    let mut current: Option<Fg> = None;
    for (x, paint) in cells {
        match paint {
            Paint::Blank { cols } => {
                // Blanks carry no colour, so drop out of the current run rather
                // than painting a background nobody asked for.
                if current.take().is_some() {
                    write!(out, "{FG_RESET}")?;
                }
                for _ in 0..cols {
                    out.write_all(b" ")?;
                }
            }
            Paint::Ink { glyph, .. } => {
                let color = ranks
                    .rank_at(x, y)
                    .and_then(|r| style.depth.quantize(cell_rgb(style, progress, r, x, y, t)));
                if color != current {
                    match color {
                        Some(c) => write!(out, "{c}")?,
                        None => write!(out, "{FG_RESET}")?,
                    }
                    current = color;
                }
                write!(out, "{glyph}")?;
            }
        }
    }
    if current.is_some() {
        write!(out, "{FG_RESET}")?;
    }
    Ok(())
}

/// The area available to draw in, in display columns and rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

impl Viewport {
    /// The terminal's current size, or a conservative 80x24 if it cannot be read.
    pub fn detect() -> Self {
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        Viewport {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }

    /// Where to place `art` so it is centred, and how much of it fits. Art larger
    /// than the viewport is anchored at the edge and clipped rather than allowed
    /// to wrap, which would desynchronise every subsequent frame.
    pub fn fit(&self, art: &Art, reserve_rows: u16) -> Fit {
        let art_w = frame::art_cols(art);
        let usable_rows = self.rows.saturating_sub(reserve_rows);
        Fit {
            ox: self.cols.saturating_sub(art_w) / 2,
            oy: usable_rows.saturating_sub(art.height()) / 2,
            cols: art_w.min(self.cols),
            rows: art.height().min(usable_rows),
        }
    }
}

/// Where a piece of art sits in the viewport, and how much of it is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fit {
    pub ox: u16,
    pub oy: u16,
    pub cols: u16,
    pub rows: u16,
}

// ---------------------------------------------------------------------------
// Reveal: the diffing session
// ---------------------------------------------------------------------------

/// Per-cell visual state, used for frame diffing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellState {
    Hidden,
    /// Lit at a quantised brightness `0..=GLOW_LEVELS` (`GLOW_LEVELS` == settled).
    Lit(u8),
}

/// A live terminal reveal session.
///
/// Construct it, call [`render`](Reveal::render) with each new progress value as
/// your task advances, then [`finish`](Reveal::finish). The terminal is restored
/// on drop, on panic, and on Ctrl+C, and everything degrades to a no-op when
/// stdout is not a TTY (piped, redirected, CI), so the same code is safe
/// everywhere.
///
/// Progress may move backwards as well as forwards; the reveal is seekable.
///
/// ```no_run
/// use inkling::{Art, ordering::{Ordering, Geodesic}, render::{Reveal, Style}};
///
/// let art = Art::parse(include_str!("../assets/dragon.txt"));
/// let ranks = Geodesic::default().rank(&art);
///
/// let mut reveal = Reveal::new(&art, &ranks, Style::default())?;
/// for done in 0..=100 {
///     reveal.render(done as f32 / 100.0)?;
///     // ... do a slice of real work ...
/// }
/// reveal.finish()?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Reveal<'a> {
    art: &'a Art,
    ranks: &'a RankMap,
    style: Style,
    state: Vec<CellState>,
    out: io::Stdout,
    /// Viewport as of the last frame; a change means the terminal was resized.
    viewport: Viewport,
    fit: Fit,
    /// Whether we entered the alternate screen (true only on a TTY, until finished).
    active: bool,
}

impl<'a> Reveal<'a> {
    /// Begin a reveal session. On a TTY this switches to the alternate screen and
    /// hides the cursor; otherwise it is inert until [`finish`](Reveal::finish).
    pub fn new(art: &'a Art, ranks: &'a RankMap, style: Style) -> io::Result<Self> {
        let mut out = io::stdout();
        let active = out.is_terminal();
        let viewport = if active {
            Viewport::detect()
        } else {
            Viewport { cols: 80, rows: 24 }
        };
        let fit = viewport.fit(art, 0);
        if active {
            guard::arm();
            execute!(out, EnterAlternateScreen, Hide, Clear(ClearType::All))?;
            guard::set_alt_screen(true);
            guard::set_cursor_hidden(true);
        }
        Ok(Reveal {
            art,
            ranks,
            style,
            state: vec![CellState::Hidden; art.cell_count()],
            out,
            viewport,
            fit,
            active,
        })
    }

    /// Render the frame at `progress`. A no-op when stdout is not a TTY.
    pub fn render(&mut self, progress: f32) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        // A resize invalidates every cached cell position, so start the diff over.
        let viewport = Viewport::detect();
        if viewport != self.viewport {
            self.viewport = viewport;
            self.fit = viewport.fit(self.art, 0);
            self.state.fill(CellState::Hidden);
            execute!(self.out, Clear(ClearType::All))?;
        }
        self.paint(progress)
    }

    /// Diff `progress`'s frame against the cached state and repaint only the cells
    /// that moved.
    fn paint(&mut self, progress: f32) -> io::Result<()> {
        let (art, ranks, style, fit) = (self.art, self.ranks, &self.style, self.fit);
        let mut dirty = false;

        for y in 0..fit.rows {
            for (x, at, cell) in frame::row(art, ranks, progress, y) {
                if at.saturating_add(cell.cols()) > fit.cols {
                    break;
                }
                let idx = art.index(x, y);
                let target = match cell {
                    Paint::Blank { .. } => CellState::Hidden,
                    Paint::Ink { .. } => {
                        let rank = ranks.rank_at(x, y).unwrap_or(0.0);
                        let level = match style.palette {
                            // A rainbow cell's colour is fixed by position, so it
                            // settles immediately and never needs a frontier repaint.
                            Palette::Rainbow => GLOW_LEVELS,
                            Palette::Glow => {
                                (settle(style, progress, rank) * GLOW_LEVELS as f32).round() as u8
                            }
                        };
                        CellState::Lit(level)
                    }
                };

                if self.state[idx] == target {
                    continue;
                }
                if !dirty {
                    queue!(self.out, Print(SYNC_BEGIN))?;
                    dirty = true;
                }
                queue!(self.out, MoveTo(fit.ox + at, fit.oy + y))?;
                match (target, cell) {
                    // Clear across the glyph's full display width so a hidden wide
                    // cell never leaves a stray half-column behind.
                    (CellState::Hidden, _) => {
                        for _ in 0..cell.cols() {
                            queue!(self.out, Print(' '))?;
                        }
                    }
                    (CellState::Lit(level), Paint::Ink { glyph, .. }) => {
                        let rgb = match style.palette {
                            Palette::Rainbow => rainbow_rgb(x, y, 0.0),
                            Palette::Glow => {
                                blend(style.head, style.body, level as f32 / GLOW_LEVELS as f32)
                            }
                        };
                        if let Some(c) = style.depth.quantize(rgb) {
                            write!(self.out, "{c}")?;
                        }
                        write!(self.out, "{glyph}")?;
                    }
                    (CellState::Lit(_), Paint::Blank { .. }) => unreachable!(),
                }
                self.state[idx] = target;
            }
        }

        if dirty {
            write!(self.out, "{FG_RESET}")?;
            queue!(self.out, Print(SYNC_END))?;
            self.out.flush()?;
        }
        Ok(())
    }

    /// Restore the terminal and leave the completed art in normal scrollback.
    pub fn finish(mut self) -> io::Result<()> {
        self.restore()?;
        write!(self.out, "{}", frame::to_string(self.art, self.ranks, 1.0))?;
        self.out.flush()
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            execute!(self.out, ResetColor, Show, LeaveAlternateScreen)?;
            guard::set_alt_screen(false);
            guard::set_cursor_hidden(false);
        }
        Ok(())
    }
}

impl Drop for Reveal<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Animate the reveal of `art` over `duration`, driven by `easing`.
///
/// A convenience driver built on [`Reveal`] for demos and indeterminate waits.
/// When stdout is not a TTY it prints the final frame once and returns.
pub fn animate(
    art: &Art,
    ranks: &RankMap,
    style: Style,
    duration: Duration,
    easing: Easing,
) -> io::Result<()> {
    if !io::stdout().is_terminal() {
        print!("{}", frame::to_string(art, ranks, 1.0));
        return Ok(());
    }

    let mut reveal = Reveal::new(art, ranks, style)?;
    let total = duration.as_secs_f32().max(0.001);
    let frame_time = Duration::from_millis(16); // ~60 fps
    let start = Instant::now();

    for tick in 1u32.. {
        let t = (start.elapsed().as_secs_f32() / total).min(1.0);
        reveal.render(easing.apply(t))?;
        if t >= 1.0 {
            break;
        }
        // Sleep until the next tick boundary so pacing does not drift with the
        // time spent painting.
        if let Some(remaining) = (start + frame_time * tick).checked_duration_since(Instant::now())
        {
            std::thread::sleep(remaining);
        }
    }
    reveal.finish()
}

// Re-exported for the loader and kept public for callers measuring their own art.
pub use crate::frame::art_cols;
pub use crate::width::{glyph_cols as display_cols, truncate_to_cols};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ordering::{Geodesic, Ordering};

    fn row_bytes(
        art: &Art,
        ranks: &RankMap,
        style: &Style,
        progress: f32,
        y: u16,
        budget: u16,
    ) -> String {
        let mut buf: Vec<u8> = Vec::new();
        let scene = Scene { art, ranks, style };
        queue_row(&mut buf, scene, progress, 0.0, y, budget).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn monochrome_rows_carry_no_escapes() {
        let art = Art::parse("####");
        let ranks = Geodesic::default().rank(&art);
        let out = row_bytes(&art, &ranks, &Style::monochrome(), 1.0, 0, 80);
        assert_eq!(out, "####");
    }

    #[test]
    fn rows_are_clipped_to_the_budget() {
        let art = Art::parse("##########");
        let ranks = Geodesic::default().rank(&art);
        let out = row_bytes(&art, &ranks, &Style::monochrome(), 1.0, 0, 4);
        assert_eq!(out, "####");
    }

    /// A wide glyph is dropped whole rather than split across the clip edge.
    #[cfg(feature = "unicode")]
    #[test]
    fn clipping_never_splits_a_wide_glyph() {
        let art = Art::parse("世界");
        let ranks = Geodesic::default().rank(&art);
        assert_eq!(
            row_bytes(&art, &ranks, &Style::monochrome(), 1.0, 0, 3),
            "世"
        );
        assert_eq!(
            row_bytes(&art, &ranks, &Style::monochrome(), 1.0, 0, 4),
            "世界"
        );
    }

    /// A hidden wide glyph reserves both its columns, so revealed ink to its right
    /// never slides sideways as the frontier passes.
    #[cfg(feature = "unicode")]
    #[test]
    fn hidden_wide_glyphs_hold_their_columns() {
        use crate::width::str_cols;
        let art = Art::parse("世a");
        let mut ranks = RankMap::new(art.width(), art.height());
        ranks.set(0, 0, 1.0); // the wide glyph reveals last
        ranks.set(1, 0, 0.0);
        let early = row_bytes(&art, &ranks, &Style::monochrome(), 0.5, 0, 80);
        assert_eq!(early, "  a", "hidden wide glyph must reserve two columns");
        assert_eq!(str_cols(&early), 3);
        assert_eq!(
            str_cols(&row_bytes(&art, &ranks, &Style::monochrome(), 1.0, 0, 80)),
            3
        );
    }

    #[test]
    fn trailing_blanks_are_trimmed() {
        let art = Art::parse("#   #");
        let mut ranks = RankMap::new(art.width(), art.height());
        ranks.set(0, 0, 0.0);
        ranks.set(4, 0, 1.0);
        assert_eq!(
            row_bytes(&art, &ranks, &Style::monochrome(), 0.5, 0, 80),
            "#"
        );
    }

    #[test]
    fn colour_runs_are_coalesced() {
        let art = Art::parse("####");
        let ranks = Geodesic::default().rank(&art);
        let style = Style {
            feather: 0.0, // every settled cell is the same body colour
            depth: ColorDepth::TrueColor,
            ..Style::default()
        };
        let out = row_bytes(&art, &ranks, &style, 1.0, 0, 80);
        assert_eq!(
            out.matches("\x1b[38;2;").count(),
            1,
            "one escape should cover the whole run: {out:?}"
        );
        assert!(out.ends_with(FG_RESET), "run must be closed: {out:?}");
    }

    #[test]
    fn depth_maps_onto_the_available_palette() {
        assert_eq!(ColorDepth::Mono.quantize((255, 0, 0)), None);
        assert_eq!(
            ColorDepth::TrueColor.quantize((1, 2, 3)),
            Some(Fg::Rgb(1, 2, 3))
        );
        assert_eq!(
            ColorDepth::Ansi16.quantize((250, 10, 10)),
            Some(Fg::Basic(9))
        );
        // Neutral triples take the 24-step grey ramp, which is far finer than the
        // cube's 51-unit spacing: 232 is its black end and 255 its white one.
        assert_eq!(
            ColorDepth::Ansi256.quantize((0, 0, 0)),
            Some(Fg::Indexed(232))
        );
        assert_eq!(
            ColorDepth::Ansi256.quantize((255, 255, 255)),
            Some(Fg::Indexed(255))
        );
        // A saturated triple goes to the 6x6x6 cube instead.
        assert_eq!(
            ColorDepth::Ansi256.quantize((255, 0, 0)),
            Some(Fg::Indexed(16 + 36 * 5))
        );
    }

    #[test]
    fn foreground_escapes_are_well_formed() {
        assert_eq!(Fg::Rgb(1, 2, 3).to_string(), "\x1b[38;2;1;2;3m");
        assert_eq!(Fg::Indexed(200).to_string(), "\x1b[38;5;200m");
        assert_eq!(Fg::Basic(3).to_string(), "\x1b[33m");
        assert_eq!(Fg::Basic(9).to_string(), "\x1b[91m");
        assert_eq!(FG_RESET, "\x1b[39m");
    }

    #[test]
    fn viewport_fit_clips_oversized_art() {
        let art = Art::parse(&"##########\n".repeat(10));
        let viewport = Viewport { cols: 4, rows: 3 };
        let fit = viewport.fit(&art, 1);
        assert_eq!(fit.cols, 4, "clipped, not wrapped");
        assert_eq!(fit.rows, 2, "one row reserved for the caption");
        assert_eq!((fit.ox, fit.oy), (0, 0));
    }

    #[test]
    fn viewport_fit_centres_small_art() {
        let art = Art::parse("##");
        let fit = Viewport { cols: 10, rows: 10 }.fit(&art, 0);
        assert_eq!(fit.ox, 4);
        assert_eq!(fit.cols, 2);
    }

    #[test]
    fn light_style_is_darker_than_the_default() {
        let sum = |(r, g, b): (u8, u8, u8)| r as u32 + g as u32 + b as u32;
        assert!(sum(Style::light().body) < sum(Style::default().body));
        assert!(sum(Style::light().head) < sum(Style::default().head));
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn display_width_counts_wide_glyphs() {
        use crate::width::glyph_cols;
        assert_eq!(glyph_cols('a'), 1);
        assert_eq!(glyph_cols('世'), 2);
        let art = Art::parse("a世\nbb"); // row 0 is 1 + 2 = 3 columns wide
        assert_eq!(art_cols(&art), 3);
    }
}
