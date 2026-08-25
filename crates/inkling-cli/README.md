# inkling (CLI)

The `inkling` command line tool: pipe progress in, watch ASCII art reveal. It is the
language-agnostic bridge to [inkling](https://github.com/codizzler/inkling), the way you
would pipe to `pv`, with no bindings to link against.

![Inkling revealing ASCII art in rainbow as a task runs](https://raw.githubusercontent.com/codizzler/inkling/main/docs/demo-hero.gif)

```sh
cargo install inkling-cli      # installs the `inkling` binary
```

Feed it progress on stdin, one token per line:

```sh
# count a known total
seq 0 100 | inkling --total 100

# rainbow palette, your own art, captions streamed in
inkling --total 100 --rainbow --art snake.txt < progress.log

# no total: an indeterminate reveal that breathes while the build runs
make 2>&1 | inkling --geodesic --easing ease-out-cubic
```

| Token on stdin | Effect |
| --- | --- |
| `N` | set absolute progress to `N` |
| `+N` | advance progress by `N` |
| any other text | becomes the caption beneath the art |

On end of input the art finishes filled. A malformed option is a usage error on stderr and
exit status 2, never a silently ignored flag.

## Options

```text
    -t, --total <N>      total units of work; omit for an indeterminate spinner
    -a, --art <FILE>     ASCII art to reveal (default: the built-in dragon)
    -m, --message <MSG>  initial caption shown beneath the art

REVEAL
        --geodesic       trace the art's spine instead of a directional wipe
        --scanline       plain reading order, the baseline
        --reading        wipe along the locale's reading direction
        --ltr, --rtl     wipe left-to-right or right-to-left explicitly
        --start <WHERE>  geodesic start tip: top-left (default), bottom, topological
        --bridge <N>     blank cells the spine may step across (default 1, 0 off)
        --easing <CURVE> linear (default), ease-out-cubic, ease-out-quint,
                         ease-in-out-cubic

COLOUR
        --rainbow        lolcat-style rainbow palette
        --light          palette tuned for a light terminal background
        --no-color       no colour at all (also honours NO_COLOR)
        --color <DEPTH>  auto (default), truecolor, 256, 16, none
        --head <HEX>     colour at the leading edge, e.g. ffe28a
        --body <HEX>     colour of settled ink, e.g. 7886a8
        --feather <F>    width of the glow, in rank units (default 0.07; 0 off)

    -h, --help           print this help
    -V, --version        print the version
```

## The inkling family

This crate, `inkling-cli`, installs the `inkling` binary. The same engine ships five ways:

- **`inkling-cli`** (this crate), the command-line tool. `cargo install inkling-cli`.
- **`inkling-loader`**, the Rust library it is built on. `cargo add inkling-loader`.
- **`inkling-loader` on PyPI**, the Python package. `pip install inkling-loader`.
- **`inkling-loader` on npm**, the Node addon. `npm install inkling-loader`.
- **`inkling-wasm` on npm**, the WebAssembly build for the browser.

License: MIT.
