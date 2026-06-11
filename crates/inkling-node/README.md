# inkling (Node)

Reveal ASCII art as a progress indicator, from Node. The same Rust core as the
[inkling](https://github.com/codizzler/inkling) crate, compiled to a native addon with
[napi-rs](https://napi.rs).

```sh
npm install inkling-loader   # once published
```

```js
const { Loader } = require("inkling-loader");

const bar = new Loader({ total: items.length, rainbow: true });
for (const item of items) {
  work(item);
  bar.inc();
}
bar.finish();
```

| Method | Effect |
| --- | --- |
| `inc(delta = 1)` | advance the position |
| `set(pos)` | set the absolute position |
| `setLength(total)` | change the total |
| `setMessage(text)` | caption beneath the art |
| `finish()` / `finishAndClear()` | finish, keeping or erasing the art |

Constructor options: `total`, `art`, `artPath`, `rainbow`, `geodesic`, `reading`, `message`.

## Building

```sh
npm install
npm run build        # produces the native .node addon plus index.js / index.d.ts
```

## The inkling family

This is the Node package. The same engine ships several ways:

- **`inkling-loader` on npm** (this package), for Node. `npm install inkling-loader`.
- **`inkling-loader`** on crates.io, the Rust library. `cargo add inkling-loader`.
- **`inkling-loader`** on PyPI, the Python package. `pip install inkling-loader`.
- **`inkling-cli`**, the `inkling` command, to drive a reveal from any language through a pipe.

Built from the [inkling](https://github.com/codizzler/inkling) Rust core. License: MIT.
