# inkling (WebAssembly)

Reveal ASCII art as a progress indicator, in the browser. The same pure Rust core as the
[inkling](https://github.com/codizzler/inkling) crate, compiled to WebAssembly with
[wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/).

The browser has no terminal, so this exposes the engine's *model* rather than a renderer:
parse art, choose an ordering, and read back the per-cell reveal ranks. Your JavaScript draws
each frame however it likes, which is exactly how the native renderers work.

```js
import init, { Reveal } from "inkling";

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

Then `import` the module from `pkg/`, or publish the package to npm.

Built from the [inkling](https://github.com/codizzler/inkling) Rust core. License: MIT.
