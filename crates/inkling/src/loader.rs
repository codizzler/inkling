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
//!
//! The handle is cheap to clone (via [`handle`](Loader::handle)) and `Send + Sync`,
//! so worker threads can report progress while the render thread owns the terminal,
//! which keeps all drawing on one thread and free of races. When stdout is not a
//! TTY the loader does not animate; it prints the finished art once on `finish`, so
//! logs and CI still show the result.

use std::io::{self, IsTerminal, Read, Write};
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};
use std::sync::atomic::{AtomicU64, AtomicU8};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::{
    cursor::{Hide, MoveToNextLine, MoveToPreviousLine, Show},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};

use crate::art::Art;
use crate::ordering::{Geodesic, Ordering};
use crate::render::Style;

/// The built-in art used when you do not supply your own.
const DEFAULT_ART: &str = include_str!("../assets/dragon.txt");
const FPS: u64 = 30;

// Loader lifecycle, stored in `Shared::state`.
const RUNNING: u8 = 0;
const FINISH_KEEP: u8 = 1; // complete the art and leave it on screen
const FINISH_CLEAR: u8 = 2; // complete and erase the art

/// State shared between the public handles and the render thread.
struct Shared {
    pos: AtomicU64,
    total: AtomicU64, // 0 means indeterminate (spinner)
    state: AtomicU8,
    message: Mutex<String>,
    art: Art,
    ranks: crate::rank::RankMap,
    style: Style,
}

impl Shared {
    fn inc(&self, delta: u64) {
        self.pos.fetch_add(delta, Relaxed);
    }
    fn set(&self, pos: u64) {
        self.pos.store(pos, Relaxed);
    }
    fn set_message(&self, msg: String) {
        if let Ok(mut guard) = self.message.lock() {
            *guard = msg;
        }
    }
}

/// A living progress reveal.
///
/// Create one with [`Loader::new`], advance it, and [`finish`](Loader::finish).
/// Dropping the last handle finishes it for you, so the terminal is always
/// restored. Not `Clone`; for cross-thread updates take a [`Handle`].
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

    /// Configure a loader with custom art, ordering, style, or message.
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
        ProgressReader {
            inner: reader,
            handle: self.handle(),
        }
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
                crate::frame::to_string(&self.shared.art, &self.shared.ranks, 1.0)
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
}

/// Builder for a customised [`Loader`].
pub struct Builder {
    total: u64,
    art: Option<Art>,
    ordering: Box<dyn Ordering>,
    style: Style,
    message: String,
}

impl Builder {
    fn new() -> Self {
        Builder {
            total: 0,
            art: None,
            ordering: Box::new(Geodesic::default()),
            style: Style::default(),
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

    /// The ordering that decides the reveal path. Defaults to [`Geodesic`].
    pub fn ordering(mut self, ordering: impl Ordering + 'static) -> Self {
        self.ordering = Box::new(ordering);
        self
    }

    /// Colours and frontier glow.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// A short caption shown beneath the art.
    pub fn message<S: Into<String>>(mut self, message: S) -> Self {
        self.message = message.into();
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
            message: Mutex::new(self.message),
            art,
            ranks,
            style: self.style,
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

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.handle.inc(n as u64);
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// The render thread: an inline, in-place reveal at ~30 fps.
// ---------------------------------------------------------------------------

fn run(shared: Arc<Shared>) {
    let mut out = io::stdout();
    let _ = execute!(out, Hide); // also enables VT processing on Windows
    let lines = shared.art.height() + 1; // art rows plus one caption row
    let frame = Duration::from_millis(1000 / FPS);
    let start = Instant::now();
    let mut displayed = 0.0f32;
    let mut first = true;

    loop {
        let finishing = shared.state.load(Acquire) != RUNNING;
        let total = shared.total.load(Relaxed);
        let pos = shared.pos.load(Relaxed);
        let t = start.elapsed().as_secs_f32();

        let target = if total == 0 {
            // Spinner: a smooth breathing reveal between 10% and 100%.
            0.1 + 0.9 * (0.5 - 0.5 * (t * 1.5).cos())
        } else {
            (pos as f32 / total as f32).clamp(0.0, 1.0)
        };
        displayed += (target - displayed) * 0.3; // glide toward the true value
        let progress = if finishing { 1.0 } else { displayed };

        let drawn = if finishing && shared.state.load(Relaxed) == FINISH_CLEAR {
            clear_block(&mut out, lines, first)
        } else {
            draw_frame(&mut out, &shared, progress, t, first)
        };
        let _ = drawn;
        first = false;

        if finishing {
            let _ = execute!(out, Show);
            let _ = out.flush();
            break;
        }
        thread::sleep(frame);
    }
}

fn draw_frame(
    out: &mut io::Stdout,
    shared: &Shared,
    progress: f32,
    t: f32,
    first: bool,
) -> io::Result<()> {
    let art = &shared.art;
    let (w, h) = (art.width(), art.height());
    let style = &shared.style;
    let cols = terminal::size().map(|(c, _)| c).unwrap_or(80);

    if !first {
        queue!(out, MoveToPreviousLine(h + 1))?;
    }
    for y in 0..h {
        queue!(out, Clear(ClearType::CurrentLine))?;
        let mut last: Option<(u8, u8, u8)> = None;
        for x in 0..w {
            match shared.ranks.rank_at(x, y) {
                Some(r) if r <= progress => {
                    if style.color {
                        let c = crate::render::cell_rgb(style, progress, r, x, y, t);
                        if last != Some(c) {
                            queue!(
                                out,
                                SetForegroundColor(Color::Rgb {
                                    r: c.0,
                                    g: c.1,
                                    b: c.2
                                })
                            )?;
                            last = Some(c);
                        }
                    }
                    queue!(out, Print(art.glyph(x, y)))?;
                }
                _ => {
                    if last.take().is_some() {
                        queue!(out, ResetColor)?;
                    }
                    queue!(out, Print(' '))?;
                }
            }
        }
        if last.is_some() {
            queue!(out, ResetColor)?;
        }
        queue!(out, MoveToNextLine(1))?;
    }

    // Caption row.
    queue!(out, Clear(ClearType::CurrentLine))?;
    let msg = shared
        .message
        .lock()
        .ok()
        .map(|m| m.clone())
        .unwrap_or_default();
    if !msg.is_empty() {
        let shown: String = msg.chars().take(cols.saturating_sub(1) as usize).collect();
        if style.color {
            queue!(
                out,
                SetForegroundColor(Color::Rgb {
                    r: 120,
                    g: 134,
                    b: 168
                })
            )?;
        }
        queue!(out, Print(shown))?;
        if style.color {
            queue!(out, ResetColor)?;
        }
    }
    queue!(out, MoveToNextLine(1))?;
    out.flush()
}

fn clear_block(out: &mut io::Stdout, lines: u16, first: bool) -> io::Result<()> {
    if !first {
        queue!(out, MoveToPreviousLine(lines))?;
    }
    for _ in 0..lines {
        queue!(out, Clear(ClearType::CurrentLine), MoveToNextLine(1))?;
    }
    queue!(out, MoveToPreviousLine(lines))?;
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
        loader.finish_and_clear();
    }

    #[test]
    fn iterator_yields_every_item() {
        let loader = Loader::builder().total(5).art(Art::parse("##")).start();
        let collected: Vec<i32> = (0..5).inkling_with(loader).collect();
        assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    }
}
