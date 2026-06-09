# inkling (CLI)

The `inkling` command line tool: pipe progress in, watch ASCII art reveal. It is the
language-agnostic bridge to [inkling](https://github.com/codizzler/inkling), the way you
would pipe to `pv`, with no bindings to link against.

```sh
cargo install inkling-cli      # installs the `inkling` binary
```

Feed it progress on stdin, one token per line:

```sh
# count a known total
seq 0 100 | inkling --total 100

# rainbow palette, your own art, captions streamed in
inkling --total 100 --rainbow --art snake.txt < progress.log
```

| Token on stdin | Effect |
| --- | --- |
| `N` | set absolute progress to `N` |
| `+N` | advance progress by `N` |
| any other text | becomes the caption beneath the art |

On end of input the art finishes filled. Run `inkling --help` for every flag
(`--total`, `--art`, `--message`, `--rainbow`, `--geodesic`, `--reading`).

License: MIT.
