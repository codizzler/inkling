# Contributing to Inkling

Bug reports, art, and pull requests are all welcome. This file is short on purpose: the
project is small, and most of what you need is one command.

## The shape of the workspace

```text
crates/inkling        the library, published as inkling-loader, imported as inkling
crates/inkling-cli    the `inkling` binary
crates/inkling-py     Python bindings (PyO3 + maturin)
crates/inkling-node   Node native addon (napi-rs)
crates/inkling-wasm   browser build (wasm-bindgen)
docs/                 the showcase recordings and how to make them
scripts/              release pre-flight
```

The three bindings build with their own toolchains (maturin, napi-rs, wasm-pack), so they
are deliberately excluded from the cargo workspace. That means `cargo test` at the root
does not touch them; CI checks each one on its own manifest, and so should you if you
change the core's public API.

## Before you open a pull request

```sh
cargo test --workspace --all-features
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

CI runs those on Linux, macOS and Windows, plus five more jobs it is easy to trip:

- **The zero dependency core.** `cargo test -p inkling-loader --no-default-features`. The
  core (`art`, `rank`, `ordering`, `easing`, `width`, `frame`) has no dependencies and it
  stays that way. Anything reaching for the terminal belongs behind the `terminal` feature.
- **MSRV.** `cargo +1.74 check --workspace --all-features`. A newer std method compiles
  fine locally and breaks the declared floor.
- **Rustdoc.** `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps`.
- **The bindings.** `cargo check --manifest-path crates/inkling-<py|wasm|node>/Cargo.toml`.
- **`cargo deny check`**, for advisories, licenses and duplicate versions.

## What the code cares about

- **The rank map is the model.** An `Ordering` turns `Art` into a `RankMap`, and a cell is
  visible exactly when `rank <= progress`. That function must stay pure and monotonic:
  it is what makes a reveal seekable, resumable, and testable without a terminal.
- **One cell-walk.** Deciding what a cell looks like at a given progress happens in
  `frame::cell`, and every renderer goes through it. If you find yourself writing a second
  copy of that loop, that is the bug.
- **Tests do not need a TTY.** Render into a buffer and assert on the bytes. The renderer
  is written against a sink for exactly this reason.
- **Prose is part of the work.** Module docs explain why, not what. Match the surrounding
  voice.

## Adding art

Art lives in `crates/inkling/assets/` as plain text. Keep the artist's signature in the
file. Whitespace is background, and `Art::parse` crops blank rows and columns, so you do
not need to trim the file by hand, but do check how it reveals under both orderings:

```sh
cargo run --example download -- geodesic crates/inkling/assets/your-art.txt
```

## Releasing

Bump the version in all five `Cargo.toml` files and in `crates/inkling-node/package.json`,
add a `CHANGELOG.md` entry, then:

```sh
scripts/check-versions.sh v0.2.0
```

Pushing the matching tag runs the full CI suite and that same check before anything
reaches crates.io, PyPI or npm.
