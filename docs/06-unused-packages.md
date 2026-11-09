# 06 — Unused packages (phase 2)

The question "can I delete this `\usepackage`?" cannot be answered by reading the source,
because packages act by side effect (microtype, hyperref, babel, fontenc, geometry), by
redefining existing commands (amsmath's `\boldsymbol`, `\eqref`), or by being loaded for a
single option. The honest answer is experimental: remove it, rebuild, see what changes. Prior
tools do that badly (top-level only, hash of PDF bytes). prumo does it properly.

## Candidates

From the probe tree, a candidate is any `pkg` frame whose parent is `main.tex` or the user's
own class/package (anything under `Project` in the `.fls`, not `TexmfDist`). Dependencies pulled
in by distribution packages are never candidates — removing `translations` from under `siunitx`
is not the user's call.

For each candidate, prumo knows (from `opts`) the raw option list, which it needs to block the
package without an option clash.

Skip list, reported as "loaded for side effects — not tested": `fontenc`, `inputenc`, `babel`,
`polyglossia`, `fontspec`, `microtype`, `geometry`, `hyperref`, `bookmark`, `lmodern` and other
pure font-selection packages, `etoolbox`/`xparse`/`expl3` (kernel-adjacent), `nag`, `showframe`,
`layout`, `lipsum`? (no — lipsum is testable). Keep the list short and editable
(`--test-all` overrides it). Hard-coding judgement is a smell; the list exists only because
these packages will always "pass" the no-change test while obviously doing something.

## Removing the request (the default since hardening 2)

The experiment the user would run: the candidate's `\usepackage`/`\RequirePackage` request is
deleted from its loader's source (`main.tex`, or the user's own `.cls`/`.sty`) in the temp
copy, then the same build runs. A one-name request disappears; from `{a,b,c}` the name is
dropped; comments are left alone. The user's files are never touched. This is exact where
blocking is not: biblatex checks `\@ifpackageloaded{csquotes}` and then uses csquotes's
macros, so a blocked csquotes made the build fail ("Outdated 'csquotes' package" with a bare
marker, "Undefined control sequence" with a dated one) while deleting the line builds fine
(finding 48).

## Blocking a package (verified; the fallback)

When the request is not found in the loader's source (split over lines, generated), prumo
falls back to defining the kernel's "already loaded" marker before the document runs:

```
pdflatex -jobname=main \
  "\makeatletter\@namedef{ver@xcolor.sty}{9999/12/31 blocked by prumo}\@namedef{opt@xcolor.sty}{}\makeatother\input{main}"
```

`\usepackage{xcolor}` (anywhere — preamble, class, another package) now becomes a no-op. The
`opt@` definition must hold the option list the document actually requests (from the `opts`
record) or LaTeX raises "Option clash". In the differential run prumo additionally keeps the
probe loaded so it can detect whether anything *else* changed in the load graph.

Verified outcomes on the basic fixture:
- block `xcolor` → compiles, `pdftotext` identical → **removable**
- block `amsmath` → `! LaTeX Error: Environment align undefined.` → **required**

## Verdicts

| Verdict | Evidence |
|---|---|
| **required** | build fails (nonzero exit, or an error in the log that the baseline did not have) |
| **affects output** | build succeeds; page count, `pdftotext -layout` text, or rendered pages differ |
| **affects metadata** | text identical; `pdfinfo`/bookmarks/links differ (hyperref family) |
| **no observable effect** | everything identical → safe to remove, with the standard caveat |
| **loaded anyway** | everything identical, and the candidate build's records still show the package, loaded by another one (`graphicx` under `tikz`): the document may use it, the line is redundant |
| **already loaded** | a distribution unit had loaded it before the line ran (`amsmath` under beamer); nothing rebuilt, the line is redundant |
| **not tested** | skip list |

"Loaded anyway" exists because "no observable effect" was misread: florilegium's manual calls
`\includegraphics` and still passed the test without its `\usepackage{graphicx}`, because tikz
loads graphicx. Saying the package does nothing would have been wrong; the line is what is
redundant. The blocking fallback leaves no `pkg` frame, so it cannot produce this verdict.

`--compare text` (default) uses page count + `pdftotext -layout` diff. `--compare render`
additionally rasterizes with `pdftoppm -r 50` and compares pages pixel-wise with a small
tolerance; catches geometry/microtype-class effects at ~10× the cost. Always show *what*
differed: the first differing line, or the first differing page.

## Build strategy

- Baseline: one full build with the user's command (`--build`, default `latexmk -pdf`), in a
  temp copy of the project directory (`cp -r`, or hardlinks for big image trees).
- Per candidate: a fresh temp copy, same command, with the request removed from the loader's
  copy (or, as a fallback, the blocking preamble injected via `-usepretex`). Candidates are independent → `rayon` parallelism, `--jobs N`
  (default: physical cores / 2, because latexmk is itself multi-process when it runs biber etc.).
- Cost: `1 + N` full builds. Say so up front (`14 candidates × ~3 s = ~45 s`) and show progress.
- Time limit per build (`--timeout`, default 10× baseline) to survive infinite-loop failures.
- Caching: key on (candidate, hash of sources + options). Re-running after editing the
  document only rebuilds affected candidates.

## Interactions to handle

- **Pairs**: removing A alone and B alone may each be fine while removing both breaks the
  document (A provides a fallback B also provides). v1 ignores this and says so; `--pairs` is a
  possible later feature (quadratic).
- **Packages that only matter with `-shell-escape`, biber, makeindex**: the user's build
  command carries those; prumo runs the same command, so they are covered.
- **`\usepackage` inside `\if...` branches**: the probe only sees the branch that ran. Covered by
  "the corpus is only as complete as the builds you ran".
- **Class-level candidates**: for the custom-class user this is the point — `obra.cls` loads
  `enumitem` that the document never uses. Report it under the class ("obra.cls loads 2 packages
  this document does not use: enumitem, fancyhdr?"). Note that a class serving many documents
  should be evaluated against the corpus (`prumo unused --corpus 'docs/**/*.tex' obra.cls`:
  a package is removable from the class only if every document passes). That is the v1.1 feature
  that makes this tool valuable to the original requester; design the per-document result so it
  merges.
