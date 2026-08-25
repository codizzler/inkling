# Changelog

All notable changes to Inkling are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Every package in the
repository (`inkling-loader`, `inkling-cli`, the Python, Node and WASM bindings) shares
one version number and is released together.

## [0.2.0] - 2026-08-25

A correctness and durability pass over the whole workspace. The rank-map model is
unchanged; almost everything wrapped around it moved.

### Added

- `Loader::println` and `Loader::suspend`: print a line above a live reveal, or clear it
  for the duration of a closure that writes to the terminal itself. The standard
  progress-bar affordance, and previously the most conspicuous omission.
- `Loader::elapsed`, `rate`, and `eta`.
- `Handle::wrap_read`, so a worker thread can wrap a reader.
- `Builder::easing`, exposing the `Easing` curves that were public but unreachable from
  the front door.
- `ColorDepth`: terminal colour depth is detected from `COLORTERM` and `TERM`, and the
  glow and rainbow palettes are quantized onto 256- and 16-colour terminals instead of
  assuming truecolor.
- `Style::light`, a palette tuned for a light terminal background, and `Style::monochrome`.
- `Directional::ltr` and `Directional::rtl` for explicit reading direction, since
  `reading()` cannot detect a locale on Windows.
- A `width` module (`glyph_cols`, `str_cols`, `truncate_to_cols`) as the one place display
  width is decided.
- The CLI grew `--version`, `--scanline`, `--ltr`, `--rtl`, `--start`, `--bridge`,
  `--easing`, `--light`, `--no-color`, `--color`, `--head`, `--body`, and `--feather`.
- The Python and Node bindings expose the full style, ordering, easing and geodesic
  surface, plus `println`, `elapsed`, `rate` and `eta`. Python ships `py.typed` and
  `inkling.pyi`; Node implements `Symbol.dispose`.
- `scripts/check-versions.sh`, run in CI before any publish, so a tag can never disagree
  with the manifests.

### Fixed

- **The terminal is restored on Ctrl+C and on panic.** A `SIGINT`/`SIGTERM` handler on
  unix and a console control handler on Windows show the cursor and leave the alternate
  screen before the process dies, and a chained panic hook restores before the message is
  printed. Previously either one left an invisible cursor behind.
- **Drawing is clipped to the viewport, on both axes.** Art taller than the terminal no
  longer repaints the bottom row over and over, and art wider than it no longer wraps and
  desynchronizes the inline renderer's cursor arithmetic, which used to smear the
  scrollback permanently. The default dragon on a default-height terminal hit this.
- **Windows gets colour again.** Every renderer now writes escape sequences directly
  rather than routing colour through the Win32 console API, which is what makes them
  testable against a plain writer, but a Windows console starts with
  `ENABLE_VIRTUAL_TERMINAL_PROCESSING` off. It is switched on when the terminal is first
  taken over, matching what the README has always claimed.
- **The inline block no longer walks up the screen.** It stepped back by the block's full
  height when the cursor was already sitting on the block's last line, so at 30 fps the
  art climbed one row per frame, overwriting whatever was printed above it and leaving a
  trail of stale caption lines below. Covered by a test that renders two frames into a
  buffer and asserts the block is exactly cursor-neutral.
- **Control characters in art and captions are neutralized.** Both are untrusted text
  written straight to a terminal (`make 2>&1 | inkling` puts another program's output on
  the caption line), and a control character costs no display columns but can move the
  cursor or repaint the screen.
- **`RankMap::rank_at` bounds-checks `x`.** An out-of-range column used to wrap into the
  next row and return a neighbouring cell's rank instead of `None`.
- **`Art::parse` crops blank columns**, not only blank rows. Trailing whitespace in an art
  file used to inflate the canvas, which flipped `Directional::Auto` to the wrong axis,
  offset centering, and skewed the geodesic dominant-axis ordering. Tabs are expanded to
  8-column stops, oversized input is clamped rather than silently truncated by the `u16`
  cast, and empty art normalizes to 0x0.
- **Wide glyphs hold their columns in every renderer.** The live loader, the inline
  renderer, the final persisted frame and `frame::to_string` all reserve display width for
  a hidden cell, so CJK and emoji art no longer shears sideways as it reveals. Only the
  low-level `Reveal` got this right before.
- `GeodesicReport::spine_length` and friends now describe the skeleton the reveal actually
  traces, rather than the raw ink component's diameter.
- The CLI reports usage errors and exits 2 instead of silently ignoring a malformed
  `--total`, a dangling `-a`, or an unknown flag, and no longer swallows read errors as a
  clean end of input.

### Changed

- The five copies of the cell-walk (the live renderer, the two loader paths, the final
  frame, and `frame::to_string`) are one implementation. Diffing, colour, and width
  handling are now correct everywhere by construction rather than by repetition.
- `Geodesic` shares one `Spine` between the reveal and `diagnose`, and its bridged
  neighbour expansion no longer allocates a `Vec` per node in the inner BFS loop.
- `crossterm` is pulled in with `default-features = false`. The input event reader was
  never used and dragged in `mio`, `signal-hook`, `signal-hook-mio`,
  `signal-hook-registry`, `log`, and (on unix) `parking_lot`. Six crates left the tree.
- The README leads with the geodesic reveal, and the banner is rendered rather than left
  as a TODO. The recordings were re-shot on tighter canvases and re-encoded through a
  single global palette: 13.2 MB down to 4.8 MB in total, and the image above the fold
  from 6.1 MB to 226 KB.

### Infrastructure

- CI gained MSRV (1.74), rustdoc link, `cargo-deny`, and `--no-default-features` test
  jobs, and now compiles the WASM and Node bindings, which nothing checked before.
- Release workflows run the full CI suite and the version check before publishing, wait
  for the crates.io index to carry `inkling-loader` before publishing `inkling-cli`, and
  build wheels and Node binaries for aarch64 Linux, musl, and x86_64 macOS.

## [0.1.5] - 2026-06-11

- Node and WASM bindings, published to npm.
- Skeleton-based geodesic ordering, replacing the ink-diameter trace.
- Loader pacing smoothed against the reported position.

## [0.1.3] - 2026-06-09

- Synchronized output (DEC 2026) around every frame.
- Unicode display width for wide glyphs.
- `inkling::prelude`.

[0.2.0]: https://github.com/codizzler/inkling/releases/tag/v0.2.0
[0.1.5]: https://github.com/codizzler/inkling/releases/tag/v0.1.5
[0.1.3]: https://github.com/codizzler/inkling/releases/tag/v0.1.3
