//! Terminal renderer: an optimised, colourful superset of [`crate::frame`].
//!
//! The unit of rendering is a [`Reveal`] session: construct it, push a new
//! `progress` value whenever your task advances, and finish. Each call diffs
//! against the previous frame, so only cells whose appearance changed are
//! repainted, in practice the moving "frontier" band plus whatever ink just
//! settled. Settled cells are painted exactly once.
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
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::{art::Art, easing::Easing, rank::RankMap};

/// Number of quantised brightness steps across the frontier. The frontier band
/// repaints as it moves; level `GLOW_LEVELS` is "settled" and paints just once.
const GLOW_LEVELS: u8 = 8;

/// DEC private mode 2026, *synchronized output*. A terminal that understands it
/// buffers everything between begin and end and presents the frame as one atomic
/// update, so a reveal never tears mid-paint; terminals that do not recognise the
/// mode silently ignore both markers, so it is always safe to emit.
pub(crate) const SYNC_BEGIN: &str = "\x1b[?2026h";
pub(crate) const SYNC_END: &str = "\x1b[?2026l";

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
    /// Emit colour. Defaults off when `NO_COLOR` is set.
    pub color: bool,
    /// How revealed cells are coloured.
    pub palette: Palette,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            feather: 0.07,
            body: (120, 134, 168),
            head: (255, 226, 138),
            color: std::env::var_os("NO_COLOR").is_none(),
            palette: Palette::Glow,
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
}

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
/// on drop even if you forget, and everything degrades to a no-op when stdout is
/// not a TTY (piped, redirected, CI), so the same code is safe everywhere.
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
    origin: (u16, u16),
    /// Whether we entered the alternate screen (true only on a TTY, until finished).
    active: bool,
}

impl<'a> Reveal<'a> {
    /// Begin a reveal session. On a TTY this switches to the alternate screen and
    /// hides the cursor; otherwise it is inert until [`finish`](Reveal::finish).
    pub fn new(art: &'a Art, ranks: &'a RankMap, style: Style) -> io::Result<Self> {
        let mut out = io::stdout();
        let active = out.is_terminal();
        let origin = if active {
            let (cols, _) = terminal::size().unwrap_or((art.width(), art.height()));
            (cols.saturating_sub(art_cols(art)) / 2, 1)
        } else {
            (0, 0)
        };
        if active {
            execute!(out, EnterAlternateScreen, Hide, Clear(ClearType::All))?;
        }
        Ok(Reveal {
            art,
            ranks,
            style,
            state: vec![CellState::Hidden; art.cell_count()],
            out,
            origin,
            active,
        })
    }

    /// Render the frame at `progress`. A no-op when stdout is not a TTY.
    pub fn render(&mut self, progress: f32) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        paint(
            &mut self.out,
            self.art,
            self.ranks,
            &self.style,
            &mut self.state,
            progress,
            self.origin,
        )
    }

    /// Restore the terminal and leave the completed art in normal scrollback.
    pub fn finish(mut self) -> io::Result<()> {
        self.restore()?;
        write!(
            self.out,
            "{}",
            crate::frame::to_string(self.art, self.ranks, 1.0)
        )?;
        self.out.flush()
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            execute!(self.out, ResetColor, Show, LeaveAlternateScreen)?;
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
        print!("{}", crate::frame::to_string(art, ranks, 1.0));
        return Ok(());
    }

    let mut reveal = Reveal::new(art, ranks, style)?;
    let total = duration.as_secs_f32().max(0.001);
    let frame = Duration::from_millis(16); // ~60 fps
    let start = Instant::now();

    for tick in 1.. {
        let t = (start.elapsed().as_secs_f32() / total).min(1.0);
        reveal.render(easing.apply(t))?;
        if t >= 1.0 {
            break;
        }
        // Sleep until the next tick boundary so pacing does not drift with the
        // time spent painting.
        if let Some(remaining) = (start + frame * tick).checked_duration_since(Instant::now()) {
            std::thread::sleep(remaining);
        }
    }
    reveal.finish()
}

/// Diff `progress`'s frame against `state` and repaint only the cells that moved.
fn paint(
    out: &mut io::Stdout,
    art: &Art,
    ranks: &RankMap,
    style: &Style,
    state: &mut [CellState],
    progress: f32,
    (ox, oy): (u16, u16),
) -> io::Result<()> {
    let mut dirty = false;
    for y in 0..art.height() {
        let mut col = 0u16; // display column, so wide glyphs stay aligned
        for x in 0..art.width() {
            let glyph = art.glyph(x, y);
            let cw = glyph_cols(glyph);
            let idx = art.index(x, y);
            let target = match ranks.rank_at(x, y) {
                Some(r) if r <= progress => {
                    let level = match style.palette {
                        // A rainbow cell's colour is fixed by position, so it
                        // settles immediately and never needs a frontier repaint.
                        Palette::Rainbow => GLOW_LEVELS,
                        Palette::Glow if style.feather <= 0.0 => GLOW_LEVELS,
                        Palette::Glow => {
                            let a = ((progress - r) / style.feather).clamp(0.0, 1.0);
                            (a * GLOW_LEVELS as f32).round() as u8
                        }
                    };
                    CellState::Lit(level)
                }
                _ => CellState::Hidden,
            };

            if state[idx] != target {
                if !dirty {
                    queue!(out, Print(SYNC_BEGIN))?;
                }
                queue!(out, MoveTo(ox + col, oy + y))?;
                match target {
                    // Clear across the glyph's full display width so a hidden wide
                    // cell never leaves a stray half-column behind.
                    CellState::Hidden => {
                        for _ in 0..cw {
                            queue!(out, Print(' '))?;
                        }
                    }
                    CellState::Lit(level) => {
                        if style.color {
                            let (r, g, b) = match style.palette {
                                Palette::Rainbow => rainbow_rgb(x, y, 0.0),
                                Palette::Glow => {
                                    blend(style.head, style.body, level as f32 / GLOW_LEVELS as f32)
                                }
                            };
                            queue!(out, SetForegroundColor(Color::Rgb { r, g, b }))?;
                        }
                        queue!(out, Print(glyph))?;
                    }
                }
                state[idx] = target;
                dirty = true;
            }
            col += cw;
        }
    }

    if dirty {
        queue!(out, ResetColor, Print(SYNC_END))?;
        out.flush()?;
    }
    Ok(())
}

/// Linear interpolation between two RGB colours; `s == 0` yields `a`, `s == 1` yields `b`.
fn blend(a: (u8, u8, u8), b: (u8, u8, u8), s: f32) -> (u8, u8, u8) {
    let lerp = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * s)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

/// The colour an ink cell shows at `progress`: `head` at the frontier, easing to
/// `body` once it has settled `feather` behind. Shared with the loader renderer.
pub(crate) fn frontier_rgb(style: &Style, progress: f32, rank: f32) -> (u8, u8, u8) {
    if style.feather <= 0.0 {
        return style.body;
    }
    let a = ((progress - rank) / style.feather).clamp(0.0, 1.0);
    blend(style.head, style.body, a)
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

/// Display columns a glyph occupies: 0 for zero-width or combining marks, 2 for
/// wide glyphs (CJK and many emoji), 1 otherwise. Keeps the reveal aligned when
/// the art is not pure ASCII.
pub(crate) fn glyph_cols(c: char) -> u16 {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) as u16
}

/// The widest row of `art`, in display columns.
pub(crate) fn art_cols(art: &Art) -> u16 {
    (0..art.height())
        .map(|y| {
            (0..art.width())
                .map(|x| glyph_cols(art.glyph(x, y)))
                .sum::<u16>()
        })
        .max()
        .unwrap_or(0)
}

/// Truncate `s` to at most `max` display columns, dropping whole glyphs so a wide
/// glyph is never split across the edge.
pub(crate) fn truncate_to_cols(s: &str, max: u16) -> String {
    let mut out = String::new();
    let mut used = 0u16;
    for c in s.chars() {
        let w = glyph_cols(c);
        if used + w > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_counts_wide_glyphs() {
        assert_eq!(glyph_cols('a'), 1);
        assert_eq!(glyph_cols('世'), 2);
        let art = Art::parse("a世\nbb"); // row 0 is 1 + 2 = 3 columns wide
        assert_eq!(art_cols(&art), 3);
    }

    #[test]
    fn truncate_respects_display_width() {
        assert_eq!(truncate_to_cols("abc", 2), "ab");
        assert_eq!(truncate_to_cols("a世", 3), "a世"); // 1 + 2 == 3 fits
        assert_eq!(truncate_to_cols("世界", 3), "世"); // 2 + 2 > 3, drop the second
    }
}
