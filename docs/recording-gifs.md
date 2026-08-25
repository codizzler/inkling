# Recording the showcase GIFs

The README points at four recordings in this folder:

| GIF | What it captures |
| --- | --- |
| `demo-geodesic.gif` | Geodesic spine trace painting a serpent along its body (the lead image) |
| `demo-dragon.gif` | Default directional wipe, glow palette, built-in dragon |
| `demo-rainbow.gif` | Rainbow (lolcat) palette on the built-in dragon |
| `demo-hero.gif` | Rainbow palette revealing `inkling3d.txt` |

The cleanest way to capture a terminal GIF is [vhs](https://github.com/charmbracelet/vhs) by
Charm, which scripts the whole session so every recording looks consistent. The tracked
artifacts are the GIFs; the `.tape` scripts that produce them are local scaffolding (they are
gitignored), so the recipe lives here instead.

## Record them

From the repo root, build the examples once so the recording shows only program output (with
the binaries cached, `cargo run -q` starts instantly):

```sh
cargo build --examples
```

Then record and shrink:

```sh
vhs docs/geodesic.tape
scripts/optimize-gifs.sh
```

**Check `NO_COLOR` first.** vhs inherits the environment it is launched from, and inkling
honours [`NO_COLOR`](https://no-color.org). If it is set, every recording comes out
monochrome and the rainbow demo becomes indistinguishable from the glow one. Some tool
environments set it without saying so:

```sh
env | grep NO_COLOR    # expect nothing
```

The four GIFs share one tape. Only the output, the command, and the canvas size differ.
Drop this in a file, fill in the row from the table, and run `vhs <file>` from the repo root:

```tape
Output docs/<OUTPUT>.gif
Set FontSize <SIZE>
Set Width <WIDTH>
Set Height <HEIGHT>
Set Padding 24
Set LoopOffset 45%
Hide
Type "<COMMAND>"
Enter
Sleep 1100ms
Show
Sleep 8s
```

| `<OUTPUT>` | `<COMMAND>` | `<SIZE>` | `<WIDTH>` x `<HEIGHT>` |
| --- | --- | --- | --- |
| `demo-geodesic` | `cargo run -q --example download -- geodesic crates/inkling/assets/serpent.txt` | 16 | 760 x 600 |
| `demo-dragon` | `cargo run -q --example download` | 13 | 740 x 960 |
| `demo-rainbow` | `cargo run -q --example download -- rainbow` | 13 | 740 x 960 |
| `demo-hero` | `cargo run -q --example download -- rainbow crates/inkling/assets/inkling3d.txt` | 13 | 1440 x 590 |

`Hide` ... `Show` skips the typed command. The `Sleep 1100ms` before `Show` covers the
example's own preamble and nothing more, so the reveal itself is captured from zero: the
recording never shows more of the drawing than its caption claims.

`Set LoopOffset` then rotates which frame the file *opens* on. The whole reveal is still
recorded and still played; this only picks the still image GitHub shows before the GIF
cycles into view, so the preview is a half-painted drawing rather than a blank canvas.

## Framing

The canvas has to be at least as wide as the art, or the loader falls back to the alternate
screen (and, past that, clips). The dragon is 77 columns and the 3D logo is 167, so:

- `Set Width` must clear the art's column count at the chosen font size. If a recording of
  inline art suddenly loses the command preamble and centres itself, the canvas went one
  column too narrow and the loader switched to fullscreen.
- `Set Height` needs the art's rows plus the preamble plus a caption line, or the terminal
  scrolls mid-reveal.
- `Set FontSize` trades legibility for both. 16 suits short art; 13 keeps the 43-row dragon
  and the 167-column logo to a sane file size, and these render at README scale anyway.
- A dark terminal theme makes the glowing frontier and the rainbow pop. vhs defaults to one;
  override with `Set Theme "..."` if you like.

## Optimizing

`scripts/optimize-gifs.sh` re-encodes every recording in place through a single global
palette with transparency-diffed frames, which is worth roughly 4x on a terminal capture
with no visible loss. It is lossy, so run it once per recording rather than repeatedly.

The frame rate is per-file in that script. The lead recording keeps 20 fps because it is
the first thing anyone sees; the rainbow palette re-colours every glyph on every frame, so
nothing diffs away there and the only lever is fewer, coarser frames.

## With asciinema plus agg

If you would rather not use vhs:

```sh
asciinema rec demo.cast -c "cargo run -q --example download -- rainbow"
agg demo.cast docs/demo-rainbow.gif
```
