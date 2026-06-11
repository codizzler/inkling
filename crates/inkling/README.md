# inkling

**Reveal arbitrary ASCII art as a progress indicator.**

[Repository](https://github.com/codizzler/inkling) · [Docs](https://docs.rs/inkling-loader)

![Inkling revealing ASCII art in rainbow as a task runs](https://raw.githubusercontent.com/codizzler/inkling/main/docs/demo-hero.gif)

Inkling maps progress onto the order a picture's glyphs appear. A normal bar fills a line;
Inkling paints a drawing, one glyph at a time, as your task runs.

Published as `inkling-loader` because the short name is taken; the import path is `inkling`.

```toml
[dependencies]
inkling-loader = "0.1"
```

```rust
use inkling::prelude::*;

let loader = Loader::new(1000);
loader.set_message("Downloading");
for _ in 0..1000 {
    do_a_slice_of_work();
    loader.inc(1);
}
loader.finish();
```

A background thread repaints a live reveal at about 30 fps, inline in the terminal. Updates
are lock free, so any thread can report progress. Wrap an iterator or a reader and it advances
itself; without a total, `Loader::spinner()` runs an indeterminate reveal. Off a TTY it prints
the finished art once instead of animating, so the same code is correct in a pipe or in CI.

## How it reveals

Every ink cell gets a **reveal rank** in `0..=1`, and a cell is visible exactly when
`rank <= progress`. Rank is fixed and monotonic, so the reveal is seekable, resumable, and a
pure function of progress. The one pluggable seam is the `Ordering` trait: `Directional` (a
clean wipe, the default), `Geodesic` (trace the spine, so a serpent paints tip to tail), or
your own.

The core (`art`, `rank`, `ordering`, `easing`, `frame`) is pure `std` with zero dependencies
and builds with `--no-default-features`. Only the live terminal renderer pulls in
[`crossterm`](https://crates.io/crates/crossterm), behind the default `terminal` feature.

## The inkling family

This crate, `inkling-loader`, is the Rust library. The same engine ships five ways:

- **`inkling-loader`** (this crate), the Rust library. `cargo add inkling-loader`, import as `inkling`.
- **`inkling-cli`**, the `inkling` command, so bash, Make, or any language can drive a reveal
  through a pipe. `cargo install inkling-cli`.
- **`inkling-loader` on PyPI**, the Python package. `pip install inkling-loader`.
- **`inkling-loader` on npm**, the Node addon. `npm install inkling-loader`.
- **`inkling-wasm` on npm**, the WebAssembly build for the browser.

## More

Full usage, the geodesic write-up, the `inkling` CLI, and the Python package live in the
[repository README](https://github.com/codizzler/inkling#readme). ASCII art is from the
community at [asciiart.eu](https://www.asciiart.eu/).

## License

MIT. See [LICENSE-MIT](https://github.com/codizzler/inkling/blob/main/LICENSE-MIT).
