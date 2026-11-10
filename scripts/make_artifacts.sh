#!/usr/bin/env bash
# Build every fixture (a directory under fixtures/ with a \documentclass) with every engine using the probe, and check in the
# artifacts parsers are tested against (.prumo, .fls, log tail, pdffonts, golden JSON).
# Usage: scripts/make_artifacts.sh [fixture...]   (run from repo root; needs TeX Live + poppler-utils;
# with names, only those fixtures are rebuilt)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
# Fixed build directory so the PWD line in every .fls is stable across regenerations.
build=${PRUMO_BUILD_DIR:-/tmp/prumo-build}
# A cold luaotfload font cache makes the first LuaTeX run scan every system font
# (6,886 INPUT lines on the basic fixture instead of ~140). Warm it once.
# XeTeX's first run builds the fontconfig cache (finding 26) and would charge it to size10.clo.
( t=$(mktemp -d); cd "$t" && lualatex -interaction=batchmode '\documentclass{article}\begin{document}x\end{document}' >/dev/null 2>&1 \
    && xelatex -interaction=batchmode '\documentclass{article}\usepackage{fontspec}\begin{document}x\end{document}' >/dev/null 2>&1; rm -rf "$t" )
# The probe as prumo loads it: the environments are run::DEFAULT_ENVIRONMENTS
# (tests/fixtures.rs checks the two lists agree).
envs=tikzpicture,axis,tcolorbox,tblr,longtable,tabularx,minted,lstlisting,forest
pretex="\\RequirePackage{prumo}\\PrumoEnvironments{$envs}"
for dir in "$root"/fixtures/*/; do
  fx=$(basename "$dir")
  if [ $# -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$fx"; then continue; fi
  main=$(grep -l '\\documentclass' "$dir"/*.tex 2>/dev/null | head -1) || true
  [ -n "$main" ] || continue
  job=$(basename "${main%.tex}")
  # A fixture's header may pin its engine (`% !TEX program = xelatex`, the editors' magic
  # comment, which prune honors too) and ask for latexmk (`% prumo: build with latexmk`).
  engines=$(sed -n 's/^%[ !]*TEX[ ]*program[ ]*=[ ]*//Ip' "$main" | head -1); engines=${engines:-pdflatex xelatex lualatex}
  if grep -qi '^% prumo: build with latexmk' "$main"; then mk=1; else mk=0; fi
  for eng in $engines; do
    tmp="$build/$fx"; rm -rf "$tmp"; mkdir -p "$tmp"; cp "$dir"/*.tex "$dir"/*.cls "$dir"/*.sty "$dir"/*.bib "$tmp"/ 2>/dev/null || true
    out="$dir/artifacts/$eng"; mkdir -p "$out"
    case $eng in pdflatex) mkflag=-pdf;; xelatex) mkflag=-xelatex;; lualatex) mkflag=-lualatex;; esac
    if [ $mk = 1 ]; then
      ( cd "$tmp" && TEXINPUTS="$root/tex:" SOURCE_DATE_EPOCH=0 FORCE_SOURCE_DATE=1 \
          latexmk $mkflag -interaction=nonstopmode -halt-on-error -recorder -jobname="$job" \
          "-usepretex=$pretex" "$job.tex" > build.out 2>&1 ) || { echo "FAIL $fx $eng (latexmk)"; tail -20 "$tmp/build.out"; exit 1; }
    else
      ( cd "$tmp" && TEXINPUTS="$root/tex:" SOURCE_DATE_EPOCH=0 FORCE_SOURCE_DATE=1 \
          $eng -interaction=nonstopmode -halt-on-error -recorder -jobname="$job" \
          "$pretex\\input{$job}" > build.out 2>&1 ) || { echo "FAIL $fx $eng"; tail -20 "$tmp/build.out"; exit 1; }
    fi
    cp "$tmp/$job.prumo" "$tmp/$job.fls" "$out/"
    cp "$tmp/$job.log" "$out/$job.log"
    # The texmf roots of the installation that ran the build; tests classify .fls paths with these.
    for v in TEXMFROOT TEXMFDIST TEXMFVAR TEXMFSYSVAR TEXMFCONFIG TEXMFSYSCONFIG TEXMFLOCAL TEXMFHOME; do
      echo "$v=$(kpsewhich -var-value $v)"
    done > "$out/kpsewhich.txt"
    pdffonts "$tmp/$job.pdf" > "$out/pdffonts.txt"
    # The pdftex.map lines that matter for this run (loaded TFMs, and PostScript names the
    # PDF embeds), so the Rust font report can be tested without a TeX installation.
    map=$(grep -m1 'pdftex.map$' "$tmp/$job.fls" | sed 's/^INPUT //' || true); [ -n "$map" ] || map=$(kpsewhich --format=map pdftex.map)
    tfms=$({ grep '\.tfm$' "$tmp/$job.fls" || true; } | sed 's|.*/||; s|\.tfm$||' | sort -u | paste -sd'|')
    pss=$({ tail -n +3 "$out/pdffonts.txt" || true; } | awk '{print $1}' | sed 's/^[A-Z]*+//' | sort -u | paste -sd'|')
    grep -E "^(${tfms:-NONE}) |^[^ ]+ (${pss:-NONE}) " "$map" > "$out/fontmap.txt" || true
    ( cd "$tmp" && python3 -I "$root/scripts/prumo_report.py" "$job" --json > "$out/golden.json" \
                && python3 -I "$root/scripts/prumo_report.py" "$job" > "$out/report.txt" )
    echo "ok   $fx $eng  $(grep -c . "$out/$job.prumo") records, $(grep -c ^INPUT "$out/$job.fls") inputs"
    rm -rf "$tmp"
  done
done
