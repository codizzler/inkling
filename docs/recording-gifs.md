# Recording the showcase GIFs

The README points at three recordings in this folder:

- `docs/demo-dragon.gif`
- `docs/demo-serpent.gif`
- `docs/demo-custom.gif`

The cleanest, most reproducible way to capture a terminal GIF is
[vhs](https://github.com/charmbracelet/vhs) by Charm. It scripts the whole session,
so every recording looks consistent.

## With vhs (recommended)

Install vhs, then save this as `demo-dragon.tape`:

```tape
Output docs/demo-dragon.gif
Set FontSize 18
Set Width 1000
Set Height 640
Set Padding 24
Type "cargo run --example dragon -- --art crates/inkling/assets/dragon.txt"
Enter
Sleep 5s
```

Render it:

```sh
vhs demo-dragon.tape
```

Repeat for the serpent (drop the `--art` flag) and for a custom piece of art.

## With asciinema plus agg

```sh
asciinema rec demo.cast -c "cargo run --example dragon"
agg demo.cast docs/demo-serpent.gif
```

## Tips

- Keep the window roughly square so the art is not cropped.
- A dark terminal theme makes the glowing frontier pop.
- Trim to a few seconds. A loader does not need to be long.
