#!/usr/bin/env bash
# Run the benchmarks in the full TeX Live image, pinned to one CPU, with this checkout's
# prumo. Writes docs/benchmarks/results/. Needs Docker and a clone of florilegium.
#
#   docs/benchmarks/run.sh                      # every document, 5 warm runs
#   docs/benchmarks/run.sh --docs minimal --runs 3
#
# BENCH_IMAGE   the image (default prumo-dev:tl2026-full: texlive/texlive:latest + Poppler,
#               built by `make verify-prune`)
# BENCH_CPUS    the CPU to pin to (default 4, a performance core on the 13900K)
# FLORILEGIUM   path to a florilegium clone (default ~/ai/florilegium), FLORILEGIUM_COMMIT
#               the commit whose manual is measured (default 833c4fc)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
image=${BENCH_IMAGE:-prumo-dev:tl2026-full}
cpus=${BENCH_CPUS:-4}
florilegium=${FLORILEGIUM:-$HOME/ai/florilegium}
commit=${FLORILEGIUM_COMMIT:-833c4fc}

cargo build --release --quiet --manifest-path "$root/Cargo.toml"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/docs/florilegium"
cp -r "$here/docs/minimal" "$here/docs/standard" "$work/docs/"
python3 -I "$here/gen_large.py" --out "$work/docs/large/main.tex"
git -C "$florilegium" archive "$commit" | tar -x -C "$work/docs/florilegium"
docker run --rm --user "$(id -u):$(id -g)" --cpuset-cpus "$cpus" \
  -e BENCH_IMAGE="$image" -e BENCH_IMAGE_ID="$(docker image inspect -f '{{.Id}}' "$image")" \
  -e FLORILEGIUM_COMMIT="$commit" \
  -v "$work:/bench" -v "$here:/out" -v "$root/target/release/prumo:/usr/local/bin/prumo:ro" \
  -w /bench "$image" python3 -I /out/bench.py --out /out/results "$@"
