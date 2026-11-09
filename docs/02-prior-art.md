# 02 — Prior art

Survey done 2026-11-06. Verdict first: none of the six areas has a mature, maintained,
general-purpose tool. Two have narrow, usable prior art (dependency bundling; install-on-demand).
The rest have scripts, building blocks, or abandoned projects.

## By area

### A. Unused `\usepackage` detection

| Tool | Approach | Gap |
|---|---|---|
| LaTeXpkges / latexpkges3 (Python, PyPI) | Remove each top-level `\usepackage`, rebuild, compare PDF hashes (or XDV diff) | Top-level only (by its own README); hash comparison trips on timestamps; ignores packages loaded by a class |
| latex-dependency-grapher v1.0 (Java, CTAN 2024-01) | Static scan of `\usepackage`/`\RequirePackage`/`\input`; flags declared-but-unused; GraphViz output | Static; no compile data; no release since 1.0 |
| cmdtrack, refcheck, chklref, checkcites | Find unused *commands*, *labels*, *citations* | Not packages |
| texlab, LaTeX Workshop, TeXstudio, LTeX | Editor diagnostics | No unused-package diagnostic |

**Reuse:** nothing to reuse; the idea of "remove and rebuild" is right, the execution is not.
prumo blocks packages at the kernel level (works for class-loaded packages too), seeds candidates
from probe data, and compares page text, not bytes. See `06-unused-packages.md`.

### B. Compile-time profiling

| Tool | Approach | Gap |
|---|---|---|
| **texprof / texprofile** (Ruckert; TUGboat 45:2, 2024; in TeX Live) | Modified pdfTeX/eTeX engine records time per input line and per macro into `.tprof`; `texprofile` reports by file/line/macro with call-graph data | pdfTeX-only; a different engine binary, not your normal one; not package-oriented |
| l3benchmark (`\benchmark:n`, `tic`/`toc`) | expl3 timers you place by hand | Building block |
| regstats, scrtime | Whole-run time | No breakdown |
| ConTeXt `mtxrun --script profile` | Lua/TeX profiling | ConTeXt only |
| latexmk | Build orchestration | No timing beyond `time latexmk` |

**Reuse:** `\sys_timer:` (expl3, all engines) — verified. Point users at texprof for per-macro
work; prumo is the engine-agnostic package/file/page layer above it. Ruckert's article notes that
no profiling tool had been available to macro package authors before texprof — that is the demand
signal.

### C. Package dependency graph

| Tool | Approach | Gap |
|---|---|---|
| latex-dependency-grapher | Static scan, kpsewhich resolution | Misses conditional/option-driven loads, kernel and class loads |
| snapshot + bundledoc | `\listfiles`-style flat `.dep` list, then archive the files | Flat; no edges |
| `\listfiles`, adhocfilelist, inputtrc | Flat lists / traces | No graph |
| `-recorder` `.fls` | Every file read/written | Flat; no nesting |
| texlogsieve (Lua, 1.6.1, 2026-01) | Rebuilds file nesting from the log's parentheses to filter messages | Does not export the tree; but its parser is the reference for log-based nesting |

**Reuse:** nothing directly; the `package/before` hook plus the depth counter gives exact edges
(verified). Use texlogsieve's approach as the cross-check parser for logs from runs without the
probe.

### D. Font usage

| Tool | Approach | Gap |
|---|---|---|
| pdffonts (Poppler) | Lists fonts embedded in the PDF | Only the output side; Type 3 fonts are anonymous (`F44`) |
| tracefnt / `\tracingfonts` | NFSS load/substitution trace in the log | Raw; no report |
| pdfTeX log trailer | `<...pfb>` list of embedded Type 1 files | Hand parsing |

**Reuse:** pdffonts as the cross-check. The `.fls` is the better source on pdfTeX: `.tfm` INPUT =
loaded, `.pfb`/`.pk` INPUT = embedded (verified). Nothing links either to the package that caused
the load; prumo can, via the `.fd` file hook context.

### E. Slimming / pruning TeX Live

| Tool | Approach | Gap |
|---|---|---|
| mkjobtexmf (Oberdiek, v0.8, 2011) | Run with `-recorder`/strace, copy used files into a job-local texmf tree | File level, one job, "experimental" by its own docs |
| snapshot + bundledoc (Pakin) | Archive a document's dependencies | For archiving, not for an install |
| texliveonfly (2011) | Install missing packages via tlmgr while compiling | Add-only |
| TinyTeX (Yihui Xie; R) | Small TeX Live + auto-install missing packages from log | Add-only |
| tlmgr | `scheme-minimal`, `scheme-infraonly`, `remove`, `search --global --file`, pinning, `--usermode` | No "remove what these documents don't use" |
| Tectonic bundles | Self-contained bundles; custom bundles from a hand-written spec | No usage-driven subsetting |
| MiKTeX | Install on the fly | No pruning |

**Reuse:** `texlive.tlpdb` as the file→package map (it is what `tlmgr search --file` reads).
Everything else only adds or copies. The inverse — subtract what the corpus doesn't use — is new.

### F. General linters

chktex, lacheck, nag, texlogsieve, texlogfilter, texfot, latexindent, TeXtidote, LTeX+, texcount,
arara, rubber, latexmk, llmk, texlab: none covers A–E. `nag` warns about *obsolete* packages,
which is adjacent and worth linking to in the unused report ("also: nag says this one is
obsolete").

## Kernel and distribution capabilities prumo stands on

- **Hook management system** (LaTeX 2020-10-01+): `file/before|after`, `package/before|after`,
  `class/before|after`, named variants, `shipout/before|after`, `begindocument`, `enddocument`,
  `enddocument/end`. `\CurrentFile` inside file hooks; `\@currname`/`\@currext` inside package
  hooks; `\@raw@opt@<name>.<ext>` holds the raw option list (2021-06+).
- **`-recorder`**: every engine writes `<job>.fls` with `PWD`, `INPUT`, `OUTPUT` lines.
- **`texlive.tlpdb`**: plain-text package database with `runfiles` lists and `depend` edges.
- **Timers**: `\sys_timer:` in expl3 maps to `\pdfelapsedtime` (pdfTeX), `\elapsedtime`
  (XeTeX), `os.clock()` (LuaTeX). Unit: scaled seconds, 1/65536 s.

## Demand signals

- TUGboat 45:2 (2024): texprof article — profiling demand from package authors.
- latex3/latex2e issue #1877 (Oct 2025): "LuaLaTeX is disproportionately slow" — engine
  performance is an active topic; a per-package breakdown is the natural next question.
- LaTeX Workshop issue #2797: user asking which TeX Live packages are "absolutely necessary"
  because the default install is gigabytes of unneeded packages.

## Sources

- https://github.com/bazzilic/LaTeXpkges · https://pypi.org/project/latexpkges3
- https://ctan.org/tex-archive/support/latex-dependency-grapher
- https://tug.org/TUGboat/tb45-2/tb140ruckert-profiler.html
- https://ctan.org/pkg/texlogsieve
- https://ctan.org/tex-archive/support/bundledoc · https://mirrors.mit.edu/CTAN/macros/latex/contrib/snapshot/snapshot.pdf
- https://yihui.org/tinytex/ · https://github.com/cran/tinytex
- https://www.tug.org/texlive/doc/tlmgr.html
- https://github.com/tectonic-typesetting/tectonic-texlive-bundles
- https://ctan.csail.mit.edu/macros/latex/base/ltfilehook-doc.pdf
- https://github.com/latex3/latex2e/issues/1877
- https://osl.ugr.es/CTAN/macros/latex/contrib/l3experimental/l3benchmark.pdf
- https://pypi.org/project/texlivemetadata/0.1.1/ (Python wrapper around tlmgr output; reference only)
