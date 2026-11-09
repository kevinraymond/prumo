#!/bin/sh
# Two prumo binaries, the same output? Builds every fixture once per engine with OLD, then
# runs every report with OLD and NEW on that cache and compares, byte for byte. The gate for
# a refactor that must not change anything. Run it inside the TeX image, with the repo at
# the same path:
#
#   docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:$PWD" -w "$PWD" prumo-tex \
#     sh scripts/same_output.sh /path/to/old-prumo target/debug/prumo /tmp/diffs
#
# A fixture whose build fails (fontspec on pdflatex) is rebuilt on every call, since a failed
# run is never reused, so its timings move; everything else must come out identical.
set -u
OLD=$1; NEW=$2; OUT=$3
export HOME=/tmp/home; mkdir -p "$HOME" "$OUT"
same=0; diff=0
for main in fixtures/*/*.tex; do
    name=$(basename "$(dirname "$main")")
    for engine in pdflatex xelatex lualatex; do
        export PRUMO_CACHE="/tmp/cache-$engine"
        "$OLD" time --engine "$engine" --plain "$main" >/dev/null 2>&1 || { echo "skip   $name/$engine: the build fails, so every call rebuilds and timings move"; continue; }
        for cmd in "time --plain" "time --plain --by page" "time --plain --by file" "time --plain --preamble" "time --json" "fonts" "fonts --json" "graph" "graph --dot" "packages" "packages --json" "doctor --check-log" "doctor --check-log --json"; do
            case "$cmd" in doctor*) eng="" ;; *) eng="--engine $engine" ;; esac
            # shellcheck disable=SC2086  # $cmd and $eng are words on purpose
            o=$("$OLD" $cmd $eng "$main" 2>&1)
            # shellcheck disable=SC2086
            n=$("$NEW" $cmd $eng "$main" 2>&1)
            if [ "$o" = "$n" ]; then same=$((same+1)); else diff=$((diff+1)); echo "DIFF   $name/$engine: $cmd"; printf '%s\n' "$o" > "$OUT/$name-$engine-$(echo "$cmd" | tr ' ' _).old"; printf '%s\n' "$n" > "$OUT/$name-$engine-$(echo "$cmd" | tr ' ' _).new"; fi
        done
        "$OLD" time --engine "$engine" "--html=$OUT/h-old.html" "$main" >/dev/null 2>&1; "$NEW" time --engine "$engine" "--html=$OUT/h-new.html" "$main" >/dev/null 2>&1
        if cmp -s "$OUT/h-old.html" "$OUT/h-new.html"; then same=$((same+1)); else diff=$((diff+1)); echo "DIFF   $name/$engine: html"; cp "$OUT/h-old.html" "$OUT/$name-$engine.old.html"; cp "$OUT/h-new.html" "$OUT/$name-$engine.new.html"; fi
    done
done
export PRUMO_CACHE=/tmp/cache-pdflatex
for cmd in "prune fixtures --engines pdflatex" "prune fixtures --engines pdflatex --json"; do
    # shellcheck disable=SC2086
    o=$("$OLD" $cmd 2>&1)
    # shellcheck disable=SC2086
    n=$("$NEW" $cmd 2>&1)
    if [ "$o" = "$n" ]; then same=$((same+1)); else diff=$((diff+1)); echo "DIFF   $cmd"; printf '%s\n' "$o" > "$OUT/prune.old"; printf '%s\n' "$n" > "$OUT/prune.new"; fi
done
o=$("$OLD" doctor 2>&1 | sed 's/0\.1\.1/V/'); n=$("$NEW" doctor 2>&1 | sed 's/0\.1\.1/V/')
if [ "$o" = "$n" ]; then same=$((same+1)); else diff=$((diff+1)); echo "DIFF   doctor"; printf '%s\n' "$o" > "$OUT/doctor.old"; printf '%s\n' "$n" > "$OUT/doctor.new"; fi
echo "same: $same  different: $diff"
