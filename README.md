<div align="center">

# Inkling

**Reveal ASCII art as a progress indicator.**

Inkling maps progress onto the order a picture's glyphs appear. A normal bar fills a
line; Inkling paints a drawing, one glyph at a time, as your task runs. The name is
literal: the engine calls every non blank glyph *ink*, and an inkling is something
slowly taking shape.

![Inkling revealing a dragon as a task progresses](docs/demo-dragon.gif)

<sub>Rust · zero dependency core · Windows, macOS, Linux</sub>

</div>

---

## Why

Progress bars are a solved problem and a dull one. Inkling treats the loader as a small
canvas without giving up what a real progress API needs: a known total, increments from
any thread, iterator and reader wrappers, indeterminate spinners, and correct terminal
handling. The picture is the bonus. The ergonomics are the point.

## Install

Not on crates.io yet. Track the git repo:

```toml
[dependencies]
inkling-loader = { git = "https://github.com/codizzler/inkling" }
```

It publishes as `inkling-loader` because the short name is taken. The import path is
`inkling`.

## Usage

The front door is `Loader`. Give it a total, advance it as work completes, finish it. A
background thread repaints a live reveal at about 30 fps. Updates are lock free, so any
thread can report progress.

```rust
use inkling::Loader;

let loader = Loader::new(1000);
loader.set_message("Downloading");
for _ in 0..1000 {
    do_a_slice_of_work();
    loader.inc(1);
}
loader.finish();
```

Wrap an iterator and it advances itself, taking the total from `size_hint`:

```rust
use inkling::ProgressIteratorExt;

for item in items.iter().inkling() {
    process(item);
}
```

Wrap a reader and bytes advance it, which is what a download wants:

```rust
let loader = Loader::new(content_length);
let mut body = loader.wrap_read(response);
std::io::copy(&mut body, &mut file)?;
loader.finish();
```

Without a total, `Loader::spinner()` runs an indeterminate, breathing reveal. The
builder swaps the art, ordering, palette, or message:

```rust
use inkling::{Art, Loader, render::Style};

let loader = Loader::builder()
    .total(500)
    .art(Art::parse(include_str!("../art/whale.txt")))
    .style(Style::rainbow())          // lolcat style colouring
    .message("Rendering")
    .start();
```

`Loader` restores the terminal on finish or drop. Off a TTY (a pipe or CI) it prints the
finished art once instead of animating, so the same code is correct in both places. Run
`cargo run --example loader` to see it, with `iter`, `spinner`, `threads`, and `rainbow`
variants.

### Lower level control

`Loader` is a thin layer over a small core. Drive a `Reveal` directly with
`render(progress)` for frame by frame control, animate over a fixed duration with
`animate`, or render one frame to a `String` with `frame::to_string`. That last one has
no dependencies and is the ground truth the tests check against.

## Behaviour

| Platform | Notes |
| --- | --- |
| Windows 10+ | Virtual terminal sequences are enabled automatically. Windows Terminal gives truecolor. |
| macOS, Linux | Any modern terminal. |

Inkling honours `NO_COLOR`, never writes escape codes when output is not a terminal, and
falls back to a single plain frame when there is no TTY.

## How it works

Everything reduces to one value, the **reveal rank**. Each ink cell gets a rank in
`0..=1`, and a cell is visible exactly when `rank <= progress`:

```text
rank : cell -> 0..=1
visible(p) = { cell | rank(cell) <= p }
```

Rank is fixed and monotonic, so the reveal cannot run backwards, any progress value
renders directly (it is seekable and resumable), and a frame is a pure function of
progress. Assigning ranks is the one pluggable seam, the `Ordering` trait. The default
`Geodesic` ordering treats the ink as a graph, takes its largest connected component,
finds the spine with a double BFS sweep, and ranks each cell by geodesic distance along
it. Detached ink inherits the rank of the nearest spine cell, so loose detail reveals
next to the body it belongs to rather than dumping at the end.

```text
art       a grid of glyphs; whitespace is background
rank      RankMap: each ink cell's reveal rank in 0..=1
ordering  Ordering trait turns art into a RankMap (Geodesic, Scanline, or your own)
easing    timing curves
frame     pure text render of one frame; zero dependencies; the test ground truth
loader    Loader: the thread safe handle (inc/set, iterator and reader wrap)   [feature]
render    crossterm renderer: Reveal, animate, palettes; diff based            [feature]
```

The core (`art`, `rank`, `ordering`, `easing`, `frame`) is pure `std` with zero
dependencies and builds with `--no-default-features`. Only the terminal layer pulls in
[`crossterm`](https://crates.io/crates/crossterm), behind the default `terminal` feature.

## Showcase

Recordings live in `docs/`. See [docs/recording-gifs.md](docs/recording-gifs.md) for a
one command way to capture them with `vhs`.

| Reveal | What it shows |
| --- | --- |
| ![dragon](docs/demo-dragon.gif) | The geodesic spine trace on a detailed dragon |
| ![rainbow](docs/demo-rainbow.gif) | The same reveal under the rainbow palette |
| ![custom](docs/demo-custom.gif) | Your own art, revealing in step with its shape |

## Roadmap

- An authored path layer, so the reveal direction can be drawn by hand.
- Zhang and Suen skeleton thinning to refine the spine on thick art.
- Unicode width handling for wide glyphs and emoji.
- Synchronized output (DEC 2026) to remove tearing where terminals support it.
- Bindings: Python (PyO3), Node (napi), and a WebAssembly build.

## License

MIT. See [LICENSE-MIT](LICENSE-MIT).
