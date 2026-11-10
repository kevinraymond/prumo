#!/usr/bin/env bash
# Run the benchmarks in the full TeX Live image, pinned to one CPU, with this checkout's
# prumo. Writes docs/benchmarks/results/. Needs Docker, git and Python 3.11 or later; the
# documents of corpus.toml that live elsewhere are fetched at their pinned commits.
#
#   docs/benchmarks/run.sh                          # every document, 5 warm runs
#   docs/benchmarks/run.sh --docs minimal --runs 3
#   docs/benchmarks/run.sh --whatif --docs hott     # prumo whatif instead of the phases
#   docs/benchmarks/run.sh --overhead               # builds with and without prumo, in turns
#   docs/benchmarks/run.sh --report                 # rewrite the markdown from the JSON
#
# BENCH_IMAGE   the image (default prumo-dev:tl2026-full: texlive/texlive:latest + Poppler,
#               built by `make verify-prune`)
# BENCH_CPUS    the CPU to pin to (default 4, a performance core on the 13900K)
# BENCH_REPOS   where clones of the fetched documents are kept (default
#               ~/.cache/prumo-bench/repos)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
image=${BENCH_IMAGE:-prumo-dev:tl2026-full}
cpus=${BENCH_CPUS:-4}

cargo build --release --quiet --manifest-path "$root/Cargo.toml"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Fetch only the documents asked for: bench.py's --docs, or all of them.
docs=
args=("$@")
for i in "${!args[@]}"; do [ "${args[$i]}" = --docs ] && docs=${args[$((i + 1))]:-}; done
case " $* " in
  *" --report "*) mkdir -p "$work/docs" ;;
  *) python3 -I "$here/fetch.py" --out "$work/docs" ${docs:+--docs "$docs"} ;;
esac
docker run --rm --user "$(id -u):$(id -g)" --cpuset-cpus "$cpus" \
  -e BENCH_IMAGE="$image" -e BENCH_IMAGE_ID="$(docker image inspect -f '{{.Id}}' "$image")" \
  -v "$work:/bench" -v "$here:/out" -v "$root/tex:/probe:ro" \
  -v "$root/target/release/prumo:/usr/local/bin/prumo:ro" \
  -w /bench "$image" python3 -I /out/bench.py --out /out/results "$@"
