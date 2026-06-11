# inkling (Python)

Reveal ASCII art as a progress indicator, from Python. Same engine as the Rust crate,
exposed as a tiny extension built with [PyO3](https://pyo3.rs). It installs as
`inkling-loader` and imports as `inkling`.

![inkling](https://raw.githubusercontent.com/codizzler/inkling/main/docs/demo-hero.gif)

```sh
pip install inkling-loader
```

Determinate, like `tqdm` but the bar is a drawing:

```python
from inkling import Loader

with Loader(total=len(items), rainbow=True) as bar:
    for it in items:
        work(it)
        bar.inc()
```

A download, setting the position as bytes arrive:

```python
bar = Loader(total=content_length, art_path="dragon.txt")
for chunk in response:
    file.write(chunk)
    bar.inc(len(chunk))
bar.finish()
```

| Method | Effect |
| --- | --- |
| `inc(delta=1)` | advance the position |
| `set(pos)` | set the absolute position |
| `set_length(total)` | change the total |
| `set_message(text)` | caption beneath the art |
| `finish()` / `finish_and_clear()` | finish, keeping or erasing the art |

Constructor keywords: `total`, `art`, `art_path`, `rainbow`, `geodesic`, `reading`,
`message`.

## The inkling family

This is the Python package. The same engine ships five ways:

- **`inkling-loader` on PyPI** (this package), for Python. `pip install inkling-loader`, import as `inkling`.
- **`inkling-loader`** on crates.io, the Rust library. `cargo add inkling-loader`.
- **`inkling-cli`**, the `inkling` command, to drive a reveal from any language through a pipe.
  `cargo install inkling-cli`.
- **`inkling-loader` on npm**, the Node addon. `npm install inkling-loader`.
- **`inkling-wasm` on npm**, the WebAssembly build for the browser.

## Building from source

```sh
pip install maturin
maturin develop            # build and install into the current venv
maturin build --release    # produce a wheel in target/wheels
```

Built from the [inkling](https://github.com/codizzler/inkling) Rust core. License: MIT.
