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

- Ten documents now, three engines. Three live here (one line, the five-page paper, the
  3,991-page generated book). Seven are fetched at a pinned commit and never stored: the
  ACM sigconf sample with BibTeX and with biblatex, the metropolis demo deck, florilegium's
  manual, the HoTT book, Open Logic and the 7,710-page Stacks Project. `corpus.toml` has
  each one's commit, license, engines and prepare step, `fetch.py` lays them out. kaobook
  was left out, because thmtools' `sibling=` fails on the 2026-11-01 kernel, in five lines
  too.
- `bench.py` has three modes: the phases (as before), `--overhead` (builds with and without
  the probe in turns, plus per-package times from the probed records) and `--whatif`.
  `charts.py` draws four SVG charts, light and dark, from the JSON. Color means phase in
  all of them, and every pair of colors, the gray included, is checked with the dataviz
  validator.
- The probe costs 7 ms on the one-line document and under 6% everywhere. On the paper,
  LuaLaTeX's extra preamble time is biblatex (+151 ms) and fontspec's LuaTeX code (67 ms).
- On the corpus, the precompiled preamble works on two of the six pdfLaTeX
  documents (paper 49%, deck 25%). Draft pays on XeLaTeX (13 to 38%). Externalize pays on
  the deck and florilegium (15 to 32%), but its first run on a long book is a pass per
  picture: 93 minutes on Open Logic for 4 s a build.
- The run had seven P-cores busy at once, and its phase numbers came out 0.3 to 5% slower
  than the quieter run before it. The README says so.

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
- v0.2.0 is released (2026-11-10, after the `whatif` PR). The release has its ten assets,
  crates.io has 0.2.0, and ghcr has `0.2.0` and `0.2.0-small` for amd64 and arm64. The
  one-line installer from the README installs 0.2.0 in a clean Debian container.

### Related tools

texprof, latexpkges3, latex-dependency-grapher, texfindpkg, stygtfo, TinyTeX, texliveonfly
and mkjobtexmf were each installed and run on TeX Live 2026. The README has a section on them
and a paragraph on texprof. texprof is not a pdfTeX: it is TeX with e-TeX and Prote, writes
DVI, and could not build a LaTeX format against the 2026-11-01 kernel. `docs/02-prior-art.md`
is corrected; CLAUDE.md still says "pdfTeX only".

### whatif

- `prumo whatif main.tex [--try preamble,draft,externalize] [--runs 5] [--shell-escape]`.
  Each experiment rebuilds its own copy of the directory with one change.
  The timed builds run without the probe: a warm-up each, then the baseline and the variants
  in turns. Medians are compared. A difference no larger than the larger spread (2 ms at
  least, the polling resolution) is "within noise". The output is compared by page count,
  the words on each page and a 24 dpi render (finding 63). `--json` has the same fields
  under schema 1.
- The builds run one at a time, not on `unused`'s parallel pool, because parallel builds
  slow each other down. The temp copy, the timeout and the text comparison come from `run`
  and `unused`.
- `preamble` works on pdfTeX (the fixture goes from 244 to 91 ms). XeTeX and LuaTeX fail,
  and the row gives the error (finding 60). `doctor` has a mylatexformat row and the TeX
  image installs it.
- `externalize`: turned on from the command line alone, the external library made every
  picture build typeset the whole document (finding 61). The system call now carries the hook and names the engine. The text
  is identical on all three engines, and ten pictures went from 944 to 138 ms. Without
  `--shell-escape` it is refused when asked for and not run by default. The table and the
  README mark it experimental.
- New fixture `whatif`, eight claims, all tested. `make_artifacts.sh whatif` regenerates one
  fixture now. The fixture turned up finding 62: `doctor --check-log` now tells an
  existence check from a read.
- Defaults: `--runs` is at least 2. The comparison uses the last run's PDF. Logs go to
  `<cache>/<job>/whatif/<experiment>/`. With `--shell-escape` the probed run is
  `--build "<engine> -shell-escape"`, so `time` rebuilds afterwards. No `--latexmk` or
  `--build`, since their wall time is several passes.
- `cargo test`, clippy and fmt pass, and so do the 19 TeX tests in the image. TeX Live 2021
  runs it too (on a copy of the fixture without siunitx, which that image lacks).

### Only if cheap

- `prumo time output.prumo` and `prumo graph output.prumo` read records from a build prumo
  did not run, and build nothing. `docs/overleaf.md` says how, from Overleaf's source; not
  tried on overleaf.com.
- LuaLaTeX font definitions per page: the `luaotfload.patch_font` hypothesis is wrong for
  `\addfontfeature`, so nothing shipped (finding 57).

**Blocked:** nothing.
**Open questions:** should the LuaTeX font count go in after all, through `font.nextid()`?
Should `compare` default to three runs, so it reports a median?

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
