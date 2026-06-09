# Recording the showcase GIFs

The README points at four recordings in this folder:

| GIF | What it captures |
| --- | --- |
| `demo-hero.gif` | Rainbow palette revealing `inkling3d.txt` as a simulated download |
| `demo-dragon.gif` | Default directional wipe, glow palette, built-in dragon |
| `demo-rainbow.gif` | Rainbow (lolcat) palette on the built-in dragon |
| `demo-geodesic.gif` | Geodesic spine trace painting a serpent along its body |

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

The four GIFs share one tape. Only the output, the command, and the canvas size differ.
Drop this in a file, fill in the row from the table, and run `vhs <file>` from the repo root:

```tape
Output docs/<OUTPUT>.gif
Set FontSize 16
Set Width <WIDTH>
Set Height <HEIGHT>
Set Padding 24
Hide
Type "<COMMAND>"
Enter
Sleep 800ms
Show
Sleep 8s
```

| `<OUTPUT>` | `<COMMAND>` | `<WIDTH>` x `<HEIGHT>` |
| --- | --- | --- |
| `demo-hero` | `cargo run -q --example download -- rainbow crates/inkling/assets/inkling3d.txt` | 1920 x 680 |
| `demo-dragon` | `cargo run -q --example download` | 1000 x 1200 |
| `demo-rainbow` | `cargo run -q --example download -- rainbow` | 1000 x 1200 |
| `demo-geodesic` | `cargo run -q --example download -- geodesic crates/inkling/assets/serpent.txt` | 1100 x 820 |

`Hide` ... `Show` skips the typed command, letting the example clear the screen so the GIF is
pure reveal. The hero is wide and short; the dragon is tall; tune the canvas to your art.

## Tuning

- `Set Width` / `Set Height` frame the canvas. Tall art (the 43-row dragon) needs more
  height; wide art (inkling3d) needs more width and less height, like the hero row above.
- `Set FontSize` trades legibility for fit. 16 is a good default; drop it for taller art.
- The trailing `Sleep` holds on the finished art. Each demo runs about seven seconds, so
  `Sleep 8s` captures the whole run and a beat on the final frame.
- A dark terminal theme makes the glowing frontier and the rainbow pop. vhs defaults to one;
  override with `Set Theme "..."` if you like.

## With asciinema plus agg

If you would rather not use vhs:

```sh
asciinema rec demo.cast -c "cargo run -q --example download -- rainbow"
agg demo.cast docs/demo-rainbow.gif
```
