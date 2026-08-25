//! `Loader`: the ergonomic, thread-safe front door to Inkling.
//!
//! This is how most programs should use Inkling. Create a [`Loader`] with a total,
//! advance it from anywhere with [`inc`](Loader::inc) or [`set`](Loader::set), and
//! a background thread keeps a living reveal painted at ~30 fps until you
//! [`finish`](Loader::finish). It mirrors the idioms people already expect from a
//! progress bar:
//!
//! * **Drive it by hand** with `inc`/`set`, determinate or [`spinner`](Loader::spinner).
//! * **Wrap an iterator**: `for x in items.inkling() { .. }`.
//! * **Wrap a reader**: `loader.wrap_read(file)` advances by bytes read.
//! * **Log around it** with [`println`](Loader::println) or
//!   [`suspend`](Loader::suspend), which lift the art out of the way first.
//!
//! The handle is cheap to clone (via [`handle`](Loader::handle)) and `Send + Sync`,
//! so worker threads can report progress while the render thread owns the terminal,
//! which keeps all drawing on one thread and free of races. When stdout is not a
//! TTY the loader does not animate; it prints the finished art once on `finish`, so
//! logs and CI still show the result.

use std::io::{self, IsTerminal, Read, Write};
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};
use std::sync::atomic::{AtomicU16, AtomicU64, AtomicU8};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveTo, MoveToColumn, MoveToNextLine, MoveToPreviousLine, Show},
    execute, queue,
    style::{Print, ResetColor},
    terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::art::Art;
use crate::easing::Easing;
use crate::ordering::{Directional, Ordering};
use crate::render::{queue_row, Scene, Style, Viewport};
use crate::{frame, guard, width};

/// The built-in art used when you do not supply your own.
const DEFAULT_ART: &str = include_str!("../assets/dragon.txt");
const FPS: u64 = 30;

/// Time constant of the glide toward the true progress value, in seconds. Applied
/// as `1 - exp(-dt / TAU)` so the smoothing is the same however fast we redraw.
const GLIDE_TAU: f32 = 0.12;

// Loader lifecycle, stored in `Shared::state`.
const RUNNING: u8 = 0;
const FINISH_KEEP: u8 = 1; // complete the art and leave it on screen
const FINISH_CLEAR: u8 = 2; // complete and erase the art

/// State shared between the public handles and the render thread.
struct Shared {
    pos: AtomicU64,
    total: AtomicU64, // 0 means indeterminate (spinner)
    state: AtomicU8,
    /// Lines the inline block currently occupies on screen. `0` means nothing is
    /// drawn, so the next frame starts fresh instead of stepping back over stale
    /// output. Suspending resets it, which is what lets a log line land cleanly.
    drawn_lines: AtomicU16,
    message: Mutex<String>,
    /// Held for the duration of a frame. [`Loader::suspend`] takes it so it can
    /// never interleave with a paint.
    painting: Mutex<()>,
    art: Art,
    ranks: crate::rank::RankMap,
    style: Style,
    easing: Easing,
    started: Instant,
}

impl Shared {
    fn inc(&self, delta: u64) {
        self.pos.fetch_add(delta, Relaxed);
    }
    fn set(&self, pos: u64) {
        self.pos.store(pos, Relaxed);
    }
    fn set_message(&self, msg: String) {
        // One choke point for every caption, whichever handle set it, so nothing
        // that reaches the terminal carries control characters. See
        // [`width::sanitize`].
        let msg = width::sanitize(&msg);
        if let Ok(mut guard) = self.message.lock() {
            *guard = msg;
        }
    }
    fn message(&self) -> String {
        self.message
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
    /// Lock the paint mutex, tolerating a poisoned lock: a panicking painter must
    /// not wedge every later frame.
    fn lock_paint(&self) -> MutexGuard<'_, ()> {
        self.painting.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Progress in `0..=1`, already eased. Indeterminate loaders breathe instead.
    fn progress(&self, elapsed: f32) -> f32 {
        let total = self.total.load(Relaxed);
        if total == 0 {
            0.1 + 0.9 * (0.5 - 0.5 * (elapsed * 1.5).cos()) // spinner
        } else {
            let raw = (self.pos.load(Relaxed) as f32 / total as f32).clamp(0.0, 1.0);
            self.easing.apply(raw)
        }
    }
}

/// A live progress reveal.
///
/// Create one with [`Loader::new`], advance it, and [`finish`](Loader::finish).
/// Dropping the last handle finishes it for you, and the terminal is restored on
/// panic and on Ctrl+C too. Not `Clone`; for cross-thread updates take a
/// [`Handle`].
pub struct Loader {
    shared: Arc<Shared>,
    joiner: Mutex<Option<JoinHandle<()>>>,
    tty: bool,
}

impl Loader {
    /// A determinate loader for `total` units of work, using the built-in dragon.
    pub fn new(total: u64) -> Self {
        Builder::new().total(total).start()
    }

    /// An indeterminate loader (a spinner) for work whose length you do not know.
    pub fn spinner() -> Self {
        Builder::new().start()
    }

    /// Configure a loader with custom art, ordering, style, easing, or message.
    pub fn builder() -> Builder {
        Builder::new()
    }

    /// Advance the position by `delta`.
    pub fn inc(&self, delta: u64) {
        self.shared.inc(delta);
    }

    /// Set the absolute position.
    pub fn set(&self, pos: u64) {
        self.shared.set(pos);
    }

    /// Change the total amount of work.
    pub fn set_length(&self, total: u64) {
        self.shared.total.store(total, Relaxed);
    }

    /// Set a short caption shown beneath the art.
    pub fn set_message<S: Into<String>>(&self, msg: S) {
        self.shared.set_message(msg.into());
    }

    /// The current position.
    pub fn position(&self) -> u64 {
        self.shared.pos.load(Relaxed)
    }

    /// The total amount of work, or `0` when indeterminate.
    pub fn length(&self) -> u64 {
        self.shared.total.load(Relaxed)
    }

    /// Time since the loader started.
    pub fn elapsed(&self) -> Duration {
        self.shared.started.elapsed()
    }

    /// Average units of work per second so far, or `0.0` before any time passes.
    pub fn rate(&self) -> f64 {
        let secs = self.elapsed().as_secs_f64();
        if secs <= 0.0 {
            0.0
        } else {
            self.position() as f64 / secs
        }
    }

    /// Estimated time remaining, from the average rate so far.
    ///
    /// `None` for an indeterminate loader, before enough has happened to
    /// extrapolate from, or once the work is done.
    pub fn eta(&self) -> Option<Duration> {
        let (total, pos) = (self.length(), self.position());
        let rate = self.rate();
        if total == 0 || pos == 0 || pos >= total || rate <= 0.0 {
            return None;
        }
        Duration::try_from_secs_f64((total - pos) as f64 / rate).ok()
    }

    /// A cheap, clonable, `Send + Sync` handle for reporting progress from other
    /// threads. Handles can update but not finish the loader.
    pub fn handle(&self) -> Handle {
        Handle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Wrap a reader so every byte read advances the loader. Ideal for downloads:
    /// set the length to the content length, then read through the wrapper.
    pub fn wrap_read<R: Read>(&self, reader: R) -> ProgressReader<R> {
        self.handle().wrap_read(reader)
    }

    /// Lift the art out of the way, run `f`, and let the reveal redraw beneath
    /// whatever it printed.
    ///
    /// Without this, anything your program writes to the terminal lands in the
    /// middle of the block and desynchronises the renderer, which is tracking
    /// where its own output sits. Use it for any logging you want interleaved with
    /// a live reveal.
    ///
    /// ```no_run
    /// # use inkling::Loader;
    /// let loader = Loader::new(3);
    /// loader.suspend(|| eprintln!("something worth saying"));
    /// ```
    pub fn suspend<T>(&self, f: impl FnOnce() -> T) -> T {
        if !self.tty {
            return f();
        }
        // Wait for any frame in flight, then take the block off the screen so the
        // caller's output starts on a clean line.
        let _painting = self.shared.lock_paint();
        let lines = self.shared.drawn_lines.swap(0, AcqRel);
        if lines > 0 {
            let mut out = io::stdout();
            let _ = erase_block(&mut out, lines);
            let _ = out.flush();
        }
        f()
    }

    /// Print a line above the reveal, which then redraws beneath it.
    ///
    /// Shorthand for [`suspend`](Self::suspend) around a `println!`.
    pub fn println<S: AsRef<str>>(&self, line: S) {
        self.suspend(|| {
            let mut out = io::stdout();
            let _ = writeln!(out, "{}", line.as_ref());
            let _ = out.flush();
        });
    }

    /// Fill the art, leave it on screen, and restore the terminal.
    pub fn finish(&self) {
        self.finalize(FINISH_KEEP);
    }

    /// Finish and erase the art from the screen.
    pub fn finish_and_clear(&self) {
        self.finalize(FINISH_CLEAR);
    }

    fn finalize(&self, how: u8) {
        let won = self
            .shared
            .state
            .compare_exchange(RUNNING, how, AcqRel, Relaxed)
            .is_ok();
        if self.tty {
            if let Ok(mut guard) = self.joiner.lock() {
                if let Some(handle) = guard.take() {
                    let _ = handle.join();
                }
            }
        } else if won && how == FINISH_KEEP {
            // No animation off a TTY; leave the finished art for logs and CI.
            print!(
                "{}",
                frame::to_string(&self.shared.art, &self.shared.ranks, 1.0)
            );
            let _ = io::stdout().flush();
        }
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.finalize(FINISH_KEEP);
    }
}

/// A cheap, clonable updater obtained from [`Loader::handle`]. Safe to send to and
/// share across threads.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
}

impl Handle {
    /// Advance the position by `delta`.
    pub fn inc(&self, delta: u64) {
        self.shared.inc(delta);
    }
    /// Set the absolute position.
    pub fn set(&self, pos: u64) {
        self.shared.set(pos);
    }
    /// Set the caption.
    pub fn set_message<S: Into<String>>(&self, msg: S) {
        self.shared.set_message(msg.into());
    }
    /// The current position.
    pub fn position(&self) -> u64 {
        self.shared.pos.load(Relaxed)
    }
    /// Wrap a reader so every byte read advances the loader, from any thread.
    pub fn wrap_read<R: Read>(&self, reader: R) -> ProgressReader<R> {
        ProgressReader {
            inner: reader,
            handle: self.clone(),
        }
    }
}

/// Builder for a customised [`Loader`].
pub struct Builder {
    total: u64,
    art: Option<Art>,
    ordering: Box<dyn Ordering>,
    style: Style,
    easing: Easing,
    message: String,
}

impl Builder {
    fn new() -> Self {
        Builder {
            total: 0,
            art: None,
            ordering: Box::new(Directional::default()),
            style: Style::default(),
            easing: Easing::default(),
            message: String::new(),
        }
    }

    /// Units of work. Leave it `0` (the default) for an indeterminate spinner.
    pub fn total(mut self, total: u64) -> Self {
        self.total = total;
        self
    }

    /// The art to reveal. Defaults to the built-in dragon.
    pub fn art(mut self, art: Art) -> Self {
        self.art = Some(art);
        self
    }

    /// The ordering that decides the reveal path. Defaults to [`Directional`].
    pub fn ordering(mut self, ordering: impl Ordering + 'static) -> Self {
        self.ordering = Box::new(ordering);
        self
    }

    /// Colours, frontier glow, and colour depth.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The curve mapping raw completion onto revealed progress. Defaults to
    /// [`Easing::Linear`]. Ignored by indeterminate spinners.
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// A short caption shown beneath the art.
    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.message = width::sanitize(&message.into());
        self
    }

    /// Build the loader and start animating (on a TTY).
    pub fn start(self) -> Loader {
        let art = self.art.unwrap_or_else(|| Art::parse(DEFAULT_ART));
        let ranks = self.ordering.rank(&art);
        let shared = Arc::new(Shared {
            pos: AtomicU64::new(0),
            total: AtomicU64::new(self.total),
            state: AtomicU8::new(RUNNING),
            drawn_lines: AtomicU16::new(0),
            message: Mutex::new(self.message),
            painting: Mutex::new(()),
            art,
            ranks,
            style: self.style,
            easing: self.easing,
            started: Instant::now(),
        });
        let tty = io::stdout().is_terminal();
        let joiner = if tty {
            let shared = Arc::clone(&shared);
            Mutex::new(Some(thread::spawn(move || run(shared))))
        } else {
            Mutex::new(None)
        };
        Loader {
            shared,
            joiner,
            tty,
        }
    }
}

// ---------------------------------------------------------------------------
// Iterator wrapping: `for x in items.inkling() { .. }`
// ---------------------------------------------------------------------------

/// Extension trait that wraps any iterator in a progress reveal.
pub trait ProgressIteratorExt: Iterator + Sized {
    /// Reveal a loader while iterating, inferring the total from `size_hint`.
    fn inkling(self) -> InklingIter<Self> {
        let total = self.size_hint().1.unwrap_or(0) as u64;
        let loader = if total > 0 {
            Loader::new(total)
        } else {
            Loader::spinner()
        };
        InklingIter {
            inner: self,
            loader: Some(loader),
        }
    }

    /// Reveal a specific, pre-configured loader while iterating.
    fn inkling_with(self, loader: Loader) -> InklingIter<Self> {
        InklingIter {
            inner: self,
            loader: Some(loader),
        }
    }
}

impl<I: Iterator> ProgressIteratorExt for I {}

/// Iterator adaptor returned by [`ProgressIteratorExt::inkling`].
pub struct InklingIter<I> {
    inner: I,
    loader: Option<Loader>,
}

impl<I> InklingIter<I> {
    /// The loader driving this iteration, for captions and logging.
    pub fn loader(&self) -> Option<&Loader> {
        self.loader.as_ref()
    }
}

impl<I: Iterator> Iterator for InklingIter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.inner.next();
        match next {
            Some(_) => {
                if let Some(loader) = &self.loader {
                    loader.inc(1);
                }
            }
            None => {
                if let Some(loader) = self.loader.take() {
                    loader.finish();
                }
            }
        }
        next
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<I> Drop for InklingIter<I> {
    fn drop(&mut self) {
        if let Some(loader) = self.loader.take() {
            loader.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Reader wrapping: bytes read advance the loader.
// ---------------------------------------------------------------------------

/// A `Read` wrapper that advances a loader by the number of bytes read.
pub struct ProgressReader<R> {
    inner: R,
    handle: Handle,
}

impl<R> ProgressReader<R> {
    /// Recover the wrapped reader.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.handle.inc(n as u64);
        Ok(n)
    }
}

impl<R: io::BufRead> io::BufRead for ProgressReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt);
        self.handle.inc(amt as u64);
    }
}

// ---------------------------------------------------------------------------
// The render thread: a reveal at ~30 fps.
// ---------------------------------------------------------------------------

/// How the block is positioned on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// In the flow of the terminal, so the next program output follows below it.
    Inline,
    /// Absolutely positioned in the alternate screen, for art too tall to inline.
    Fullscreen,
}

fn run(shared: Arc<Shared>) {
    let mut out = io::stdout();
    guard::arm();

    let mut viewport = Viewport::detect();
    let mut placement = choose_placement(&shared.art, viewport);
    enter(&mut out, placement);

    let frame_time = Duration::from_millis(1000 / FPS);
    let start = Instant::now();
    let mut displayed = 0.0f32;
    let mut last_tick = Instant::now();

    loop {
        let finishing = shared.state.load(Acquire) != RUNNING;
        let elapsed = start.elapsed().as_secs_f32();

        // Re-read the viewport every frame so a resize is handled rather than
        // smeared across the scrollback.
        let now = Viewport::detect();
        if now != viewport {
            let next = choose_placement(&shared.art, now);
            if next != placement {
                leave(&mut out, placement, &shared);
                enter(&mut out, next);
                placement = next;
            }
            shared.drawn_lines.store(0, Relaxed);
            if placement == Placement::Fullscreen {
                let _ = execute!(out, Clear(ClearType::All));
            }
            viewport = now;
        }

        // Glide toward the true value with a fixed time constant, so the easing
        // looks the same however often we redraw.
        let target = shared.progress(elapsed);
        let dt = last_tick.elapsed().as_secs_f32();
        last_tick = Instant::now();
        displayed += (target - displayed) * (1.0 - (-dt / GLIDE_TAU).exp());
        let progress = if finishing { 1.0 } else { displayed };

        {
            let _painting = shared.lock_paint();
            let _ = draw(&mut out, &shared, viewport, placement, progress, elapsed);
        }

        if finishing {
            let cleared = shared.state.load(Relaxed) == FINISH_CLEAR;
            let _painting = shared.lock_paint();
            match (placement, cleared) {
                (Placement::Fullscreen, _) => {
                    leave(&mut out, placement, &shared);
                    if !cleared {
                        let _ = persist_final(&mut out, &shared, viewport);
                    }
                }
                (Placement::Inline, true) => {
                    let lines = shared.drawn_lines.swap(0, Relaxed);
                    let _ = erase_block(&mut out, lines);
                    let _ = execute!(out, Show);
                    guard::set_cursor_hidden(false);
                }
                (Placement::Inline, false) => {
                    // Leave the finished art in place and park the cursor below it.
                    let _ = queue!(out, Print("\r\n"));
                    let _ = execute!(out, Show);
                    guard::set_cursor_hidden(false);
                }
            }
            let _ = out.flush();
            break;
        }
        thread::sleep(frame_time);
    }
}

/// Animate inline while the picture and its caption fit the viewport, which keeps
/// the reveal in the flow of the terminal and lets the next output follow below
/// it. Only when the art will not fit do we fall back to the alternate screen,
/// where it cannot scroll and duplicate itself.
fn choose_placement(art: &Art, viewport: Viewport) -> Placement {
    let fits_height = viewport.rows >= art.height() + 2;
    let fits_width = viewport.cols >= frame::art_cols(art);
    if fits_height && fits_width {
        Placement::Inline
    } else {
        Placement::Fullscreen
    }
}

fn enter(out: &mut io::Stdout, placement: Placement) {
    match placement {
        Placement::Fullscreen => {
            let _ = execute!(out, EnterAlternateScreen, Hide, Clear(ClearType::All));
            guard::set_alt_screen(true);
        }
        Placement::Inline => {
            let _ = execute!(out, Hide);
        }
    }
    guard::set_cursor_hidden(true);
}

fn leave(out: &mut io::Stdout, placement: Placement, shared: &Shared) {
    if placement == Placement::Fullscreen {
        let _ = execute!(out, ResetColor, Show, LeaveAlternateScreen);
        guard::set_alt_screen(false);
        guard::set_cursor_hidden(false);
    }
    shared.drawn_lines.store(0, Relaxed);
}

/// Draw one frame. Both placements share the same row writer; they differ only in
/// how the cursor gets to the start of each line.
fn draw(
    out: &mut impl Write,
    shared: &Shared,
    viewport: Viewport,
    placement: Placement,
    progress: f32,
    t: f32,
) -> io::Result<()> {
    let art = &shared.art;
    let scene = Scene {
        art,
        ranks: &shared.ranks,
        style: &shared.style,
    };
    // One row is reserved for the caption in both placements.
    let fit = viewport.fit(art, 1);

    queue!(out, Print(crate::render::SYNC_BEGIN))?;

    // Step back over the block we drew last time, if it is still on screen.
    // `drawn_lines` is the block's *height*, and the cursor was left on its last
    // line, so the step back is one less than that. Moving the full height would
    // walk the block one row up the screen every frame, climbing over whatever
    // was printed above it and leaving a trail of stale caption lines below.
    let previous = shared.drawn_lines.load(Relaxed);
    if placement == Placement::Inline && previous > 1 {
        queue!(out, MoveToPreviousLine(previous - 1))?;
    }

    for y in 0..fit.rows {
        match placement {
            Placement::Fullscreen => queue!(out, MoveTo(fit.ox, fit.oy + y))?,
            Placement::Inline => queue!(out, MoveToColumn(0))?,
        }
        queue!(out, Clear(ClearType::UntilNewLine))?;
        queue_row(out, scene, progress, t, y, fit.cols)?;
        if placement == Placement::Inline {
            queue!(out, MoveToNextLine(1))?;
        }
    }

    // Caption row beneath the art.
    match placement {
        Placement::Fullscreen => queue!(out, MoveTo(fit.ox, fit.oy + fit.rows))?,
        Placement::Inline => queue!(out, MoveToColumn(0))?,
    }
    queue!(out, Clear(ClearType::UntilNewLine))?;
    let msg = shared.message();
    if !msg.is_empty() {
        let shown = width::truncate_to_cols(&msg, viewport.cols.saturating_sub(1));
        match shared.style.depth.quantize(shared.style.caption) {
            Some(c) => write!(out, "{c}{shown}{}", crate::render::FG_RESET)?,
            None => write!(out, "{shown}")?,
        }
    }

    // The cursor now sits on the caption line, which is the last line of the
    // block: record its height so the next frame knows how far to step back.
    shared.drawn_lines.store(fit.rows + 1, Relaxed);

    queue!(out, Print(crate::render::SYNC_END))?;
    out.flush()
}

/// Erase an inline block of `lines` lines whose last line the cursor is on, and
/// park the cursor back at its top.
fn erase_block(out: &mut impl Write, lines: u16) -> io::Result<()> {
    if lines == 0 {
        return Ok(());
    }
    if lines > 1 {
        queue!(out, MoveToPreviousLine(lines - 1))?;
    }
    queue!(out, MoveToColumn(0))?;
    for _ in 0..lines {
        queue!(
            out,
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            MoveToNextLine(1)
        )?;
    }
    queue!(out, MoveToPreviousLine(lines))?;
    out.flush()
}

/// Print the finished art, coloured and trimmed, into the normal buffer so it
/// stays in scrollback after the alternate screen is gone.
fn persist_final(out: &mut impl Write, shared: &Shared, viewport: Viewport) -> io::Result<()> {
    let art = &shared.art;
    // Feather 0 means every cell reads as settled body colour rather than as a
    // frontier that happens to sit at the end of the bar.
    let style = Style {
        feather: 0.0,
        ..shared.style
    };
    let scene = Scene {
        art,
        ranks: &shared.ranks,
        style: &style,
    };
    for y in 0..art.height() {
        queue_row(out, scene, 1.0, 0.0, y, viewport.cols)?;
        queue!(out, Print("\r\n"))?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::art::Art;

    #[test]
    fn loader_and_handle_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Loader>();
        assert_send_sync::<Handle>();
    }

    #[test]
    fn position_tracks_updates() {
        let loader = Loader::builder().total(10).message("x").start();
        loader.inc(3);
        loader.set(7);
        assert_eq!(loader.position(), 7);
        assert_eq!(loader.length(), 10);
        loader.finish_and_clear();
    }

    #[test]
    fn iterator_yields_every_item() {
        let loader = Loader::builder().total(5).art(Art::parse("##")).start();
        let collected: Vec<i32> = (0..5).inkling_with(loader).collect();
        assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn suspend_runs_the_closure_and_returns_its_value() {
        let loader = Loader::builder().total(4).art(Art::parse("##")).start();
        assert_eq!(loader.suspend(|| 41 + 1), 42);
        loader.println("a line");
        loader.finish_and_clear();
    }

    #[test]
    fn eta_is_none_until_there_is_something_to_extrapolate() {
        let loader = Loader::builder().total(100).art(Art::parse("##")).start();
        assert_eq!(loader.eta(), None, "no progress yet");
        loader.set(100);
        assert_eq!(loader.eta(), None, "already done");
        loader.finish_and_clear();
    }

    #[test]
    fn spinner_has_no_length_and_no_eta() {
        let loader = Loader::builder().art(Art::parse("##")).start();
        assert_eq!(loader.length(), 0);
        assert_eq!(loader.eta(), None);
        loader.finish_and_clear();
    }

    #[test]
    fn easing_shapes_the_reported_progress() {
        let art = Art::parse("####");
        let build = |easing| {
            let ordering = Directional::default();
            Shared {
                pos: AtomicU64::new(50),
                total: AtomicU64::new(100),
                state: AtomicU8::new(RUNNING),
                drawn_lines: AtomicU16::new(0),
                message: Mutex::new(String::new()),
                painting: Mutex::new(()),
                ranks: ordering.rank(&art),
                art: art.clone(),
                style: Style::default(),
                easing,
                started: Instant::now(),
            }
        };
        assert!((build(Easing::Linear).progress(0.0) - 0.5).abs() < 1e-6);
        assert!(
            build(Easing::EaseOutCubic).progress(0.0) > 0.8,
            "ease-out should be well ahead at the midpoint"
        );
    }

    /// The inline block has to be exactly cursor-neutral: whatever it steps down
    /// while drawing, the next frame steps back up. One line out either way and
    /// the block walks up or down the screen once per frame at 30 fps, smearing
    /// the scrollback permanently. Rendering into a buffer is how this stays
    /// tested without a TTY.
    #[test]
    fn an_inline_frame_is_cursor_neutral() {
        let art = Art::parse("##\n##\n##");
        let ordering = Directional::default();
        let shared = Shared {
            pos: AtomicU64::new(50),
            total: AtomicU64::new(100),
            state: AtomicU8::new(RUNNING),
            drawn_lines: AtomicU16::new(0),
            message: Mutex::new("caption".into()),
            painting: Mutex::new(()),
            ranks: ordering.rank(&art),
            art: art.clone(),
            style: Style::monochrome(),
            easing: Easing::Linear,
            started: Instant::now(),
        };
        let viewport = Viewport { cols: 40, rows: 20 };

        let mut first = Vec::new();
        draw(&mut first, &shared, viewport, Placement::Inline, 0.5, 0.0).unwrap();
        // Three art rows plus the caption: a four-line block whose last line the
        // cursor is sitting on.
        assert_eq!(shared.drawn_lines.load(Relaxed), 4);
        let down = String::from_utf8(first)
            .unwrap()
            .matches("\u{1b}[1E")
            .count();
        assert_eq!(down, 3, "one step down per art row, none after the caption");

        let mut second = Vec::new();
        draw(&mut second, &shared, viewport, Placement::Inline, 0.6, 0.0).unwrap();
        let text = String::from_utf8(second).unwrap();
        assert!(
            text.contains("\u{1b}[3F"),
            "the next frame must step back over exactly what the last one stepped down"
        );
        assert!(
            !text.contains("\u{1b}[4F"),
            "stepping back one line too far"
        );
    }

    #[test]
    fn wrapped_reader_advances_the_loader() {
        let loader = Loader::builder().total(11).art(Art::parse("##")).start();
        let mut reader = loader.wrap_read(&b"hello world"[..]);
        let mut sink = Vec::new();
        io::copy(&mut reader, &mut sink).unwrap();
        assert_eq!(loader.position(), 11);
        loader.finish_and_clear();
    }
}
