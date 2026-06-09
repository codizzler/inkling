# inkling (Python)

Reveal ASCII art as a progress indicator, from Python. Same engine as the Rust crate,
exposed as a tiny extension built with [PyO3](https://pyo3.rs).

```sh
pip install inkling
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

## Building from source

```sh
pip install maturin
maturin develop            # build and install into the current venv
maturin build --release    # produce a wheel in target/wheels
```

Built from the [inkling](https://github.com/codizzler/inkling) Rust core. License: MIT.
