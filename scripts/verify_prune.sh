#!/usr/bin/env bash
# The M4 verification gate (docs/05-packages-and-pruning.md): in a full TeX Live image,
# build every fixture on every engine and keep its text and page count; prune the
# installation to the fixtures with `prumo prune --apply --yes`; rebuild; the text and page
# counts must be identical and no log may say "File not found". Prints the before/after
# size line. Slow (removes ~5,000 packages): a weekly CI job, not a per-push one.
#
# Covers all six fixtures (16 builds: fontspec is XeTeX-only, biblatex goes through latexmk).
# Usage: scripts/verify_prune.sh [image]   (default: prumo-dev:tl2026-full; see docs/STATUS.md M0)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
image=${1:-prumo-dev:tl2026-full}
cd "$root"
cargo build --release 2>&1 | tail -1
cat > /tmp/prumo-gate.sh <<'EOF'
set -uo pipefail
export HOME=/tmp/home; mkdir -p $HOME
corpus=/tmp/corpus; rm -rf $corpus; mkdir -p $corpus
fixtures="basic custom-class corpus-b beamer biblatex fontspec"
for fx in $fixtures; do mkdir -p $corpus/$fx; cp /w/fixtures/$fx/*.tex /w/fixtures/$fx/*.cls /w/fixtures/$fx/*.bib $corpus/$fx/ 2>/dev/null || true; done
( cd /tmp && lualatex -interaction=batchmode '\documentclass{article}\begin{document}x\end{document}' >/dev/null 2>&1 \
  && xelatex -interaction=batchmode '\documentclass{article}\usepackage{fontspec}\begin{document}x\end{document}' >/dev/null 2>&1 )
# Every fixture on every engine, except where its header pins one (`% !TEX program =`);
# `% prumo: build with latexmk` fixtures build through latexmk (biber), as prune --latexmk does.
snapshot() { # $1 = label
  for fx in $fixtures; do
    main=$(grep -l '\\documentclass' $corpus/$fx/*.tex | head -1); job=$(basename ${main%.tex})
    engines=$(sed -n 's/^%[ !]*TEX[ ]*program[ ]*=[ ]*//Ip' $main | head -1); engines=${engines:-pdflatex xelatex lualatex}
    for eng in $engines; do
      d=/tmp/build-$1/$fx-$eng; rm -rf $d; mkdir -p $d; cp $corpus/$fx/* $d/
      case $eng in pdflatex) mkflag=-pdf;; xelatex) mkflag=-xelatex;; lualatex) mkflag=-lualatex;; esac
      if grep -qi '^% prumo: build with latexmk' $main; then
        ( cd $d && latexmk $mkflag -interaction=nonstopmode -halt-on-error $job.tex >/dev/null 2>&1 ) || { echo "BUILD FAILED ($1): $fx $eng (latexmk)"; tail -5 $d/$job.log; status=1; }
      else
        ( cd $d && $eng -interaction=nonstopmode -halt-on-error $job.tex >/dev/null 2>&1 ) || { echo "BUILD FAILED ($1): $fx $eng"; tail -5 $d/$job.log; status=1; }
      fi
      if grep -q "File .* not found\|! LaTeX Error: File" $d/$job.log 2>/dev/null; then echo "FILE NOT FOUND ($1): $fx $eng"; grep -m2 "not found" $d/$job.log; status=1; fi
      pdftotext -layout $d/$job.pdf $d/text.txt 2>/dev/null || true
      pdfinfo $d/$job.pdf 2>/dev/null | grep Pages > $d/pages.txt || true
    done
  done
}
status=0
du -sh /usr/local/texlive/2026/texmf-dist | awk '{print "before: texmf-dist " $1}'
snapshot before
cd $corpus
# --latexmk for the whole corpus: biblatex needs biber, and the keep set must prove biber and latexmk survive.
prumo prune $fixtures --engines pdflatex,xelatex,lualatex --latexmk --out /tmp/prumo-out 2>/dev/null | head -8
prumo prune $fixtures --engines pdflatex,xelatex,lualatex --latexmk --out /tmp/prumo-out --apply --yes 2>/dev/null | tail -2
du -sh /usr/local/texlive/2026/texmf-dist | awk '{print "after:  texmf-dist " $1}'
tlmgr list --only-installed 2>/dev/null | grep -c '^i ' | awk '{print "after:  " $1 " packages installed"}'
snapshot after
for d in /tmp/build-before/*; do
  n=$(basename $d)
  cmp -s $d/text.txt /tmp/build-after/$n/text.txt || { echo "TEXT DIFFERS: $n"; diff $d/text.txt /tmp/build-after/$n/text.txt | head -5; status=1; }
  cmp -s $d/pages.txt /tmp/build-after/$n/pages.txt || { echo "PAGES DIFFER: $n"; status=1; }
done
n=$(ls -d /tmp/build-before/* | wc -l)
[ $status -eq 0 ] && echo "GATE PASSED: $n/$n builds identical after pruning" || echo "GATE FAILED"
exit $status
EOF
docker run --rm -v "$root:/w" -v "$root/target/release/prumo:/usr/local/bin/prumo" -v /tmp/prumo-gate.sh:/gate.sh "$image" bash /gate.sh
