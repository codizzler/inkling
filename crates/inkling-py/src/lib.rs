//! Python bindings for inkling, built on the same pure Rust core.
//!
//! ```python
//! from inkling import Loader
//!
//! with Loader(total=len(items), rainbow=True) as bar:
//!     for it in items:
//!         work(it)
//!         bar.inc()
//! ```

use pyo3::exceptions::{PyIOError, PyValueError};
use pyo3::prelude::*;

use inkling_core::easing::Easing;
use inkling_core::ordering::{Directional, Geodesic, Scanline, StartHint};
use inkling_core::render::{ColorDepth, Palette, Style};
use inkling_core::{Art, Loader as CoreLoader};

/// Parse `rrggbb` (with or without a leading `#`) into an RGB triple.
fn parse_hex(value: &str, field: &str) -> PyResult<(u8, u8, u8)> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let bad = || PyValueError::new_err(format!("{field} must look like 'ffe28a', got {value:?}"));
    if hex.len() != 6 {
        return Err(bad());
    }
    let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).map_err(|_| bad());
    Ok((byte(0)?, byte(2)?, byte(4)?))
}

fn parse_easing(name: &str) -> PyResult<Easing> {
    match name.replace('_', "-").as_str() {
        "linear" => Ok(Easing::Linear),
        "ease-out-cubic" => Ok(Easing::EaseOutCubic),
        "ease-out-quint" => Ok(Easing::EaseOutQuint),
        "ease-in-out-cubic" => Ok(Easing::EaseInOutCubic),
        other => Err(PyValueError::new_err(format!(
            "unknown easing {other:?}: expected one of linear, ease-out-cubic, \
             ease-out-quint, ease-in-out-cubic"
        ))),
    }
}

fn parse_color_depth(name: &str) -> PyResult<ColorDepth> {
    match name {
        "auto" => Ok(ColorDepth::detect()),
        "truecolor" | "24bit" => Ok(ColorDepth::TrueColor),
        "256" => Ok(ColorDepth::Ansi256),
        "16" => Ok(ColorDepth::Ansi16),
        "none" | "mono" => Ok(ColorDepth::Mono),
        other => Err(PyValueError::new_err(format!(
            "unknown color {other:?}: expected one of auto, truecolor, 256, 16, none"
        ))),
    }
}

fn parse_start(name: &str) -> PyResult<StartHint> {
    match name.replace('_', "-").as_str() {
        "top-left" | "topleft" => Ok(StartHint::TopLeft),
        "bottom" => Ok(StartHint::Bottom),
        "topological" => Ok(StartHint::Topological),
        other => Err(PyValueError::new_err(format!(
            "unknown start {other:?}: expected one of top-left, bottom, topological"
        ))),
    }
}

/// A live ASCII-art progress reveal.
///
/// Use it as a context manager so it always finishes cleanly, or call
/// :meth:`finish` yourself.
#[pyclass]
struct Loader {
    inner: Option<CoreLoader>,
}

#[pymethods]
impl Loader {
    /// Create and start a loader.
    ///
    /// Args:
    ///     total: units of work; omit for an indeterminate spinner.
    ///     art: ASCII art string to reveal (default: the built-in dragon).
    ///     art_path: path to an ASCII art file (overrides ``art``).
    ///     ordering: ``auto`` (default), ``geodesic``, ``scanline``, ``reading``,
    ///         ``ltr``, or ``rtl``.
    ///     rainbow: lolcat-style palette.
    ///     geodesic: shorthand for ``ordering="geodesic"``.
    ///     reading: shorthand for ``ordering="reading"``.
    ///     light: palette tuned for a light terminal background.
    ///     color: colour depth, one of ``auto``, ``truecolor``, ``256``, ``16``,
    ///         ``none``.
    ///     head: leading-edge colour, as ``"ffe28a"``.
    ///     body: settled-ink colour, as ``"7886a8"``.
    ///     feather: width of the glow in rank units; ``0`` disables it.
    ///     easing: timing curve applied to the reported fraction.
    ///     start: which spine tip a geodesic reveal starts from.
    ///     bridge: blank cells the spine may step across; ``0`` disables it.
    ///     message: caption shown beneath the art.
    #[new]
    #[pyo3(signature = (
        total=None, *, art=None, art_path=None, ordering=None,
        rainbow=false, geodesic=false, reading=false, light=false,
        color=None, head=None, body=None, feather=None,
        easing=None, start=None, bridge=None, message=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        total: Option<u64>,
        art: Option<String>,
        art_path: Option<String>,
        ordering: Option<&str>,
        rainbow: bool,
        geodesic: bool,
        reading: bool,
        light: bool,
        color: Option<&str>,
        head: Option<&str>,
        body: Option<&str>,
        feather: Option<f32>,
        easing: Option<&str>,
        start: Option<&str>,
        bridge: Option<u16>,
        message: Option<String>,
    ) -> PyResult<Self> {
        let mut style = if light { Style::light() } else { Style::default() };
        if rainbow {
            style.palette = Palette::Rainbow;
        }
        if let Some(c) = color {
            style.depth = parse_color_depth(c)?;
        }
        if let Some(c) = head {
            style.head = parse_hex(c, "head")?;
        }
        if let Some(c) = body {
            style.body = parse_hex(c, "body")?;
        }
        if let Some(f) = feather {
            if !(0.0..=1.0).contains(&f) {
                return Err(PyValueError::new_err(format!(
                    "feather must be in 0..=1, got {f}"
                )));
            }
            style.feather = f;
        }

        let mut builder = CoreLoader::builder().style(style);
        if let Some(t) = total {
            builder = builder.total(t);
        }
        if let Some(m) = message {
            builder = builder.message(m);
        }
        if let Some(e) = easing {
            builder = builder.easing(parse_easing(e)?);
        }

        let text = match (art_path, art) {
            (Some(path), _) => Some(
                std::fs::read_to_string(&path)
                    .map_err(|e| PyIOError::new_err(format!("could not read {path}: {e}")))?,
            ),
            (None, Some(text)) => Some(text),
            (None, None) => None,
        };
        if let Some(text) = text {
            let parsed = Art::parse(&text);
            if parsed.is_empty() {
                return Err(PyValueError::new_err("the art has no ink in it"));
            }
            builder = builder.art(parsed);
        }

        // The explicit `ordering=` wins; the older boolean shorthands still work.
        let spine = Geodesic {
            start: start.map(parse_start).transpose()?.unwrap_or_default(),
            bridge: bridge.unwrap_or(Geodesic::default().bridge),
        };
        let choice = ordering.unwrap_or(if geodesic {
            "geodesic"
        } else if reading {
            "reading"
        } else {
            "auto"
        });
        builder = match choice {
            "auto" => builder.ordering(Directional::default()),
            "geodesic" => builder.ordering(spine),
            "scanline" => builder.ordering(Scanline),
            "reading" => builder.ordering(Directional::reading()),
            "ltr" => builder.ordering(Directional::ltr()),
            "rtl" => builder.ordering(Directional::rtl()),
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown ordering {other:?}: expected one of auto, geodesic, \
                     scanline, reading, ltr, rtl"
                )))
            }
        };

        Ok(Loader {
            inner: Some(builder.start()),
        })
    }

    /// Advance the position by ``delta`` (default 1).
    #[pyo3(signature = (delta=1))]
    fn inc(&self, delta: u64) {
        if let Some(l) = &self.inner {
            l.inc(delta);
        }
    }

    /// Set the absolute position.
    fn set(&self, pos: u64) {
        if let Some(l) = &self.inner {
            l.set(pos);
        }
    }

    /// Change the total amount of work.
    fn set_length(&self, total: u64) {
        if let Some(l) = &self.inner {
            l.set_length(total);
        }
    }

    /// Set the caption shown beneath the art.
    fn set_message(&self, message: String) {
        if let Some(l) = &self.inner {
            l.set_message(message);
        }
    }

    /// The current position.
    #[getter]
    fn position(&self) -> u64 {
        self.inner.as_ref().map_or(0, |l| l.position())
    }

    /// The total amount of work, or 0 when indeterminate.
    #[getter]
    fn length(&self) -> u64 {
        self.inner.as_ref().map_or(0, |l| l.length())
    }

    /// Seconds since the loader started.
    #[getter]
    fn elapsed(&self) -> f64 {
        self.inner
            .as_ref()
            .map_or(0.0, |l| l.elapsed().as_secs_f64())
    }

    /// Average units of work per second so far.
    #[getter]
    fn rate(&self) -> f64 {
        self.inner.as_ref().map_or(0.0, |l| l.rate())
    }

    /// Estimated seconds remaining, or ``None`` when it cannot be estimated.
    #[getter]
    fn eta(&self) -> Option<f64> {
        self.inner
            .as_ref()
            .and_then(|l| l.eta())
            .map(|d| d.as_secs_f64())
    }

    /// Print a line above the reveal, which redraws beneath it.
    ///
    /// Use this instead of a bare ``print`` while a loader is live: it lifts the
    /// art out of the way first, so the output does not land inside the picture.
    fn println(&self, py: Python<'_>, line: &str) {
        if let Some(l) = &self.inner {
            py.allow_threads(|| l.println(line));
        }
    }

    /// Fill the art, leave it on screen, and restore the terminal.
    fn finish(&mut self, py: Python<'_>) {
        if let Some(l) = self.inner.take() {
            py.allow_threads(|| l.finish());
        }
    }

    /// Finish and erase the art from the screen.
    fn finish_and_clear(&mut self, py: Python<'_>) {
        if let Some(l) = self.inner.take() {
            py.allow_threads(|| l.finish_and_clear());
        }
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc=None, _tb=None))]
    fn __exit__(
        &mut self,
        py: Python<'_>,
        _exc_type: Option<PyObject>,
        _exc: Option<PyObject>,
        _tb: Option<PyObject>,
    ) -> bool {
        if let Some(l) = self.inner.take() {
            py.allow_threads(|| l.finish());
        }
        false // do not suppress exceptions
    }
}

/// inkling: reveal ASCII art as a progress indicator.
#[pymodule]
fn inkling(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Loader>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
