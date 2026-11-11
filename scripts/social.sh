#!/usr/bin/env bash
# Draw the repository's social image, docs/img/social.png (1280 x 640), from the record in
# docs/img/social.prumo. It needs Libertinus and DejaVu, so it runs in a full TeX Live
# image with Poppler (default prumo-dev:tl2026-full, which `make verify-prune` builds).
#
# The record is `prumo time florilegium-doc.tex --engine lualatex` on florilegium's manual at
# 833c4fc, the commit docs/benchmarks/corpus.toml pins, in that image.
#
# Usage: scripts/social.sh [--light] [--at MS]   (arguments go to scripts/social.py)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
image=${SOCIAL_IMAGE:-prumo-dev:tl2026-full}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
python3 -I "$root/scripts/social.py" "$root/docs/img/social.prumo" "$work/social.tex" "$@"
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$work:/work" -w /work "$image" \
  sh -c 'lualatex -interaction=nonstopmode -halt-on-error social.tex >/dev/null \
    && pdftoppm -r 72 -png -singlefile social.pdf social' \
  || { tail -20 "$work/social.log" 2>/dev/null; exit 1; }
cp "$work/social.png" "$root/docs/img/social.png"
echo "wrote docs/img/social.png"
