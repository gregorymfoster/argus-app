#!/usr/bin/env bash
# Shrink the README captures from prototype/scripts/readme-shots.mjs into docs/media/: stills as
# 1600 px WebP, clips as looping animated WebP. Only the public-domain sample card may appear in
# them (docs/media/CREDITS.md); public-export refuses any file there that CREDITS.md doesn't name.
#   scripts/readme-media.sh [shots-dir]   (default prototype/out/readme-shots)
set -euo pipefail
cd "$(dirname "$0")/.."
src="${1:-prototype/out/readme-shots}"
dest=docs/media
mkdir -p "$dest"
for f in "$src"/*.png; do
  ffmpeg -v error -y -i "$f" -vf scale=1600:-1:flags=lanczos -c:v libwebp -quality 82 "$dest/$(basename "${f%.png}").webp"
done
# Clips: skip the page load at the start, 10 fps, 1200 px, loop forever.
for f in "$src"/*.webm; do
  ffmpeg -v error -y -ss 1.4 -i "$f" -vf fps=10,scale=1200:-1:flags=lanczos -c:v libwebp_anim -quality 70 -compression_level 6 -loop 0 "$dest/$(basename "${f%.webm}").webp"
done
du -h "$dest"/*.webp
