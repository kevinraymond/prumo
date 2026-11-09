# Status

## M6 — polish and release (2026-11-07 to 2026-11-09)

Where things stand: every command exists, the charts are in, the artifacts live in the cache
directory, and v0.1.0 is public: one commit, CI green, binaries for five targets, two images
for two architectures. crates.io is the last install path before I announce it.

### `--html` (2026-11-07)

- `prumo time main.tex --html [FILE]` writes one self-contained page: inline CSS, a few lines
  of JS for the tabs, no network. It has a flame chart of the load tree over time, the unit
  table, the pages table, the tree as collapsible lists, and the fonts tab built from the
  same inputs as `prumo fonts`. Light and dark. Tested on every artifact set (one chart group
  per frame, names escaped, no external resources) and the SVG checked as XML.
- Checked in headless Chromium on the beamer and basic pages. The first render hid the main
  file's row and clipped the last axis label. Both fixed.
- First page on a real document, florilegium's manual on LuaLaTeX (11-08). `--html` with no
  FILE was refused by clap, since `PathBuf` rejects an empty default. Now `Option<Option<_>>`.
  The cold-cache run showed 4.8 s in `size11.clo`. That was luaotfload building its font
  name database at the first font selection. `time` now warns when the `.fls` has
  `luaotfload-names.*` as an output and names the unit holding the time (finding 51).
- I looked at the page and kept the flame chart first. Same day, Q5: the minimum kernel is
  2021-06-01, `doctor` refuses older ones, no degraded mode.
- `unused` on the manual said `graphicx` had no observable effect, while the manual calls
  `\includegraphics`. tikz loads graphicx anyway. New verdict, **loaded anyway** (`by`), read
  from the candidate build's records. Counted as removable, reported as a redundant line
  (finding 50). `tests/tex.rs::loaded_anyway_is_told_apart_from_unused`.

### Artifacts out of the document's directory (2026-11-08)

- A stray `.prumo/` showed up untracked in the florilegium repo, and corpus runs sprinkled
  one into every directory. So: artifacts go to the user's cache directory now
  (`$XDG_CACHE_HOME` or `~/.cache`, `~/Library/Caches`, `%LOCALAPPDATA%`, plus `prumo`).
  `PRUMO_CACHE` overrides the root and `--out` overrides one run. Each document gets
  `<job>-<FNV key of the absolute path>`. `prune` defaults to `<root>/prune`.
- Every command prints the directory it used, cache hit or fresh build. `prumo where
  [main.tex]` answers later. `--html` with no FILE writes `./<job>-prumo.html`.
- The user image sets `PRUMO_CACHE=/doc/.prumo` so artifacts survive the container, and
  `scripts/tex_tests.sh` points the cache into its throwaway home. Rule 1 in CLAUDE.md
  reworded to match.
- **`prumo clean`** (board #5267) lists what the cache root holds, largest first: documents
  with age and size, the `prune` directory, and strangers. `--stale` picks documents that no
  longer exist, a name picks one document, and nothing is deleted without `--yes`. `--json`
  too. `doctor` gained a `cache` row with the count and size. One wart: inside a container the
  recorded document path is the container's, so judge `--stale` from the same mount.

### First run as a new user (2026-11-08)

I tried the image the way a stranger would. Three things were not obvious, all fixed:

- Nothing said I was in an image. Now `PRUMO_IMAGE` gives doctor an `image` row and the
  build line says "in the X image".
- Printed paths were the container's (`/cache/...`). `PRUMO_PATH_MAP` translates them to the
  user's, and without a map they are marked "(inside the image)".
- Without `--engine` the default was pdflatex, which fails on a LuaLaTeX document. `--engine`
  now defaults to the `% !TEX program =` line, the build line says where the engine came
  from, and a defaulted pdflatex that dies on `\directlua` or fontspec names the engine to
  try. `tests/tex.rs::magic_comment_picks_the_engine_on_the_cli`. The magic comment went into
  all eleven florilegium documents.

### Terminal charts (2026-11-08)

- In a UTF-8 terminal `time` leads with the build over time: the main file, the preamble
  folded into one row, and a pages row of `░` typesetting and `▓` shipout. `┆` marks the end
  of the preamble and `\end{document}`. Bars are eighth-cells on a shared axis. The unit
  table gets a bar column and the pages section becomes a sparkline plus the five slowest
  pages. `chart.rs`, no new crate. Width from `COLUMNS` or `tput cols`.
- The preamble is the same every build, so it stays folded. `--preamble` zooms the axis to
  `\begin{document}` with one row per frame down to `--depth`.
- Color by kind as on the HTML page. `NO_COLOR` drops it, `--plain` or a pipe gives the old
  text, `--json` untouched.
- The same fold on the HTML page: the flame chart shows the main file, one `preamble (N
  units)` row, the files read in the body at their depth, and the pages strip. The full
  preamble chart sits under a `<details>` below it. Click the preamble row or the summary
  line to open it. No JS needed, since the summary is a native disclosure. README screenshot
  regenerated.

### `docker/prumo`, the command (2026-11-08)

- The README had a shell function to paste into your startup files. Nobody wants that, and
  mine was gone the next day because I never saved it. Now it is a 20-line `sh` script you
  copy onto `PATH`. It mounts the current directory and the cache directory at their own
  paths (the identity map is passed so paths are not marked "inside the image"), runs as
  you, and passes `-t` only when stdout is a terminal. `PRUMO_IMAGE`, `PRUMO_CACHE`,
  `PRUMO_DOCKER` (podman) and `PRUMO_DOCKER_OPTS` are the knobs.
- `tests/wrapper.rs` runs it with `PRUMO_DOCKER=echo` and checks the `docker run` line. The
  doctor `image` row now describes the mounts instead of assuming `/doc`.
- `docker/prumo.cmd` is the Windows counterpart for Docker Desktop. `C:\x\y` is mounted at
  `/C/x/y`, the map carries both names, and `host_path` joins with backslashes when the host
  side is a Windows path. `-t` only when PowerShell says stdout is not redirected. No
  `--user`, since Docker Desktop owns the bind mounts. Written blind, like the Windows `.fls`
  reading (finding 47). Nobody has run it on Windows yet.
- A document outside the mounted directory now says so in the image, instead of a bare
  "not a file".

### Release image (#5228, 2026-11-08)

- `docker/Dockerfile.user` builds itself now. A `rust:1-bookworm` stage compiles prumo on
  the build machine and cross-links for the target with the GNU cross toolchain, so a
  two-platform build does not run rustc under emulation. The `IMAGE_NAME` build arg sets what
  doctor calls the image.
- `.github/workflows/release.yml` runs on a `vX.Y.Z` tag. Full and small variants, each for
  linux/amd64 and linux/arm64, each smoke-tested through `docker/prumo` on a fixture (doctor,
  `time --json`, `--html`) before the push. A manual run pushes `:edge`.
- Verified here: native build 17 s warm, and the arm64 builder stage produces an aarch64
  binary. `rust-version` corrected to 1.88 (let chains).

### Private repo, CI and releases exercised (2026-11-08 and 09)

`kevinraymond/prumo` is on GitHub, private, to shake out the workflows before anything goes
public. Here is what the first runs caught:

- CI's TeX job failed. TeX Live had moved `nameref.sty` out of hyperref into its own package,
  so the `.fls` shape differed from the checked-in artifacts. I regenerated the artifacts,
  added the `nameref` stanza (and the current hyperref one) to `fixtures/tlpdb/excerpt.tlpdb`
  (129 packages now), and pinned `docker/Dockerfile.tex` to the base image's digest. CI and
  the artifacts now agree until the pin is bumped on purpose.
- The shape check itself was wrong. It compared signed fields, so every timing change tripped
  it. It now pairs removed and added lines by kind and last field.
- Second run: CI had been building the TeX image without `target: clean`, so buildx took the
  last stage, the one without cm-super. pdflatex used bitmap `.pk` fonts where the artifacts
  have cm-super Type 1. Fixed.
- `release-binaries.yml` is new: tests at the tag, a GitHub release (prerelease on a `-`
  tag), then musl Linux x86-64 and arm64 via `cross`, macOS on both arches, and Windows. The
  first run uploaded macOS and Windows and failed on Linux because the binary and the wrapper
  are both named `prumo`. The wrapper is out of the archives.
- The `small` image built, smoke-tested and pushed for both architectures on the first try.
  The `full` one ran out of disk on the two-architecture build. The workflow now frees the
  runner's preinstalled toolchains first (20 GB) and drops the smoke image after its test,
  which leaves 27 GB spare. Prerelease tags no longer move `latest` or `small`.
- CI gained a `cargo package --locked` job as the crates.io dry run. The crate is 0.5 MB.
- At `v0.1.0-rc.2` everything is green. CI on all four jobs, ten release assets (five targets
  with sha256), and `ghcr.io/kevinraymond/prumo:0.1.0-rc.2` plus `-small` for amd64 and
  arm64. I pulled the small one here and ran it through `docker/prumo` on a fixture. Private
  packages on ghcr need `docker login ghcr.io` with a `gh auth token`. Public ones will not.

### Idiomatic review before going public (2026-11-09)

Three read-only reviewers went over all of `src/` plus a clippy pedantic pass. Verdict from
all three: idiomatic Rust 2024, the gaps are structural, not stylistic. Fixed before the
first public commit:

- Errors that lost the file name or lied about the cause. A corrupt `run.json` now names
  the file, a missing `.fls` has its own variant, and `.fls` or model errors are no longer
  reported as "the probe never ran". The wrapper variants are `transparent`, so `source()`
  works. `texlog::parse_file` names the file too.
- `records::validate` was only called by tests. `from_records` calls it first now, and the
  two durations use `saturating_sub`, so a bad record file is an error instead of a panic.
- The HTML page did not escape the job name in the title. It does now.
- `prune --apply` ignored `tlmgr remove`'s exit status. A non-zero exit is an error that
  names the log, not an empty "nothing left to remove".
- `unused` overwrote a real verdict with "timed out" when the build plus judging took
  longer than the timeout. Gone; the build's own timeout still applies.
- Prune cache names used `DefaultHasher`, which can change between toolchains. FNV now,
  like the run cache.
- `which` finds `pdflatex.exe` on Windows. The stale "next to the document" docs are gone.
- Three copies of "last N lines of the log" and four of "parse the records and build the
  model" are two helpers in `run.rs`. `prune` and `packages` no longer run the whole
  doctor to find the tlpdb.

Left for after 0.1.0, on the board: the public API surface (every module and most items
are `pub`), the long functions (`unused::run`, `run::run`, `doctor::run`, `chart::timeline`),
tuple-shaped signatures, and `texlog` depending on the model.

### Public, and the stranger test (2026-11-09)

The repo went public at `v0.1.0`: one initial commit, CI green, ten release assets, the
`latest` and `small` images for amd64 and arm64. The stranger test was the README's curl
line plus lshort (The Not So Short Introduction to LaTeX, a public document I had never
built) through the published script:

- The script from the raw GitHub URL matches the repo byte for byte and works from `PATH`.
- On `small`, lshort fails at once: fontspec cannot find its fonts and `luabidi.sty` is not
  in scheme-small. prumo says the build exited 1 and where the log is. A new hint names the
  full image when the small one reports a missing file or font.
- On `latest`, 22 pages typeset before fontspec gives up on a font the image does not have.
  That is the document's requirement, not prumo's problem, and the report still says where
  the 4 s went (lshort.sty 1.1 s, basics.tex 0.8 s, biblatex 0.3 s).
- The bug the test found: the failed `small` run was served as "cached run" on `latest`.
  The cache was keyed by document path alone. `run.json` now records the image, a run from
  another image is stale, and a failed build is never reused.

Kevin made the ghcr package public the same night, so the README path works end to end for
a stranger now.

### Windows, for real (2026-11-09)

The lab machine had no TeX, so I put MiKTeX 25.12 on it (per user, unattended, 150 MB from
the CTAN mirrors) and ran the Windows release binary from the zip, then a native build of
the source with Rust, which was already there. Three bugs, none of them visible from Linux:

- `TEXINPUTS` was joined with `:`. kpathsea on Windows separates with `;`, so the engine
  never found `prumo.sty` and every build died at the first line. Now `;` on Windows.
- `normalize` rebuilt paths with `Path::push`, which uses backslashes on Windows, while the
  tree roots from `kpsewhich` keep forward slashes. Nothing classified as texmf-dist (the
  packages report said "0 texmf-dist files"). Windows drive paths are now normalized to the
  portable spelling on every platform. This is the fix for finding 47, which was written
  blind; the three Windows path tests now pass on Windows itself.
- Printed paths carried `\\?\` from `canonicalize`. Stripped.

With those in, on MiKTeX: `doctor` is all green (MiKTeX ships poppler, so fonts and text
comparisons work), `time`, `fonts`, `packages` (file-level, no tlpdb), `--json`, `--html`,
`where` and `clean` all behave, and `cargo test` passes natively. Two things to know about
MiKTeX: the first build on a fresh install took 65 s (its file name database and font
generation; the second took 0.6 s), and its timer has 1 ms resolution, so small units read
0.0 ms. `prumo.cmd` is still untested: that box cannot run Docker Desktop (no admin, no
WSL, and the engine wants a desktop session).

### Toward the announcement (2026-11-09)

The last gaps before I announce it are the ones a stranger hits first:

- The README had no install section. Now it has one: `cargo install prumo`, the release
  binaries, or the image. The "nothing has run on Windows" line is gone, replaced by the one
  real limitation there (no tlpdb on MiKTeX, so no `prune`).
- crates.io is prepared, not done. `publish = false` is gone and `release-binaries.yml` has
  a `crate` job after the binaries that publishes through crates.io trusted publishing (OIDC,
  no stored token). It publishes only when the tag is exactly the crate's version, so an rc
  tag on a release version does nothing. crates.io wants the first version published by hand
  before a trusted publisher can be registered, so 0.1.0 is a manual `cargo publish`, then
  Settings, Trusted Publishing on the crate: owner `kevinraymond`, repository `prumo`,
  workflow `release-binaries.yml`. From the next tag on, the job does it.
- Two hints from the lshort and MiKTeX runs. The Type 3 warning now picks its install
  command from what is on `PATH`: `tlmgr install cm-super` on TeX Live, `miktex packages
  install cm-super` on MiKTeX, plain "install cm-super" when neither is there. When fontspec
  cannot find a font, the build-failed message names the font and says whether it looked on
  this machine or in the image.
- The announcement drafts (r/LaTeX, a TeX.SE answer on the per-page timing question, a short
  post, fuller release notes) are in `~/ai/prumo-announce.md`, outside the repo, for me to
  edit and post.
- Rust users had a one-liner and everyone else had a tarball. Now `install.sh` (Linux, macOS)
  and `install.ps1` (Windows) are the first lines of the README: pick the archive for the
  machine, check the published sha256, put the binary in `~/.local/bin` or
  `%LOCALAPPDATA%\Programs\prumo`, say when that is not on PATH (PowerShell adds it). Tested
  against the v0.1.0 assets here and on the Windows box. A Homebrew tap is the next step for
  Mac users; it needs a second repo and a token, so it waits for me.

CTAN upload of `prumo.sty` alone comes later.

## Hardening 2 — build commands, Windows paths, three more document types (2026-11-07)

Done, each as its own commit:

- **`--build <cmd>`** on every build command and `unused`: prumo wraps the user's own
  command (`latexmk -pdf -shell-escape -outdir=build`, `lualatex -jobname=thesis`, a bare
  `latexmk`) and adds only `-recorder`, `-jobname`, `-interaction=nonstopmode` and the
  probe. Artifacts come from `-output-directory` / `-outdir` / `-auxdir`; a bare latexmk's
  engine is read from the `start` record. The cache is now also stale when `--latexmk` or
  the build command changes, and no longer stale when only the build's own outputs
  (`.aux`, `.out`) are newer. `tests/tex.rs::build_command_with_output_directories`.
- **Windows paths in `.fls`** (`C:/`, `c:\`, CRLF) parse into one spelling and classify
  against kpsewhich roots case-insensitively. Unit tests from the documented formats; not
  verified on a Windows machine (finding 47).
- **Three real-document fixtures** with known answers in their headers: `beamer/deck.tex`
  (5 shipouts for 4 frames, class-level loads, a redundant `\usepackage{amsmath}`),
  `biblatex/paper.tex` (biber through latexmk, `.bbl` through the hooks, csquotes unused)
  and `fontspec/glyphs.tex` (XeTeX only, OpenType fonts the engine opens itself). 25
  artifact sets now; the excerpt tlpdb has 128 stanzas. `make_artifacts.sh` honors
  `% !TEX program =` and `% prumo: build with latexmk`.
- What the fixtures found and what changed (findings 39–46): biblatex's primitive-`\input`
  files escape the hooks (doctor reports them); the probe records `\CurrentFileUsed` so
  kernel substitutions (`atbegshi-ltx`) collapse and cross-check; the fonts report has
  `opentype` (paired with the `.otf` or the luaotfload `.luc` on LuaTeX, unpaired on XeTeX)
  and `unknown_embedded` instead of calling OpenType fonts "preloaded by the format"
  (oracle first, goldens regenerated, Rust matched); the log reader no longer takes
  fontspec's `(m/n)` for a file; `prune` keeps biber/bibtex/makeindex from the `.fls`;
  `unused` reports `\usepackage` lines for packages the class had already loaded as
  "already loaded" without a rebuild; `InputKind::Image` for graphics the engine reads.
- **`unused` now deletes the request instead of blocking the package** (finding 48): the
  blocking marker made biblatex believe csquotes was loaded and call its macros, so the
  fixture's unused csquotes came out "required". The request is removed from the loader's
  source in the temp copy; blocking stays as the fallback when the request is not found.
- `make images` built the wrong Docker stage (finding 44); fixed.

- **`prumo unused <loader.cls> --corpus <docs>`**: the unused pass on every document
  (files, directories, globs; magic comments pick the engine; each document's own cache),
  merged for the packages the loader loads: removable only if no document needs it, else
  "needed by lista.tex (required)". `tests/tex.rs::unused_corpus_judges_a_class_across_documents`
  on obra.cls with a note that uses nothing and a list that uses enumitem.

- **Smoke run on florilegium's own package** (`prumo unused florilegium.sty --corpus
  '*.tex'`) found two more: a corpus pass aborted on the first document whose baseline
  failed (now reported per document and skipped), and the per-candidate timeout was a
  label applied after the fact, so a candidate that loops on errors in nonstopmode ran
  124 s (now the build runs in its own process group and is killed at the timeout, latexmk's
  engine child included; `tests/tex.rs::timeout_kills_the_build`).
- Florilegium corpus result (11 documents, 9 load `florilegium.sty`, LuaLaTeX, `--jobs 4`,
  about 4 minutes): nothing the package loads is removable (tikz needed by 9 documents,
  eso-pic by 6; the specimen and ornament documents alone would have let eso-pic go).
  `florilegium-doc.tex` on its own: `graphicx` has no observable effect; everything else it
  asks for is required or on the skip list.
- README: the "How this differs" section (board #5224); `scripts/verify_prune.sh` now covers
  all six fixtures (16 builds, biblatex through `--latexmk`).

**Not done:** Q5 (older kernels) is Kevin's call and is on the board. Prune gate on the six fixtures
(`texlive/texlive:latest`, 2026-11-07): **5,263 → 184 packages (5,079 removed, 0 refused),
texmf-dist 4.4 G → 241 MB, 16/16 rebuilds identical**; biber kept as a build tool, latexmk
kept, the fontspec document's `lm` by the format's triggers.

**Blocked:** nothing. **Open questions surfaced:** Q5 is still open; no behavior depends
on it. The Windows path handling wants a real `.fls` from a Windows machine.

## Hardening 1 — florilegium as the first real corpus (2026-11-06)

Ran every command on florilegium (LuaLaTeX, TikZ, Lua-drawn margins; 11 documents) from the
user image, on a copy of the repo. Everything ran; five things were wrong or weak, all fixed:

- **`unused` called florilegium itself "no observable effect"** — text comparison cannot see
  drawings. The default mode now adds a coarse ink check (24 dpi gray raster, 0.5 % tolerance)
  on top of text and page count; `--compare render` stays the strict 50 dpi mode. Regression
  test: corpus-b (one TikZ picture, no text) must report `tikz` as affecting output.
- **Shipout split.** florilegium's cost is in the shipout hooks (eso-pic): the probe now
  records `ship` at `shipout/before` (format extension, `04-probe.md`), `prumo time` prints
  `page 1: 384 ms (typesetting 17 ms, shipout 367 ms)` and a hint when hooks dominate, JSON
  gets `shipout_ms`. On florilegium.tex 98 % of the body is shipout; showcase page 3 is 2.2 s.
- **Hints for body-dominated documents**: when the body is more than twice the preamble, the
  hint says so (pages × ms/page) instead of naming a 7 % preamble package.
- **`compare` on an engine that fails** now shows the first `!` error ("florilegium needs
  LuaLaTeX") instead of a log tail ending in "Output written".
- **Project copies**: `run.rs` hard-links files over 1 MiB instead of copying (florilegium's
  tree is 3.8 GB with its build directory); small files are still copied because TeX rewrites
  `.aux`/`.toc`/`.out` in place. The fontspec warning now fires only when the PDF has fonts the
  TFM side cannot account for.

Numbers worth keeping: florilegium.tex 1.6 s / 4 pages, preamble 148 ms, 364 ms per page of
which ~360 ms shipout; florilegium-doc.tex 28 pages 1.5 s with `fontspec-luatex` 96 ms as the
biggest preamble unit; `prune` keep set 58 packages for the package's own documents.

**Next:** more real documents (a beamer deck, biblatex/biber through `--latexmk`, a fontspec
document on XeTeX), `--build <cmd>`, Windows paths, older kernels (Q5), `unused --corpus`.

## M5 — unused (2026-11-06)

**Decision (Kevin):** Q11 — M5 first, then hardening; no release until everything is solid.

**Done**

- `unused.rs`: candidates are `pkg` frames loaded before `\begin{document}` directly by the
  main file or by the user's own class/package (project-tree `.cls`/`.sty`); distribution
  dependencies and `\AtBeginDocument` hook loads (graphicx → color, epstopdf-base) are not.
  Each candidate is blocked with `\@namedef{ver@<pkg>.sty}{blocked}\@namedef{opt@<pkg>.sty}
  {<recorded options>}` injected ahead of the probe (`RunOptions::pretex`), rebuilt on a
  fresh copy in parallel (`std::thread::scope`, `--jobs`, default half the cores), and judged:
  required (non-zero exit or a `! ` error the baseline lacks), affects output (page count,
  `pdftotext -layout`, or with `--compare render` a 50 dpi gray raster with a 0.1 % pixel
  tolerance), affects metadata (`pdfinfo` minus dates/size), no observable effect, not tested
  (skip list, `--test-all` overrides), timed out (`--timeout`, default 10× baseline). The
  baseline is the ordinary probed run (byte-identical to a plain build). Verdicts are cached
  in `.prumo/<job>/unused.json` keyed by a hash of the project sources read, engine, mode.
- Candidate boundary: siunitx loads `color` from `begindocument/before`, which runs before
  the `begindocument` hook the probe recorded as `begindoc`, so "loaded before `begindoc`"
  was not the preamble. The probe now records `predoc` at `begindocument/before` (format
  extension, documented in `04-probe.md`; old records fall back to `begindoc`), and a
  candidate must also be named in a `\usepackage`/`\RequirePackage` line of its loader's
  source. Artifacts regenerated; the invariance test still passes with the extra hook.
- `prumo unused [--jobs N] [--compare text|render] [--test-all] [--timeout S] [--json]`; the
  text report groups candidates by loader and says what differed; class-level candidates get
  the "obra.cls loads N packages this document does not use" line.
- Acceptance (`tests/tex.rs::unused_verdicts_on_the_fixtures`, 3.5 s in the image): basic →
  `xcolor` no observable effect, `amsmath` required ("Environment align undefined"),
  `microtype`/`fontenc`/`hyperref` not tested; with `--test-all`, `hyperref` is **required**
  (the fixture uses `\href`), not "affects metadata" as `07-phases.md` guessed. custom-class →
  `graphicx` and `enumitem` (via obra.cls) no observable effect, `siunitx`/`booktabs`/
  `fancyhdr` (via obra.cls) required, `geometry` not tested with its recorded options. Render
  mode agrees. The cache returns the same verdicts without building.

**Blocked:** nothing. **Open questions surfaced:** none; Q9 (skip list) and Q10 (default
comparison) were implemented as recommended (list + `--test-all`; `text` default).

**Next:** hardening before anything release-shaped (Kevin: "no release until everything is
solid"): real-world documents beyond the fixtures, error paths, Windows paths in `.fls`,
older kernels (Q5), then M6 polish (`--html`, latexmk-aware wrapper, README differentiation
#5224). Release items (binaries, GHCR image #5228, CTAN Q12) wait for that.

## M4 — prune (2026-11-06)

**Done**

- `src/prune/mod.rs` + `keep.rs`: corpus discovery (files, directories, globs; a document is
  a `.tex` with an uncommented `\documentclass`; `% !TEX program =` magic comments pick the
  engine), per-document runs cached under `<out>/corpus/<engine>/<doc>/`, `plan()` as a pure
  function over the database and the documents' package reports, keep set with a reason per
  entry (document file + loader, infra, engine, format + `fmttriggers`, hyphenation, `--keep`,
  OpenType provider, latexmk, dependency-of), `depend` closure, removal list, `prune-plan.txt`,
  `keep.txt`, `prumo.profile` (`install-tl`, scheme-infraonly), text and JSON (schema 1).
- Q6: `--keep-hyphenation all` default (`used` matches `AddHyphen name=/synonyms=` against
  `.ldf` basenames). Q7: on XeTeX/LuaTeX, OpenType providers are matched by file stem
  (`LMRoman10-Regular[-Identity-H]` → `lmroman10-regular.otf` → `lm`); an unmatched name keeps
  every OpenType/TrueType provider with a warning. Q8: `--apply` recomputes and prints the
  plan, requires `--yes`, runs `tlmgr remove --no-depends` in batches of 200, logs every
  command and its output to `prune.log`, and reports removed vs refused.
- `tlmgr` facts (findings 31–33): it refuses a package while anything installed still lists
  it (even later in the same call), including packages that depend on collections
  (`yb-book` → `collection-fontsextra`), so the plan is one dependents-first order over
  schemes, collections and packages, retried until no progress; binary stanzas are never
  listed on their own.
- Gate: `scripts/verify_prune.sh` (also `make verify-prune`, weekly `prune-gate.yml`) on
  `texlive/texlive:latest`: builds the three fixtures on three engines, prunes, rebuilds.
  **5,263 → 165 packages (5,098 removed, 0 refused), texmf-dist 4.4 G → 216 MB, needed
  235 MB by the database, 9/9 rebuilds identical in text and page count.** Four runs to get
  there: the first (before the tlmgr findings) already passed with 658 packages left but
  every batch had refusals; the ordering across categories was the last fix.
- `docker/Dockerfile.user`: the no-install way to run prumo (full TeX Live, Poppler, warm
  font caches, `prumo` entrypoint, `.prumo/` owned by the invoking user). Publishing is an
  M6 item.

**Findings:** 31–34 in `10-verified-findings.md`; the user-image font-cache note is on the
board (#5229).

**Blocked:** nothing. **Open questions surfaced:** Q11 (release scope) is on the board as
#5225 with a recommendation: 0.1 after a short M6 polish, M5 as 0.2.

**Next:** M5 `unused` (differential builds), or M6 polish first if Q11 says so.

## M3.5 — `prumo compare` (2026-11-06)

From the requester's Reddit follow-up ("maybe I could find a compiler that is two seconds
faster"): `prumo compare main.tex [--engines a,b] [--runs N] [--latexmk] [--json]` runs the
document on every engine the machine has (`compare.rs`), two runs each by default because the
first XeTeX/LuaTeX run pays for font caches (findings 20, 26), and prints one table: total,
preamble, body, pages, slowest unit, page 1, first run. Engines not installed or failing are
rows, not errors. Per-engine caches under `.prumo/compare/<engine>/`. JSON schema 1.
`tests/tex.rs::compare_runs_every_engine` covers it; the demo is in `scripts/tex_tests.sh`.

**Next:** M4 prune (`05-packages-and-pruning.md`; Q6–Q8 decided, see the M3 entry).

## M3 — fonts (2026-11-06)

**Done**

- Oracle first (rule 4): `scripts/prumo_report.py` now resolves TFM names through the
  `pdftex.map` the run read (`kpsewhich --format=map pdftex.map` when the `.fls` has none),
  reports virtual fonts, format-preloaded fonts and `no_map_entry`, and keeps the naive
  fields. Goldens regenerated; `make_artifacts.sh` saves the relevant map lines as
  `fontmap.txt` so the Rust side is testable without TeX.
- `fonts.rs`: `parse_map`, `parse_pdffonts` (multi-word types, `CID Type 0C`), `.fd`
  declarations (`\DeclareFontShape` size specs incl. `genb*cmr`, `s*[0.95]`, `ssub`),
  `report()` matching the oracle field for field on all nine TL2026 sets, plus attribution:
  `pcrr8t  [T1/pcr via t1pcr.fd, loaded by main.tex]`. `run.rs` saves `fontmap.txt` and the
  `.fd` files read into the cache. `prumo fonts [--json]` with the Type 3 / `.pk` warning and
  the fix hint, a "loaded but never set a glyph" hint, and the partial-match note.
- Type 3 acceptance: `docker/Dockerfile.tex` has a `nocmsuper` target; `tests/tex.rs::
  fonts_report_courier_and_type3` asserts the clean branch where `sfrm1000.pfb` exists and the
  Type 3 branch where it does not; CI runs both images. `pcrr8t` is loaded-not-embedded on
  all three engines.

**Findings** (also items 27–30 in `10-verified-findings.md`)

- XeTeX's `.fls` on TL2026 has no `pdftex.map` (xdvipdfmx is a separate process); the
  fallback is `kpsewhich`. Finding 14's "on all engines" was TL2023-specific or wrong.
- TFMs the format preloads (`cmr10`, `cmmi10`, `cmr7` on pdfTeX/XeTeX) never appear as
  `.fls` INPUTs; they are embedded all the same and are named through the reverse map.
- LuaTeX reads virtual fonts eagerly: `pcrr8t.vf` and its raw `pcrr8r.tfm` appear even when
  no Courier glyph is set; the raw font is the one reported as not embedded.
- A document with no text at all (corpus-b: one TikZ picture) reads no `.tfm`; scripts that
  grep for them under `pipefail` die. Fixed in `make_artifacts.sh`.

- Bug found by the demo and fixed: the cache was keyed by job name only, so two documents
  called `main.tex` sharing one `--out` read each other's run. `cached()` now requires the
  recorded document path to match (test in `tests/tex.rs`).

**Blocked:** nothing. **Open questions surfaced:** none new (Q7, system fonts on
XeTeX/LuaTeX, is due before M4 and the fonts report already says the loaded side is
incomplete on those engines).

**Next:** the engine comparison (`prumo time --engine all`, board #5214), then M4 prune.

**Decisions for M4 (Kevin, 2026-11-06):** Q6 `--keep-hyphenation` defaults to `all`; Q7 v1 treats
font packages as unknown on XeTeX/LuaTeX and keeps every OpenType/TrueType provider the PDF
names; Q8 `prune --apply` ships in v1 behind `--yes`, Docker gate green first.

## M2 — graph, log cross-check, packages (2026-11-06)

**Done**

- `prumo graph [--format tree|dot|json | --dot | --json] [--depth N]` (`graph.rs`): the load
  tree with options, Graphviz DOT (units boxed, classes double-boxed, fill by exclusive share),
  and JSON with a flat `edges` list. Acceptance edge `obra → siunitx → translations` is tested
  on the checked-in artifacts and the live build.
- `texlog.rs`: a minimal log reader after reading texlogsieve: unwrap at `max_print_line`
  **unless the next line starts a message** (LuaTeX emits unwrapped 79-column lines), `(`+path
  opens / `)` closes, `[n` shipouts, `! ` errors, warnings, the output line. `hook_coverage()`
  cross-checks records vs `.fls` vs log; `prumo doctor --check-log main.tex` prints it. On all
  nine TL2026 logs the only files outside the hooks are the kernel's primitive-`\input` reads
  (`.aux`, `.out`) and the probe itself; zero hooked files are missing from the log.
- `prumo packages [--tlpdb PATH] [--json]` (`packages.rs`): used `texmf-dist` files → tlpdb →
  packages, with the LaTeX unit that loaded each file ("via"), plus infrastructure
  (`texlive.infra kpathsea texlive-scripts hyphen-base`), the engine package, the format's
  provider and its `fmttriggers` (read from the tlpdb, not hard-coded), closed over `depend`
  with `foo.ARCH` resolved to the installed arch stanzas. Without a tlpdb (Debian) it prints
  the file-level list. Basic fixture on pdfTeX: 81 packages, 184 MB, 36 from files read.
- `fixtures/tlpdb/excerpt.tlpdb` now has 110 verbatim stanzas: the full `depend` closure of
  everything the fixtures read, so `packages` is tested without TeX on all nine TL2026 sets
  (nothing unresolved; `siunitx via obra.cls`, `translations via siunitx`, `courier via (font
  load)`, `psnfss via main.tex`).
- Artifacts now carry the full `.log` instead of a 40-line tail; `fls::InputKind::Data` for
  the Unicode tables and font caches LuaTeX reads through Lua.

**Findings:** LuaTeX's `latex-bin` closure pulls `luatex`/`luahbtex`/`luaotfload` into a
pdfTeX document's package list; that is what `tlmgr` would install too, and the report says
"← latex-bin". `keyval.sty` belongs to the `graphics` package, so on some engines the first
loader of `graphics` is `geometry`, not `graphicx`.

**Blocked:** nothing. **Open questions surfaced:** none new; Q5 still open (no behavior
depends on it yet).

**Next:** M3 — `fonts.rs` (loaded vs embedded, Type 3/`.pk` warnings, attribution through the
`.fd` frame), `prumo fonts`; the map-file resolution from finding 14.

## M1 — the probe and `prumo time` (2026-11-06)

**Done**

- `model.rs`: record stream → frame tree with the five rules (push/pop, collapse of a unit's
  own file frame, close-at-end, `opts` attachment, pages). `unit_table()` and `page_times()`
  match `scripts/prumo_report.py --json` **exactly** (same rows, same order, same scaled
  integers) on all 18 checked-in artifact sets (`tests/fixtures.rs::model_matches_the_oracle`).
- `run.rs`: copies the project to a temp dir (skipping `.prumo`, VCS, `target`, symlinks),
  writes the embedded probe, prepends it to `TEXINPUTS`, runs `<engine> -interaction=nonstopmode
  -recorder -jobname=<job> "\RequirePackage{prumo}\input{<job>}"` or the `latexmk -usepretex`
  equivalent, and copies `.prumo`/`.fls`/`.log`/`.pdf`/`build.out`/`pdffonts.txt`/
  `kpsewhich.txt`/`run.json` into `.prumo/<job>/`. Cache is stale when a project file the
  build read is newer than the build's start, or the engine changed.
- `report.rs` + `prumo time [--by unit|package|file|page] [--top N] [--depth N] [--json]`.
  JSON is schema 1: `total_ms`, `preamble_ms`, `units`, `pages`, `tree`, `warnings`.
- `tests/tex.rs` (behind `--features tex`): every fixture × engine runs, leaves the project
  untouched, reproduces the checked-in unit set, and exclusive times sum to the total;
  **probe invariance is byte-identical on 9/9** (same directory, `SOURCE_DATE_EPOCH=0`);
  the latexmk route records. `scripts/tex_tests.sh` builds the test binaries on the host and
  runs them in the `docker/Dockerfile.tex` image; CI's `tex` job calls it.
- Demo: `prumo time fixtures/custom-class/main.tex` → `siunitx 32.5 ms excl / 48.2 ms incl`,
  "siunitx is 58% of your preamble; it is loaded by obra.cls".

**Findings:** item 26 in `10-verified-findings.md` (XeTeX's first-run font cache lands in
`size10.clo`; the hint now leads with the heaviest row even when it is a file).

**Blocked:** nothing. **Open questions surfaced:** none new.

**Next:** M2 — `prumo graph --tree|--dot|--json`, `texlog.rs` cross-check
(`doctor --check-log`), `prumo packages` on a real tlpdb with "via" attribution.

## M0 — skeleton and parsers (2026-11-06)

**Decisions (Kevin):** name stays `prumo` (free on crates.io and CTAN on 2026-11-06; the ~99
GitHub repos by that name are unrelated). Rust code MIT OR Apache-2.0; `tex/prumo.sty`
additionally LPPL-1.3c. Repo home `kevinraymond/prumo`; **no public remote until it is ready**,
private if one is created. CI TeX image is a custom one: `docker/Dockerfile.tex` =
`texlive/texlive:latest-small` + `cm-super courier fontname siunitx translations` + poppler.

**Done**

- Repo layout: handoff pack moved to `docs/`, `CLAUDE.md` at root, `cargo init` (2024 edition,
  `unsafe` forbidden), licenses, `.gitignore`, `rustfmt.toml` (120 cols). Not committed yet.
- `records.rs`, `fls.rs`, `tlpdb.rs`, `doctor.rs`; `prumo doctor [--json]`.
- Fixture artifacts regenerated on TeX Live 2026 (`fixtures/*/artifacts/`); the handoff's
  Debian TeX Live 2023 set kept as `artifacts-tl2023-debian/` so both path layouts are tested.
  Each set now carries `kpsewhich.txt` (the texmf roots that classify `.fls` paths).
- `fixtures/tlpdb/excerpt.tlpdb` is now 51 real stanzas from the TL2026 installed database
  (collection, scheme, `.x86_64-linux` binary stanzas, font packages with `addMap`/`addMixedMap`,
  hyphenation, `latex-bin` with `AddFormat`, `00texlive.*`), doc/src lists cut to 3 entries.
- `tests/fixtures.rs`: all 18 artifact sets parse, validate, classify, and agree with the
  Python oracle on page list, total time, TFM list and `.pk` count. 19 tests, no TeX needed.
- `.github/workflows/ci.yml`: fmt/clippy/test on Linux + macOS; a `tex` job that builds the
  small image and regenerates artifacts, failing if the record *shape* changes. Unrun (no remote).
- Verified: the small image reproduces the full image's records and `pdffonts` output exactly
  (9/9 fixture × engine); `doctor` says "not ready" on this Debian host (binaries, no formats, no
  tlpdb → "file-level only") and "ready" in the image.

**Findings that correct the handoff** (details in `10-verified-findings.md`, items 19–25)

- Probe invariance holds on pdfTeX and XeTeX byte for byte. On LuaTeX the trailer `/ID`
  hashes the output directory, so two plain runs in different directories already differ;
  with the same directory (or `/ID` masked) the probe changes nothing. The M1 invariance test
  builds both runs in the same directory.
- A cold luaotfload cache makes LuaTeX's first run record 6,886 INPUT lines (every system
  font). `make_artifacts.sh` now warms the cache first.
- `texlive/texlive:latest` is 2.78 GB compressed, not ~5 GB, and ships no docs, so installed
  tlpdb stanzas have no `docfiles`.

**Blocked:** nothing.

**Open questions surfaced:** none new. Q5 (minimum kernel) is due before M2; the `.sty`
already requires 2020-10-01 and `doctor` warns below 2021-06-01.

**Next:** M1 — `run.rs` (temp dir, `TEXINPUTS`, `-recorder -jobname`, cache), `model.rs`
(tree with the five rules), `prumo time` text + JSON matched to `golden.json`, invariance
test behind `--features tex`.
