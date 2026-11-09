#!/usr/bin/env bash
# Run `cargo test --features tex` when the host has no TeX: build the test binaries here,
# run them inside the image from docker/Dockerfile.tex with the repo mounted at the same
# path (the binaries carry CARGO_MANIFEST_DIR). Also builds the CLI and runs `prumo time`
# on the custom-class fixture as the M1 demo.
# Usage: scripts/tex_tests.sh [image]   (default image: prumo-tex)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
image=${1:-prumo-tex}
cd "$root"
cargo build --features tex --tests --bins --message-format=json 2>/dev/null \
  | python3 -I -c 'import json,sys
for line in sys.stdin:
    m=json.loads(line)
    if m.get("reason")=="compiler-artifact" and m.get("executable"):
        print(m["target"]["name"], m["executable"])' > /tmp/prumo-tests.txt
home=$(mktemp -d)
status=0
while read -r name exe; do
  case "$name" in prumo) continue;; esac
  echo "== $name"
  docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp/home -e PRUMO_CACHE=/tmp/home/prumo-cache -v "$home:/tmp/home" \
    -v "$root:$root" -v "$exe:/t" "$image" /t --test-threads=1 "${@:2}" || status=1
done < /tmp/prumo-tests.txt
echo "== demo: prumo time fixtures/custom-class/main.tex"
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp/home -e PRUMO_CACHE=/tmp/home/prumo-cache -v "$home:/tmp/home" -v "$root:$root" -w "$root" \
  -v "$root/target/debug/prumo:/usr/local/bin/prumo" "$image" \
  bash -c 'prumo time fixtures/custom-class/main.tex --out /tmp/home/prumo-demo \
    && prumo graph fixtures/custom-class/main.tex --out /tmp/home/prumo-demo --depth 2 \
    && prumo fonts fixtures/basic/main.tex --out /tmp/home/prumo-demo \
    && prumo packages fixtures/custom-class/main.tex --out /tmp/home/prumo-demo | head -12 \
    && prumo doctor --check-log fixtures/custom-class/main.tex --out /tmp/home/prumo-demo \
    && prumo compare fixtures/basic/main.tex --out /tmp/home/prumo-compare 2>/dev/null \
    && prumo prune fixtures/basic fixtures/custom-class fixtures/corpus-b --out /tmp/home/prumo-prune 2>/dev/null | head -4 \
    && prumo unused fixtures/custom-class/main.tex --out /tmp/home/prumo-demo --jobs 4 2>/dev/null' || status=1
rm -rf "$home"
exit $status
