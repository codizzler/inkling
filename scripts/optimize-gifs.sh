#!/usr/bin/env sh
# Re-encode the showcase recordings so they are small enough to sit in a README.
#
#   vhs docs/<name>.tape      # record (writes a large, unoptimized GIF)
#   scripts/optimize-gifs.sh  # then shrink it in place
#
# vhs writes one full frame per capture with a per-frame palette. Running the
# frames back through a single global palette, with transparency-diffed frames,
# is worth roughly 4x with no visible loss on a terminal recording. Run it once
# per recording: it is lossy, so re-running compounds the quantization.
#
# The frame rate is the other lever, and it is per-file on purpose. The lead
# recording keeps 20 fps because it is the first thing anyone sees. The rainbow
# palette re-colours every glyph on every frame, so nothing diffs away and the
# only cure is fewer, coarser frames.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

command -v ffmpeg >/dev/null || { echo "ffmpeg is required" >&2; exit 1; }

# file                   fps  colours
set -- \
    "docs/demo-geodesic.gif 20 96" \
    "docs/demo-dragon.gif   15 64" \
    "docs/demo-rainbow.gif  12 64" \
    "docs/demo-hero.gif     12 64"

for entry in "$@"; do
    # shellcheck disable=SC2086
    set -- $entry
    file=$1 fps=$2 colors=$3
    [ -f "$file" ] || { echo "skipping $file (not recorded yet)"; continue; }
    before=$(wc -c < "$file")
    tmp="${file%.gif}.opt.gif"
    ffmpeg -y -v error -i "$file" -filter_complex \
        "fps=${fps},split[a][b];[a]palettegen=max_colors=${colors}:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
        -gifflags +transdiff "$tmp"
    after=$(wc -c < "$tmp")
    mv -f "$tmp" "$file"
    printf '%-26s %8s -> %8s bytes\n' "$file" "$before" "$after"
done
