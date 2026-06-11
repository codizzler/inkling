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

use std::sync::Mutex;

use inkling_core::ordering::{Directional, Geodesic};
use inkling_core::render::Style;
use inkling_core::{Art, Loader as CoreLoader};
use napi_derive::napi;

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
    /// Lolcat-style rainbow palette.
    pub rainbow: Option<bool>,
    /// Trace the spine instead of a directional wipe.
    pub geodesic: Option<bool>,
    /// Wipe along the locale's reading direction.
    pub reading: Option<bool>,
    /// Caption shown beneath the art.
    pub message: Option<String>,
}

/// A live ASCII-art progress reveal. Use it as a context object, or call
/// [`finish`](Loader::finish) yourself.
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
        let style = if o.rainbow.unwrap_or(false) {
            Style::rainbow()
        } else {
            Style::default()
        };
        let mut builder = CoreLoader::builder().style(style);
        if let Some(t) = o.total {
            builder = builder.total(t as u64);
        }
        if let Some(m) = o.message {
            builder = builder.message(m);
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
            builder = builder.art(Art::parse(&text));
        }
        if o.geodesic.unwrap_or(false) {
            builder = builder.ordering(Geodesic::default());
        } else if o.reading.unwrap_or(false) {
            builder = builder.ordering(Directional::reading());
        }
        Ok(Loader {
            inner: Mutex::new(Some(builder.start())),
        })
    }

    /// Advance the position by `delta` (default 1).
    #[napi]
    pub fn inc(&self, delta: Option<f64>) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(l) = guard.as_ref() {
                l.inc(delta.unwrap_or(1.0) as u64);
            }
        }
    }

    /// Set the absolute position.
    #[napi]
    pub fn set(&self, pos: f64) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(l) = guard.as_ref() {
                l.set(pos as u64);
            }
        }
    }

    /// Change the total amount of work.
    #[napi]
    pub fn set_length(&self, total: f64) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(l) = guard.as_ref() {
                l.set_length(total as u64);
            }
        }
    }

    /// Set the caption shown beneath the art.
    #[napi]
    pub fn set_message(&self, message: String) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(l) = guard.as_ref() {
                l.set_message(message);
            }
        }
    }

    /// The current position.
    #[napi]
    pub fn position(&self) -> f64 {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|l| l.position()))
            .unwrap_or(0) as f64
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
}
