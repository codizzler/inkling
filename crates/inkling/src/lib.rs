//! # inkling
//!
//! Reveal arbitrary ASCII art as a progress indicator.
//!
//! A normal progress bar maps a scalar `0..=1` onto *how much of a line is
//! filled*. `inkling` generalises that to two dimensions: it maps progress onto
//! **the order in which the glyphs of a picture appear**. Give it a dragon and it
//! paints the dragon as your task runs.
//!
//! ## Quick start
//!
//! The easy front door is [`Loader`](loader::Loader): make one with a total,
//! advance it from anywhere, and a living reveal paints itself until you finish.
//!
//! ```no_run
//! use inkling::prelude::*;
//!
//! let loader = Loader::new(100);
//! for _ in 0..100 {
//!     // ... a slice of work ...
//!     loader.inc(1);
//! }
//! loader.finish();
//! ```
//!
//! Or wrap any iterator and forget about it:
//!
//! ```no_run
//! use inkling::prelude::*;
//!
//! for _item in (0..100).inkling() {
//!     // ... work ...
//! }
//! ```
//!
//! ## The one idea
//!
//! Everything turns on a single abstraction, the [`RankMap`]: every *ink* cell of
//! the art is assigned a **reveal rank** in `0..=1`, and a cell is visible exactly
//! when `rank <= progress`. Because rank is fixed and monotonic, the reveal can
//! never run backwards, any progress value renders directly (it is seekable and
//! resumable), and the whole thing is pure.
//!
//! *How* ranks are assigned is the single pluggable seam: an [`Ordering`]. The
//! flagship [`Geodesic`](ordering::Geodesic) ordering traces the "spine" of the
//! art and reveals along it, so a serpent paints from one tip to the other, around
//! every coil, with no per-art configuration.
//!
//! ```
//! use inkling::{Art, ordering::{Ordering, Geodesic}};
//!
//! let art = Art::parse("/\\__/\\\n\\____/");
//! let ranks = Geodesic::default().rank(&art);
//!
//! // The pure, dependency-free view: render the half-revealed frame as text.
//! let frame = inkling::frame::to_string(&art, &ranks, 0.5);
//! assert_eq!(frame.lines().count(), art.height() as usize);
//! ```

pub mod art;
pub mod easing;
pub mod frame;
pub mod ordering;
pub mod rank;

#[cfg(feature = "terminal")]
pub mod loader;
#[cfg(feature = "terminal")]
pub mod render;

pub use art::Art;
pub use easing::Easing;
pub use ordering::Ordering;
pub use rank::RankMap;

#[cfg(feature = "terminal")]
pub use loader::{Handle, Loader, ProgressIteratorExt};

/// The handful of imports most programs want, in one glob:
/// `use inkling::prelude::*;`.
///
/// Brings in `Loader`, the `.inkling()` iterator adaptor (via
/// `ProgressIteratorExt`), the thread-safe `Handle`, and `Art`.
pub mod prelude {
    pub use crate::Art;
    #[cfg(feature = "terminal")]
    pub use crate::{Handle, Loader, ProgressIteratorExt};
}
