# prumo — handoff pack

> This is the handoff pack as received on 2026-11-06, kept as the design reference. The paths
> below refer to the repo root (`tex/`, `scripts/`, `fixtures/` live there; this file set is
> `docs/`). `STATUS.md` has what has happened since.

**prumo** (Portuguese: *plumb line* — the builder's tool for checking that a wall is true) is a build
analyzer for LaTeX. One instrumented compile tells you where the time goes, which package loaded
which, which fonts were loaded but never set, and exactly which TeX Live packages your documents
need — so you can tune your classes and shrink your installation. A second, slower pass finds
`\usepackage` lines you can delete.

It is not a compiler and not an editor. It sits next to whatever you already use (`pdflatex`,
`latexmk`, TeXstudio) and reads what the build leaves behind.

Working codename. Rename is a single find-and-replace (see `08-open-questions.md`, Q1).

## What is in this pack

| File | What it is |
|---|---|
| `CLAUDE.md` | Charter for Claude Code sessions in the repo: conventions, rules, gates. Move to repo root. |
| `01-vision.md` | Problem, audience, the original request, non-goals |
| `02-prior-art.md` | What exists, what to reuse, what not to re-implement |
| `03-architecture.md` | The one-pass probe, data model, CLI surface, crate layout |
| `04-probe.md` | The LaTeX side: `prumo.sty`, record format, engine notes, known traps |
| `05-packages-and-pruning.md` | `.fls` → `texlive.tlpdb` → package set → `tlmgr` prune plan |
| `06-unused-packages.md` | Differential compilation design (phase 2) |
| `07-phases.md` | M0–M6 with acceptance criteria and demo scripts |
| `08-open-questions.md` | Decisions for Kevin, not for Claude Code to guess |
| `09-glossary.md` | TeX ecosystem terms used in this pack, for picking the TeX side up |
| `10-verified-findings.md` | Lab notes: what was actually run and measured while writing this pack |
| `tex/prumo.sty` | **Working** probe package, tested on pdfTeX, XeTeX, LuaTeX (TeX Live 2023) |
| `scripts/prumo_report.py` | **Working** reference analyzer (~200 lines, no deps). The oracle the Rust port must match. |
| `fixtures/` | Three test documents with known answers, plus a hand-written `tlpdb` excerpt |

## Status

Verified in a sandbox on 2026-11-06 (TeX Live 2023/Debian, LaTeX 2023-11-01):

- the hook-based probe records every file, package, class, page and phase with timings on all
  three engines, without touching the document source;
- `-recorder` output lists TFM, PFB, PK, `.fd` and `.map` files, which is enough to tell loaded
  fonts from embedded ones on pdfTeX;
- blocking a package with `\@namedef{ver@<pkg>.sty}{}` makes the differential pass work without
  editing the source;
- the reference analyzer produces a sensible report: on the custom-class fixture, `siunitx` is
  82 ms of a 195 ms build; on the basic fixture `hyperref` is 100 ms of 222 ms.

Not verified: `texlive.tlpdb` parsing (Debian's TeX Live has no tlpdb). Format is documented from
the TeX Live sources in `05-packages-and-pruning.md`; verify against a real tlpdb in M0.

## First session with Claude Code

Drop this folder into the new repo as `docs/`, move `CLAUDE.md` to the root, move `tex/`,
`scripts/` and `fixtures/` to the root too. Then:

```
Read CLAUDE.md and docs/07-phases.md. Start M0. Before writing Rust, run
fixtures through tex/prumo.sty and scripts/prumo_report.py on this machine
and confirm docs/10-verified-findings.md still holds on this TeX Live.
Answer nothing in docs/08-open-questions.md yourself; ask me.
```

Answer Q1–Q4 in `08-open-questions.md` before the session starts; they are all M0 decisions.

## How this relates to florilegium

Different repo, same shelf: florilegium is the TeX *output* side (what you can draw), prumo is the
TeX *build* side (what it costs). Cross-link the READMEs; prumo's first demo document can be a
florilegium page, which is a nice stress test (TikZ is slow and the profiler will say so).
