# inkling (Node)

Reveal ASCII art as a progress indicator, from Node. The same Rust core as the
[inkling](https://github.com/codizzler/inkling) crate, compiled to a native addon with
[napi-rs](https://napi.rs).

![The geodesic reveal tracing a serpent along its spine](https://raw.githubusercontent.com/codizzler/inkling/main/docs/demo-geodesic.gif)

```sh
npm install inkling-loader
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

It implements `Symbol.dispose`, so `using` finishes the reveal on scope exit even if the
body throws:

```js
using bar = new Loader({ total: files.length, geodesic: true });
for (const file of files) {
  if (check(file)) bar.println(`ok ${file}`);
  bar.inc();
}
```

| Method | Effect |
| --- | --- |
| `inc(delta = 1)` | advance the position |
| `set(pos)` | set the absolute position |
| `setLength(total)` | change the total |
| `setMessage(text)` | caption beneath the art |
| `println(line)` | print a line above the live reveal |
| `finish()` / `finishAndClear()` | finish, keeping or erasing the art |

| Getter | Value |
| --- | --- |
| `position`, `length` | current and total units of work |
| `elapsed` | seconds since the loader started |
| `rate` | average units per second |
| `eta` | estimated seconds remaining, or `null` |

Constructor options: `total`, `art`, `artPath`, `ordering` (`"auto"`, `"geodesic"`,
`"scanline"`, `"reading"`, `"ltr"`, `"rtl"`), the shorthands `rainbow`, `geodesic` and
`reading`, plus `light`, `color`, `head`, `body`, `feather`, `easing`, `start`, `bridge`,
and `message`.

Prebuilt binaries ship for Linux (x64 and arm64, glibc and musl), macOS (x64 and Apple
silicon), and Windows x64.

## Building

```sh
npm install
npm run build        # produces the native .node addon plus index.js / index.d.ts
```

## The inkling family

This is the Node package. The same engine ships five ways:

- **`inkling-loader` on npm** (this package), for Node. `npm install inkling-loader`.
- **`inkling-loader`** on crates.io, the Rust library. `cargo add inkling-loader`.
- **`inkling-loader`** on PyPI, the Python package. `pip install inkling-loader`.
- **`inkling-cli`**, the `inkling` command, to drive a reveal from any language through a pipe.
- **`inkling-wasm` on npm**, the WebAssembly build for the browser.

Built from the [inkling](https://github.com/codizzler/inkling) Rust core. License: MIT.
