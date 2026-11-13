# 02 — Prior art

Survey done 2026-11-06. The tools in A, B, C and E were installed and run on TeX Live 2026 on
2026-11-10 and the rows corrected. SaTeX, DEPP, texdepend, texdeps, two TeX.SE questions and a
tex-live thread were added on 2026-11-12. SaTeX 0.2.2 and pdffonts were run that day, and
DEPP 0.4 on 2026-11-13. texdepend and texdeps were read but not run. Each gap below is what a
tool did on the date given, as a fact to design around, not a verdict on the tool.

## Where prumo does the same thing

- `unused`: remove the request, rebuild, compare the output. LaTeXpkges did it in 2015, and
  latexpkges3 and stygtfo do it too (A). SaTeX's `unused-package` lint asks the same question
  without a build.
- `graph`: a package dependency graph with Graphviz output, as latex-dependency-grapher draws
  (C).
- `packages`: the files a run read (`-recorder`), mapped to TeX Live packages. mkjobtexmf
  collects the same file list (E). Joseph Wright described the same `.fls`-to-package lookup
  on the tex-live list in April 2024 and posted it as a script on TeX.SE (E, Demand signals).
  DEPP builds its list from a `file/before` hook, as prumo's probe does (E).
- `prune`: a small install for a set of documents. TinyTeX and texliveonfly get there by
  adding, mkjobtexmf by building a per-job tree, and DEPP by writing the list to install (E).
- `fonts`: the embedded side is pdffonts output (D).
- `time`: per-page timing with `\pdfelapsedtime` is a TeX.SE answer from 2014 (B). texprof
  goes finer, to the line and the macro.
- `whatif`: a preamble precompiled with mylatexformat, draft mode and TikZ externalization are
  existing techniques. prumo only measures them.

## By area

### A. Unused `\usepackage` detection

| Tool | Approach | Gap |
|---|---|---|
| LaTeXpkges (bazzilic, 2015) / latexpkges3 (TarasKuzyo fork, PyPI 1.0, 2021) | Remove each top-level `\usepackage`, rebuild twice, compare an MD5 of the output (`.pdf`, `.xdv`, `.dvi`) with `/ID` stripped; `--visual` compares Ghostscript JPEGs | Top-level only, by its own README. Checked 2026-11-10: its date patterns never match (missing commas in the pattern list), so timestamps change the hash. `--visual` compares against a directory it has deleted and reports every package removable. It writes trial files and deletes outputs in the document's directory |
| stygtfo (PyPI 0.1.0, 2022) | Comment out each column-0 `\usepackage` line, rebuild, compare pages with diff-pdf-visually | Its README says it was tested with Tectonic only. Checked 2026-11-10: with ImageMagick 7, identical pages compare as different. Without `-o` it overwrites the input |
| latex-dependency-grapher v1.0 (Java, CTAN 2024-01) | Regex scan of `\usepackage`/`\RequirePackage`/`\input`/`\include`, kpsewhich, GraphViz; "unused" = comment the line out, pdflatex still exits 0 | No output comparison; pdflatex only. Checked 2026-11-10: the regex misses `\usepackage[opts]{x}`. No release since 1.0 |
| SaTeX 0.2.2 (Florian Sihler, Rust, 2026-11-12), `unused-package` lint | Static: an abstract interpretation of the document, no build. Fires when every name a loaded package defines is unused | A hint by its own description: side effects (layout, hooks, fonts, `\AtBeginDocument` code) are not seen. Run 2026-11-12 on prumo's basic fixture: no note for xcolor, which prumo's `unused` finds removable |
| cmdtrack, refcheck, chklref, checkcites | Find unused *commands*, *labels*, *citations* | Not packages |
| texlab, LaTeX Workshop, TeXstudio, LTeX | Editor diagnostics | No unused-package diagnostic |

**Same idea:** prumo's `unused` is the remove-and-rebuild experiment LaTeXpkges ran in 2015,
and latexpkges3 and stygtfo run it too. The candidates come from the probe's records, so a
request in the user's own class or package counts as well. The request is deleted from its
loader's source, with blocking as the fallback. The comparison is page count, text and a
render, not bytes. See `06-unused-packages.md`.

### B. Compile-time profiling

| Tool | Approach | Gap |
|---|---|---|
| **texprof / texprofile** (Ruckert; TUGboat 45:2, 2024; in TeX Live's `hitex` package) | Its own engine (TeX + e-TeX + Prote, DVI output) records time per input line and per macro into `.tprof`; `texprofile` reports by file/line/macro with call-graph data; `-pdf` only stands in pdfTeX primitives when building a format | Not pdfTeX, XeTeX or LuaTeX. TeX Live 2026 ships no texprof formats, and a LaTeX format does not build against the 2026-11-01 kernel (macro stack overflow in expl3-code.tex). Not package-oriented |
| TeX.SE 210139 (2014), Robert's answer 211572 | `\pdfelapsedtime` printed and `\pdfresettimer` called in `\AtBeginDocument` and `\AtBeginShipout`, one `timer:` line per page in the log; a 2020 answer (558409) does the same on LuaLaTeX with `os.clock()` | Added by hand, per page only. The answer itself notes that TeX's asynchronous output routine charges a page with material from the pages around it. `os.clock()` is CPU time, not wall clock |
| SaTeX 0.2.2, `--timings` | Per phase and per file, with token counts, for SaTeX's own interpretation of the document | Measures SaTeX's run, not the engine's ("Both measure the run itself", its README). Run 2026-11-12 on prumo's biblatex fixture: `biblatex.sty` 1,098 ms; the first run built a kernel cache from `latex.ltx` in 55 s |
| l3benchmark (`\benchmark:n`, `tic`/`toc`) | expl3 timers you place by hand | Building block |
| regstats, scrtime | Whole-run time | No breakdown |
| ConTeXt `mtxrun --script profile` | Lua/TeX profiling | ConTeXt only |
| latexmk | Build orchestration | No timing beyond `time latexmk` |

**Same idea:** per-page timing is the TeX.SE answer's technique. prumo takes it from the
shipout hooks with `\sys_timer:` (expl3, all engines, verified) and adds packages, files and
phases. Point users at texprof for per-macro work; prumo is the engine-agnostic
package/file/page layer above it. Ruckert's article notes that no profiling tool had been
available to macro package authors before texprof, which is the demand signal.

### C. Package dependency graph

| Tool | Approach | Gap |
|---|---|---|
| latex-dependency-grapher | Regex scan of the source, kpsewhich resolution, Graphviz output | Checked 2026-11-10: includes conditional loads that never happen (xcolor → colortbl); misses loads written with options or spaces, and class loads |
| SaTeX 0.2.2, `--timings` | Prints the tree of files its interpretation read, which file under which | From the interpretation, not a build |
| texfindpkg (Lyu, CTAN 2024A, in TeX Live and MiKTeX) | Which package provides a file, command or environment, its dependencies, and `tlmgr`/`miktex` install, from TeXstudio `.cwl` data and the tlpdb | Names in, not a document; the `.cwl` dependency data differs from what a build reads |
| snapshot + bundledoc | `\listfiles`-style flat `.dep` list, then archive the files | Flat; no edges |
| texdepend (Friendly, Perl, 0.92), texdeps (Cozens); CTAN, not in TeX Live; not run | Scan the source for `\input`, `\include`, `\usepackage` and `\includegraphics` (texdeps optionally through kpsewhich) and print the files, for make or tar | Source scan; flat |
| `\listfiles`, adhocfilelist, inputtrc | Flat lists / traces | No graph |
| `-recorder` `.fls` | Every file read/written | Flat; no nesting |
| texlogsieve (Lua, 1.6.1, 2026-01) | Rebuilds file nesting from the log's parentheses to filter messages | Does not export the tree; but its parser is the reference for log-based nesting |

**Same idea:** latex-dependency-grapher draws the same kind of graph with Graphviz. prumo's
edges come from the `package/before` hook and a depth counter (verified), so they are the
loads that happened. Use texlogsieve's approach as the cross-check parser for logs from runs
without the probe.

### D. Font usage

| Tool | Approach | Gap |
|---|---|---|
| pdffonts (Poppler; 26.07.0 run 2026-11-12) | Lists the fonts in the PDF: type, encoding, embedded, subset | Only the output side; Type 3 fonts are anonymous (`F44`) |
| tracefnt / `\tracingfonts` | NFSS load/substitution trace in the log | Raw; no report |
| pdfTeX log trailer | `<...pfb>` list of embedded Type 1 files | Hand parsing |

**Same idea:** the embedded side of `fonts` is pdffonts output. The `.fls` is the better
source on pdfTeX: `.tfm` INPUT = loaded, `.pfb`/`.pk` INPUT = embedded (verified). Nothing
above links either to the package that caused the load; prumo does, through the `.fd` file
hook context.

### E. Slimming / pruning TeX Live

| Tool | Approach | Gap |
|---|---|---|
| mkjobtexmf (Oberdiek, v0.8, 2011) | Run with `-recorder`/strace, symlink (or `--copy`) the used files into a job-local texmf tree | File level, one job, "experimental" by its own docs |
| DEPP 0.4 (Marei Peischl, Island of TeX, 2026-01-07; GitLab, not on CTAN; run 2026-11-13) | A package loaded before the document. A `file/before` hook collects the files. On LuaLaTeX each file's TeX Live package comes from its path (`tex/<format>/<package>/`) and the fonts from luaotfload. On pdfLaTeX and XeLaTeX each file goes to `tlmgr search --file` through shell-escape (`texosquery-jre8` under the default restricted shell). Hyphenation patterns from babel or polyglossia. Writes `DEPENDS.txt` at `enddocument/info` | Files read outside the hooks are not in the list (below). Lists what to install |
| `depreader.lua` (Joseph Wright, TeX.SE answer 716373, 2024-04-24) | Reads the `.fls`, runs `tlmgr search --file /<name>` per file, prints the TeX Live packages and the files with no match | By its own list: only files a TeX run reads (not biber or makeindex), not format files, slow (one `tlmgr` call per file), no tlpdb `depend` edges |
| snapshot + bundledoc (Pakin) | Archive a document's dependencies | For archiving, not for an install |
| texliveonfly (2011) | Install missing packages via tlmgr while compiling | Add-only |
| TinyTeX (Yihui Xie; the installer needs no R, the auto-install is in the R package) | Small TeX Live (about 100 packages by default) + auto-install missing packages from the log | Add-only |
| tlmgr | `scheme-minimal`, `scheme-infraonly`, `remove`, `search --global --file`, `search --json` (since 2024-04, after the thread below; works on TeX Live 2026), pinning, `--usermode` | No "remove what these documents don't use" |
| Tectonic bundles | Self-contained bundles; custom bundles from a hand-written spec | No usage-driven subsetting |
| MiKTeX | Install on the fly | No pruning |

**DEPP on prumo's fixtures (2026-11-13).** Built from the GitLab source (commit `bae14f2`)
with `tex depp.ins` and run on TeX Live 2026, next to `prumo packages` on the same engine:
basic, biblatex, beamer and custom-class on pdfLaTeX and LuaLaTeX, basic, biblatex and
fontspec on XeLaTeX, fontspec on LuaLaTeX.

- All three engines worked. Its documentation says LuaLaTeX works and the other engines are
  in progress.
- A build took 0.4 to 0.9 s on LuaLaTeX. On pdfLaTeX and XeLaTeX with `-shell-escape`, one
  `tlmgr search --file` per file made it 18 to 124 s (124 s for beamer on pdfLaTeX). Without
  shell-escape the list was empty, with a warning. Under the default restricted shell it used
  `texosquery-jre8`, and the list for basic had `cmextra`, a directory in amsfonts, in place
  of amsfonts.
- For the packages a document loads, its lists and prumo's agreed, with two differences.
  tlmgr can list two packages for a file and the first is the one recorded, so on pdfLaTeX
  `array.sty` and `enumerate.sty` came out as `latex-tools-dev`, LaTeX's development release,
  not `tools`. Every list from pdfLaTeX and XeLaTeX with `-shell-escape` had a line `\par` in
  it; tlmgr prints nothing for the document's own files.
- Files read outside the hooks were not in its lists on any engine: the Type 1 fonts embedded
  at shipout (amsfonts's `.pfb` files for Computer Modern, and cm-super's through the font map
  on basic), babel's `babel-en-US.ini`, and `atbegshi.sty` under beamer. prumo has them from
  `-recorder`. scheme-minimal, which DEPP's setup example starts from, brings in amsfonts but
  not cm-super.
- Its README says the list is written at `\begin{document}`, so fonts loaded later are not in
  it. 0.4 writes it from `enddocument/info`, and a font and a file first used on page 2 of a
  test document were in the list.

**Same idea:** `packages` is the `.fls`-to-package lookup from the tex-live thread, and
mkjobtexmf collects the same file list. prumo reads `texlive.tlpdb` directly (it is what
`tlmgr search --file` reads), so it makes one read, not one `tlmgr` call per file.
`prune`'s `keep.txt` and `install-tl` profile serve the purpose DEPP's `DEPENDS.txt` does. The
tools above add packages, copy files or list what to install. None of them works out what an
installation can remove.

### F. General linters

chktex, lacheck, nag, texlogsieve, texlogfilter, texfot, latexindent, TeXtidote, LTeX+, texcount,
arara, rubber, latexmk, llmk, texlab: none covers A–E. `nag` warns about *obsolete* packages,
which is adjacent and worth linking to in the unused report ("also: nag says this one is
obsolete"). SaTeX (2026) lints a document by abstract interpretation and covers parts of A,
B and E (above).

## Kernel and distribution capabilities prumo stands on

- **Hook management system** (LaTeX 2020-10-01+): `file/before|after`, `package/before|after`,
  `class/before|after`, named variants, `shipout/before|after`, `begindocument`, `enddocument`,
  `enddocument/end`. `\CurrentFile` inside file hooks; `\@currname`/`\@currext` inside package
  hooks; `\@raw@opt@<name>.<ext>` holds the raw option list (2021-06+).
- **`-recorder`**: every engine writes `<job>.fls` with `PWD`, `INPUT`, `OUTPUT` lines.
- **`texlive.tlpdb`**: plain-text package database with `runfiles` lists and `depend` edges.
- **Timers**: `\sys_timer:` in expl3 maps to `\pdfelapsedtime` (pdfTeX), `\elapsedtime`
  (XeTeX), and on LuaTeX to `os.gettimeofday()` minus an epoch that l3kernel sets to
  `gettimeofday() - os.clock()` when its Lua code loads (wall clock; finding 52). Unit:
  scaled seconds, 1/65536 s.
- **Poppler**: `pdffonts`, `pdftotext`, `pdftoppm`, `pdfinfo`.
- **latexmk** drives multi-pass builds; **mylatexformat** builds `whatif`'s precompiled
  preamble.

## Demand signals

- TUGboat 45:2 (2024): texprof article, profiling demand from package authors.
- latex3/latex2e issue #1877 (Oct 2025): "LuaLaTeX is disproportionately slow". Engine
  performance is an active topic, and a per-package breakdown is the natural next question.
- LaTeX Workshop issue #2797: user asking which TeX Live packages are "absolutely necessary"
  because the default install is gigabytes of unneeded packages.
- TeX.SE 210139 (2014-11-02, "how to find bottleneck of long compiling time for my
  document"): a 340-page document, and the asker wants "those 20% of the things which might
  cause 80% of the compiling time". With the per-page timer, about 20 of the 340 pages took
  more than half the time.
- TeX.SE 716306 (2024-04-23, "Automatically determine all ctan packages necessary to
  compile") and the tex-live thread "tlmgr search output" that followed (2024-04-24,
  messages 050431 to 050441). Joseph Wright started a script that uses `--recorder`, parses
  the `.fls` and looks up the TeX Live package for each line. Marei Peischl replied that she
  runs `tlmgr search --file` through shell-escape in a container, in a project on the Island
  of TeX GitLab. DEPP is her project there, created in January 2024, and it does that.
  Norbert Preining said he does this regularly, showed the output of
  `tlmgr search --file /article.cls`, and added `tlmgr search --json` that day. Markus Kohm
  pointed at texliveonfly. What came of it: Joseph's script as TeX.SE answer 716373,
  `tlmgr search --json`, and DEPP, public on GitLab at 0.4. CTAN's package list has nothing
  from the script as of 2026-11-12.

## Sources

- https://github.com/bazzilic/LaTeXpkges · https://pypi.org/project/latexpkges3
- https://ctan.org/tex-archive/support/latex-dependency-grapher
- https://tug.org/TUGboat/tb45-2/tb140ruckert-profiler.html
- https://github.com/EagleoutIce/satex-analyzer (SaTeX; github.com/gigalasr/satex-analyzer
  is a fork from 2026-09-26 at 0.1.0) · https://crates.io/crates/satex-analyzer
- https://gitlab.com/islandoftex/texmf/depp (run at commit bae14f2655f2)
- https://tex.stackexchange.com/questions/210139 · https://tex.stackexchange.com/a/211572
- https://tex.stackexchange.com/questions/716306 · https://tex.stackexchange.com/a/716373
- https://tug.org/pipermail/tex-live/2024-April/050431.html (thread start) ·
  https://tug.org/pipermail/tex-live/2024-April/050436.html (Preining, `tlmgr search --file`) ·
  https://tug.org/pipermail/tex-live/2024-April/050440.html (`tlmgr search --json`)
- https://ctan.org/pkg/texlogsieve
- https://ctan.org/tex-archive/support/bundledoc · https://mirrors.mit.edu/CTAN/macros/latex/contrib/snapshot/snapshot.pdf
- https://yihui.org/tinytex/ · https://github.com/cran/tinytex
- https://www.tug.org/texlive/doc/tlmgr.html
- https://github.com/tectonic-typesetting/tectonic-texlive-bundles
- https://ctan.csail.mit.edu/macros/latex/base/ltfilehook-doc.pdf
- https://github.com/latex3/latex2e/issues/1877
- https://osl.ugr.es/CTAN/macros/latex/contrib/l3experimental/l3benchmark.pdf
- https://pypi.org/project/texlivemetadata/0.1.1/ (Python wrapper around tlmgr output; reference only)
