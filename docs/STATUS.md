# Status

## Prior art credited (2026-11-12)

Docs only. Nothing in `src/`, `tex/`, `tests/` or `fixtures/` changed. Where prumo does what
an existing tool already does, the README and the design docs now say so by name, before
saying what differs.

- README: Origin says most of the pieces exist elsewhere. Related tools has SaTeX, DEPP and
  pdffonts, a "Same idea as existing tools" list and a "What differs" list. A new Built on
  section credits the hooks, `\sys_timer:`, `-recorder`, the tlpdb, Poppler, latexmk and
  mylatexformat.
- `docs/02-prior-art.md` has the same list, rows for SaTeX, DEPP, Joseph Wright's script,
  texdepend, texdeps and the TeX.SE per-page timer, and the tex-live thread of April 2024
  under demand signals. Its gaps are dated facts now, without verdicts.
  `06-unused-packages.md` got the same pass.
- SaTeX 0.2.2 and pdffonts 26.07.0 were run on TeX Live 2026. DEPP, texdepend and texdeps
  were read, not run.
- The byte-identical test compares the whole PDF, built twice in one directory with
  `SOURCE_DATE_EPOCH=0` and `FORCE_SOURCE_DATE=1`. The README says that now. The test passes
  on the TeX Live 2026 image (2026-11-12).
- CLAUDE.md no longer calls texprof pdfTeX, and "Don't reinvent" names SaTeX, texfindpkg,
  DEPP and mylatexformat.

**Blocked:** nothing. DEPP is still to be run.
**Open questions:** 13 to 18 in `08-open-questions.md`, recorded from the review. CLAUDE.md
rule 3 allows more than the test checks ("modulo /CreationDate and ID"); should it say what
the test does? Carried over: should the LuaTeX font count go in through `font.nextid()`, and
should `compare` default to three runs?

## Earlier milestones, one line each

- **Launch prep, v0.2.0 (11-09 to 10):** probe v0.2 (`docstart`, `lastpage`, environments),
  the phase table and wall-time finalization, `compare` by wall time, ten-document benchmarks
  with probe overhead and `whatif`, the `whatif` command, Related tools, public documents,
  v0.2.0 released.
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
