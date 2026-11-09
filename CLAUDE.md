# prumo — project charter for Claude Code

prumo is a Rust CLI that analyzes LaTeX builds: timing per package/file/page, who-loaded-whom,
fonts loaded vs embedded, TeX Live packages actually needed, and (phase 2) `\usepackage` lines
that can be removed. Design docs live in `docs/`. Read `docs/03-architecture.md` before touching
`src/`, and `docs/04-probe.md` before touching `tex/prumo.sty`.

## Rules that do not bend

1. **Never modify the user's source files.** All builds happen on a copy in a temp dir. prumo
   writes nothing in the document's directory: artifacts go to the user's cache directory
   (`prumo where`, `PRUMO_CACHE`) unless `--out` or `--html FILE` names a place.
2. **Never run a destructive `tlmgr` command.** `prumo prune` prints a plan. It executes the plan
   only with `--apply`, and `--apply` first re-runs the plan, shows it, and requires `--yes`.
3. **Never change typesetting.** `prumo.sty` only listens to hooks and writes a record file. If a
   feature needs a macro redefinition, it is the wrong feature. The acceptance test is a
   byte-identical PDF (modulo `/CreationDate` and ID) with and without the probe.
4. **The Python reference is the oracle.** `scripts/prumo_report.py` output on `fixtures/` is
   frozen as golden files in `tests/golden/`. The Rust `report` must match it (field for field,
   timings within tolerance). When the design changes, change the Python first, regenerate
   goldens, then port.
5. **Hooks over patches.** Everything on the TeX side goes through the LaTeX hook management
   system (`\AddToHook`). No `\let`-and-redefine of kernel or package internals.
6. **Engine-agnostic.** pdfTeX, XeTeX, LuaTeX all pass the fixture suite in CI. If a feature is
   engine-specific (e.g. PFB-based embed detection), it is gated and reported as such.
7. **No network in the tool.** prumo reads a local `texlive.tlpdb`. Fetching a remote tlpdb is
   out of scope. Tests never touch the network.

## Conventions

- Rust 2024 edition, stable toolchain, `cargo clippy -- -D warnings`, `cargo fmt`. No `unsafe`.
- One crate, `src/main.rs` (clap) + `src/lib.rs` with modules: `records`, `fls`, `log`, `tlpdb`, `doctor`,
  `fonts`, `model`, `report`, `run`, `prune`, `unused`. Split into a workspace only if `lib`
  gets a second consumer.
- Errors: `anyhow` at the CLI edge, `thiserror` in the library. Every parser error names the
  file and line.
- Output: human text by default, `--json` always available and stable (it is the API). `--dot`
  for graphs. HTML only in M6.
- The probe `tex/prumo.sty` is embedded with `include_str!` and written to a temp dir that is
  prepended to `TEXINPUTS` at run time. Users never install it. Also keep it installable
  (`TEXMFHOME`) for people who want to run the engine themselves.
- Timings are `u64` scaled seconds internally (1/65536 s). Convert to ms only at the edge.
- Portuguese names for things Kevin invents (the tool, fixture classes); plain English for
  everything else. Do not call any reference document a "bible".

## Testing

- `cargo test` runs parser tests with checked-in fixture artifacts (`.prumo`, `.fls`, `.log`
  tail, `pdffonts` output, `kpsewhich.txt`, golden JSON) under `fixtures/<name>/artifacts/<engine>/`
  (TeX Live 2026) and `artifacts-tl2023-debian/<engine>/` (Debian layout), so tests pass without
  a TeX installation. `scripts/make_artifacts.sh` regenerates the former; run it in the image
  from `docker/Dockerfile.tex`.
- `cargo test --features tex` additionally runs the fixtures through a real engine. CI uses the
  image from `docker/Dockerfile.tex` (scheme-small + the fixture packages) for this job.
  `scripts/tex_tests.sh` runs those tests inside the image when the host has no TeX.
- `scripts/verify_prune.sh` (`make verify-prune`) is the prune gate on the full image: slow,
  weekly in CI, and the only place `prune --apply` ever runs.
- A fixture is a document with a *known answer* written in its header comment. Every claim in a
  header comment has a test.

## What to ask Kevin about

Anything listed in `docs/08-open-questions.md`. Do not pick defaults for those. For everything
else, pick the simplest thing that passes the fixtures and note the choice in the PR.

## Don't reinvent

- Per-macro profiling → point at `texprof` (TeX Live, pdfTeX only). prumo stays at package/file/page.
- Log parsing for file nesting → read how `texlogsieve` does it before writing a log parser; prumo
  prefers hook records and uses the log only as a cross-check.
- Build orchestration → `latexmk`. prumo wraps it (`-usepretex`, `-jobname`), never replaces it.
- Package installation → `tlmgr`. prumo emits commands for it.

## Reporting format for a session

End each session with a short note in `docs/STATUS.md`: milestone, what passed, what is blocked,
which open questions surfaced. Keep it to the current milestone; roll older entries into one
line each.
