//! Node bindings for inkling, built on the same Rust core.
//!
//! ```js
//! const { Loader } = require("inkling-loader");
//!
//! const bar = new Loader({ total: items.length, rainbow: true });
//! for (const item of items) {
//!   work(item);
//!   bar.inc();
//! }
//! bar.finish();
//! ```
//!
//! Or, with explicit resource management, so it always finishes:
//!
//! ```js
//! using bar = new Loader({ total: items.length });
//! ```

use std::sync::Mutex;

use inkling_core::easing::Easing;
use inkling_core::ordering::{Directional, Geodesic, Scanline, StartHint};
use inkling_core::render::{ColorDepth, Palette, Style};
use inkling_core::{Art, Loader as CoreLoader};
use napi_derive::napi;

fn invalid(message: String) -> napi::Error {
    napi::Error::new(napi::Status::InvalidArg, message)
}

/// Parse `rrggbb` (with or without a leading `#`) into an RGB triple.
fn parse_hex(value: &str, field: &str) -> napi::Result<(u8, u8, u8)> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let bad = || invalid(format!("{field} must look like 'ffe28a', got '{value}'"));
    if hex.len() != 6 {
        return Err(bad());
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).map_err(|_| bad());
    Ok((byte(0)?, byte(2)?, byte(4)?))
}

fn parse_easing(name: &str) -> napi::Result<Easing> {
    match name.replace('_', "-").as_str() {
        "linear" => Ok(Easing::Linear),
        "ease-out-cubic" => Ok(Easing::EaseOutCubic),
        "ease-out-quint" => Ok(Easing::EaseOutQuint),
        "ease-in-out-cubic" => Ok(Easing::EaseInOutCubic),
        other => Err(invalid(format!(
            "unknown easing '{other}': expected linear, ease-out-cubic, \
             ease-out-quint, or ease-in-out-cubic"
        ))),
    }
}

fn parse_color_depth(name: &str) -> napi::Result<ColorDepth> {
    match name {
        "auto" => Ok(ColorDepth::detect()),
        "truecolor" | "24bit" => Ok(ColorDepth::TrueColor),
        "256" => Ok(ColorDepth::Ansi256),
        "16" => Ok(ColorDepth::Ansi16),
        "none" | "mono" => Ok(ColorDepth::Mono),
        other => Err(invalid(format!(
            "unknown color '{other}': expected auto, truecolor, 256, 16, or none"
        ))),
    }
}

fn parse_start(name: &str) -> napi::Result<StartHint> {
    match name.replace('_', "-").as_str() {
        "top-left" | "topleft" => Ok(StartHint::TopLeft),
        "bottom" => Ok(StartHint::Bottom),
        "topological" => Ok(StartHint::Topological),
        other => Err(invalid(format!(
            "unknown start '{other}': expected top-left, bottom, or topological"
        ))),
    }
}

/// Options for a [`Loader`]. All fields are optional.
#[napi(object)]
#[derive(Default)]
pub struct LoaderOptions {
    /// Units of work; omit for an indeterminate spinner.
    pub total: Option<f64>,
    /// ASCII art string to reveal (default: the built-in dragon).
    pub art: Option<String>,
    /// Path to an ASCII art file (overrides `art`).
    pub art_path: Option<String>,
    /// Reveal path: `auto`, `geodesic`, `scanline`, `reading`, `ltr`, or `rtl`.
    pub ordering: Option<String>,
    /// Lolcat-style rainbow palette.
    pub rainbow: Option<bool>,
    /// Shorthand for `ordering: "geodesic"`.
    pub geodesic: Option<bool>,
    /// Shorthand for `ordering: "reading"`.
    pub reading: Option<bool>,
    /// Palette tuned for a light terminal background.
    pub light: Option<bool>,
    /// Colour depth: `auto`, `truecolor`, `256`, `16`, or `none`.
    pub color: Option<String>,
    /// Leading-edge colour, as `"ffe28a"`.
    pub head: Option<String>,
    /// Settled-ink colour, as `"7886a8"`.
    pub body: Option<String>,
    /// Width of the glow in rank units; `0` disables it.
    pub feather: Option<f64>,
    /// Timing curve applied to the reported fraction.
    pub easing: Option<String>,
    /// Which spine tip a geodesic reveal starts from.
    pub start: Option<String>,
    /// Blank cells the spine may step across; `0` disables bridging.
    pub bridge: Option<u32>,
    /// Caption shown beneath the art.
    pub message: Option<String>,
}

/// A live ASCII-art progress reveal.
///
/// Call [`finish`](Loader::finish) when the work is done, or declare it with
/// `using` so it finishes on scope exit.
#[napi]
pub struct Loader {
    inner: Mutex<Option<CoreLoader>>,
}

#[napi]
impl Loader {
    /// Create and start a loader.
    #[napi(constructor)]
    pub fn new(options: Option<LoaderOptions>) -> napi::Result<Self> {
        let o = options.unwrap_or_default();

        let mut style = if o.light.unwrap_or(false) {
            Style::light()
        } else {
            Style::default()
        };
        if o.rainbow.unwrap_or(false) {
            style.palette = Palette::Rainbow;
        }
        if let Some(c) = &o.color {
            style.depth = parse_color_depth(c)?;
        }
        if let Some(c) = &o.head {
            style.head = parse_hex(c, "head")?;
        }
        if let Some(c) = &o.body {
            style.body = parse_hex(c, "body")?;
        }
        if let Some(f) = o.feather {
            if !(0.0..=1.0).contains(&f) {
                return Err(invalid(format!("feather must be in 0..=1, got {f}")));
            }
            style.feather = f as f32;
        }

        let mut builder = CoreLoader::builder().style(style);
        if let Some(t) = o.total {
            if !t.is_finite() || t < 0.0 {
                return Err(invalid(format!("total must be a non-negative number, got {t}")));
            }
            builder = builder.total(t as u64);
        }
        if let Some(m) = o.message {
            builder = builder.message(m);
        }
        if let Some(e) = &o.easing {
            builder = builder.easing(parse_easing(e)?);
        }

        let text = match (o.art_path, o.art) {
            (Some(path), _) => Some(
                std::fs::read_to_string(&path)
                    .map_err(|e| napi::Error::from_reason(format!("could not read {path}: {e}")))?,
            ),
            (None, Some(text)) => Some(text),
            (None, None) => None,
        };
        if let Some(text) = text {
            let parsed = Art::parse(&text);
            if parsed.is_empty() {
                return Err(invalid("the art has no ink in it".into()));
            }
            builder = builder.art(parsed);
        }

        // The explicit `ordering` wins; the older boolean shorthands still work.
        let spine = Geodesic {
            start: o.start.as_deref().map(parse_start).transpose()?.unwrap_or_default(),
            bridge: o
                .bridge
                .map(|b| b.min(u16::MAX as u32) as u16)
                .unwrap_or(Geodesic::default().bridge),
        };
        let choice = o.ordering.clone().unwrap_or_else(|| {
            if o.geodesic.unwrap_or(false) {
                "geodesic".into()
            } else if o.reading.unwrap_or(false) {
                "reading".into()
            } else {
                "auto".into()
            }
        });
        builder = match choice.as_str() {
            "auto" => builder.ordering(Directional::default()),
            "geodesic" => builder.ordering(spine),
            "scanline" => builder.ordering(Scanline),
            "reading" => builder.ordering(Directional::reading()),
            "ltr" => builder.ordering(Directional::ltr()),
            "rtl" => builder.ordering(Directional::rtl()),
            other => {
                return Err(invalid(format!(
                    "unknown ordering '{other}': expected auto, geodesic, scanline, \
                     reading, ltr, or rtl"
                )))
            }
        };

        Ok(Loader {
            inner: Mutex::new(Some(builder.start())),
        })
    }

    /// Run `f` against the live loader, if it has not been finished yet.
    fn with<T>(&self, f: impl FnOnce(&CoreLoader) -> T) -> Option<T> {
        let guard = self.inner.lock().ok()?;
        guard.as_ref().map(f)
    }

    /// Advance the position by `delta` (default 1).
    #[napi]
    pub fn inc(&self, delta: Option<f64>) {
        let delta = delta.unwrap_or(1.0);
        if delta.is_finite() && delta > 0.0 {
            self.with(|l| l.inc(delta as u64));
        }
    }

    /// Set the absolute position.
    #[napi]
    pub fn set(&self, pos: f64) {
        if pos.is_finite() && pos >= 0.0 {
            self.with(|l| l.set(pos as u64));
        }
    }

    /// Change the total amount of work.
    #[napi]
    pub fn set_length(&self, total: f64) {
        if total.is_finite() && total >= 0.0 {
            self.with(|l| l.set_length(total as u64));
        }
    }

    /// Set the caption shown beneath the art.
    #[napi]
    pub fn set_message(&self, message: String) {
        self.with(|l| l.set_message(message));
    }

    /// Print a line above the reveal, which redraws beneath it.
    ///
    /// Use this instead of `console.log` while a loader is live: it lifts the art
    /// out of the way first, so the output does not land inside the picture.
    #[napi]
    pub fn println(&self, line: String) {
        self.with(|l| l.println(line));
    }

    /// The current position.
    #[napi(getter)]
    pub fn position(&self) -> f64 {
        self.with(|l| l.position()).unwrap_or(0) as f64
    }

    /// The total amount of work, or 0 when indeterminate.
    #[napi(getter)]
    pub fn length(&self) -> f64 {
        self.with(|l| l.length()).unwrap_or(0) as f64
    }

    /// Seconds since the loader started.
    #[napi(getter)]
    pub fn elapsed(&self) -> f64 {
        self.with(|l| l.elapsed().as_secs_f64()).unwrap_or(0.0)
    }

    /// Average units of work per second so far.
    #[napi(getter)]
    pub fn rate(&self) -> f64 {
        self.with(|l| l.rate()).unwrap_or(0.0)
    }

    /// Estimated seconds remaining, or null when it cannot be estimated.
    #[napi(getter)]
    pub fn eta(&self) -> Option<f64> {
        self.with(|l| l.eta().map(|d| d.as_secs_f64())).flatten()
    }

    /// Fill the art, leave it on screen, and restore the terminal.
    #[napi]
    pub fn finish(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some(l) = guard.take() {
                l.finish();
            }
        }
    }

    /// Finish and erase the art from the screen.
    #[napi]
    pub fn finish_and_clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some(l) = guard.take() {
                l.finish_and_clear();
            }
        }
    }

    /// `Symbol.dispose`, so `using bar = new Loader(...)` restores the terminal on
    /// scope exit even if the block throws. Without it the loader would only be
    /// cleaned up whenever the garbage collector happened to reach it.
    #[napi(js_name = "[Symbol.dispose]")]
    pub fn dispose(&self) {
        self.finish();
    }
}
