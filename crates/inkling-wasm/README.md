# inkling (WebAssembly)

Reveal ASCII art as a progress indicator, in the browser. The same pure Rust core as the
[inkling](https://github.com/codizzler/inkling) crate, compiled to WebAssembly with
[wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/).

The browser has no terminal, so this exposes the engine's *model* rather than a renderer:
parse art, choose an ordering, and read back the per-cell reveal ranks. Your JavaScript draws
each frame however it likes, which is exactly how the native renderers work.

```sh
npm install inkling-wasm
```

```js
import init, { Reveal } from "inkling-wasm";

await init();
const reveal = new Reveal(art, "geodesic"); // or "auto", "top", "left", ...
const glyphs = reveal.glyphs();             // width*height chars, row-major
const ranks = reveal.ranks();               // Float32Array, -1 for background

// a cell is visible exactly when its rank is in 0..=progress
function frame(progress) {
  let out = "";
  for (let i = 0; i < ranks.length; i++) {
    out += ranks[i] >= 0 && ranks[i] <= progress ? glyphs[i] : " ";
    if ((i + 1) % reveal.width() === 0) out += "\n";
  }
  return out;
}
```

## Building

```sh
cargo install wasm-pack
wasm-pack build --target web    # emits pkg/ with inkling.js + inkling_bg.wasm
```

Then `import` the module from `pkg/`, or use the published npm package.

## The inkling family

This is the browser (WebAssembly) build. The same engine ships five ways:

- **`inkling-wasm` on npm** (this package), the WebAssembly build for the browser.
- **`inkling-loader`** on crates.io, the Rust library. `cargo add inkling-loader`.
- **`inkling-cli`**, the `inkling` command, to drive a reveal from any language through a pipe.
- **`inkling-loader`** on PyPI, the Python package. `pip install inkling-loader`.
- **`inkling-loader`** on npm, the Node addon. `npm install inkling-loader`.

Built from the [inkling](https://github.com/codizzler/inkling) Rust core. License: MIT.
