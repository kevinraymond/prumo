# Status

## Launch prep (2026-11-09 and 10)

This round adds a table of where a LaTeX run's time goes, from engine start to the closed
PDF, on each engine. Everything below was measured, and the method is in the repo so anyone
can check it.

### Probe v0.2

- New records: `docstart` (end of `\begin{document}`), `lastpage` (after the final
  `\clearpage`), and `env+`/`env-` for the environments named with
  `\PrumoEnvironments{...}`. The CLI passes `--env` (default: tikzpicture, axis, tcolorbox,
  tblr, longtable, tabularx, minted, lstlisting, forest; `none` for none). A command and not
  a package option, because raw option handling changed since the 2021-06 floor.
- `docstart` and `end` go through `\AddToHookNext`, so they come after the code 92 TeX Live
  files add to `begindocument/end` (amsmath among them) and after document-level code
  (finding 53).
- A `ship` with no `page` is a discarded shipout. The model keeps it apart and the pages
  section says so.
- Loaded from the main file's first line (Overleaf), the probe dropped its first event and
  recorded its own `pkg-`. Both fixed (finding 56).
- PDF byte-identical with and without the probe on the seven fixtures and three engines, on
  TeX Live 2026 and on Ubuntu 22.04's TeX Live 2021 (kernel 2021-11-15). New fixture
  `environments` with seven claims, all tested.
- Oracle first: `prumo_report.py` has the phases, environments and discarded shipouts, the
  goldens are regenerated, and the Rust matches on all 19 TeX Live 2026 sets.
  `make_artifacts.sh` now sets `FORCE_SOURCE_DATE=1`; beamer's `\today` had changed a font
  subset from one day to the next.

### Phases, and how time is measured

- `time` leads with the phase table (text, `--json` `phases`, `--html`): startup, preamble,
  begin-document, typesetting, shipout, end-document, finalization. JSON adds `timer`,
  `phases`, `environments` and `discarded_shipouts`; nothing was renamed.
- Finalization is the engine process's wall time minus the last record. `run.json` keeps
  `wall_ns`, polled every millisecond (it was 20). Not measured under latexmk.
- The brief said LuaTeX's timer is `os.clock()`. It is not: l3kernel reads
  `os.gettimeofday()` and uses `os.clock()` only to place the zero (finding 52). All three
  timers are wall clock, and the README says how each is read.
- `compare` has a phases-by-engine table and ranks by wall time. The old total left out
  startup and finalization and called LuaLaTeX fastest on a one-line document it was
  slowest on (finding 55). With `--runs` 3 or more the median warm run is reported.

### Benchmarks (`docs/benchmarks/`)

- `run.sh` runs `bench.py` in the full TeX Live 2026 image pinned to one P-core of the
  13900K: four documents (one line; a five-page paper with biber, pgfplots and siunitx;
  florilegium's manual; a 3,991-page generated book), three engines, a cold run with fresh
  font caches and five warm runs (three for the book), medians per phase. Results and
  method in `docs/benchmarks/README.md`; the widest spread is 4%.
- On the one-line document LuaLaTeX spends 235 of 315 ms starting up, 22 ms on the preamble
  and 54 ms writing the PDF. On the paper it is 236, 763 and 135 of 1,392 ms (pdfLaTeX 855
  ms in all). On the book LuaTeX's shipout is 2.7 ms a page against pdfTeX's 0.7. A cold
  LuaLaTeX run is 4.4 s instead of 0.3 s.
- Scale: the book's 8,669 records are read in 3 ms, and `--html` writes 1.3 MB in 8 ms.
  Plain output now lists the slowest `--top` pages instead of all 3,991.

### Public documents

ClassicThesis, the metropolis demo and the Llama 2 paper's arXiv source run through every
report command. kaobook's documentation fails on TeX Live 2026 with or without prumo. What
they changed: a failed build prints its first `!` error, plain output lists the slowest
`--top` pages instead of all of them, and the shipout hint names page and image writing, not
only hooks (finding 58).

### Release check

- v0.1.1 has its ten assets. `install.sh` from the README works in a clean Debian container,
  `install.ps1` on the Windows box, `cargo install prumo` in `rust:1-slim`.
- `doctor` without LaTeX now says where to get it, folds the four Poppler rows into one that
  says what is lost, and names MiKTeX when there is no tlpdb.
- The crate is 0.2.0 now. Not tagged: that is mine to do, and the release workflow then
  publishes the binaries, images and crate.

### Related tools

texprof, latexpkges3, latex-dependency-grapher, texfindpkg, stygtfo, TinyTeX, texliveonfly
and mkjobtexmf were each installed and run on TeX Live 2026. The README has a section on them
and a paragraph on texprof. texprof is not a pdfTeX: it is TeX with e-TeX and Prote, writes
DVI, and could not build a LaTeX format against the 2026-11-01 kernel. `docs/02-prior-art.md`
is corrected; CLAUDE.md still says "pdfTeX only".

### Only if cheap

- `prumo time output.prumo` and `prumo graph output.prumo` read records from a build prumo
  did not run, and build nothing. `docs/overleaf.md` says how, from Overleaf's source; not
  tried on overleaf.com.
- LuaLaTeX font definitions per page: the `luaotfload.patch_font` hypothesis is wrong for
  `\addfontfeature`, so nothing shipped (finding 57).

**Blocked:** nothing. **Needs me:** the v0.2.0 tag. **Open questions:** should the
LuaTeX font count go in after all, through `font.nextid()`? Should `compare` default to three
runs, so it reports a median?

## Earlier milestones, one line each

- **M6, polish and release (11-07 to 09):** `--html`, terminal charts, artifacts in the cache
  directory, `docker/prumo`, release images and binaries, the public repo at v0.1.0 then
  v0.1.1 (crate under trusted publishing), Windows verified on MiKTeX, one-line installers,
  the API hygiene pass checked byte for byte with `scripts/same_output.sh`.
- **Hardening 2 (11-07):** `--build <cmd>`, Windows `.fls` paths, the beamer, biblatex and
  fontspec fixtures, `unused` removes the request instead of blocking, `unused --corpus`,
  timeouts that kill. Prune gate: 5,263 → 184 packages, 16/16 rebuilds identical.
- **Hardening 1 (11-06):** florilegium as the first corpus: the ink check in `unused`, the
  `ship` record and shipout split, body hints, hard links for big files.
- **M5, unused (11-06):** candidates from the records, blocked or removed and rebuilt in
  parallel, judged by exit, page count, text and render, cached verdicts.
- **M4, prune (11-06):** corpus keep set with a reason per package, `tlmgr remove` plan,
  `--apply --yes`, the Docker prune gate.
- **M3.5, compare (11-06):** every engine, two runs each, one table.
- **M3, fonts (11-06):** TFMs against the PDF through the map file, Type 3 and bitmap warnings,
  OpenType pairing.
- **M2, graph, log cross-check, packages (11-06):** tree, DOT and JSON; `texlog`;
  `doctor --check-log`; the tlpdb package set with attribution.
- **M1, the probe and `prumo time` (11-06):** the model's five rules, matching the oracle
  exactly; the instrumented run on a copy; `time` text and JSON.
- **M0, skeleton and parsers (11-06):** name, licenses, repo layout, the parsers, `doctor`,
  artifacts for TeX Live 2026 and Debian's 2023.
