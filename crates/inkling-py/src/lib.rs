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

use pyo3::exceptions::PyIOError;
use pyo3::prelude::*;

use inkling_core::ordering::{Directional, Geodesic};
use inkling_core::render::Style;
use inkling_core::{Art, Loader as CoreLoader};

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
    ///     rainbow: lolcat-style palette.
    ///     geodesic: trace the spine instead of a directional wipe.
    ///     reading: wipe along the locale's reading direction.
    ///     message: caption shown beneath the art.
    #[new]
    #[pyo3(signature = (
        total=None, *, art=None, art_path=None,
        rainbow=false, geodesic=false, reading=false, message=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        total: Option<u64>,
        art: Option<String>,
        art_path: Option<String>,
        rainbow: bool,
        geodesic: bool,
        reading: bool,
        message: Option<String>,
    ) -> PyResult<Self> {
        let style = if rainbow {
            Style::rainbow()
        } else {
            Style::default()
        };
        let mut builder = CoreLoader::builder().style(style);
        if let Some(t) = total {
            builder = builder.total(t);
        }
        if let Some(m) = message {
            builder = builder.message(m);
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
            builder = builder.art(Art::parse(&text));
        }
        if geodesic {
            builder = builder.ordering(Geodesic::default());
        } else if reading {
            builder = builder.ordering(Directional::reading());
        }
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
    fn position(&self) -> u64 {
        self.inner.as_ref().map_or(0, |l| l.position())
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
