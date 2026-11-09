# 07 — Phases

Each milestone ends with a demo you can run in one command. Acceptance criteria are tests, not
feelings. Order is chosen so the first useful thing (`prumo time`) exists after M1 and the
headline feature (prune) after M4; unused detection is last because it is the only expensive one.

## M0 — Skeleton and parsers (no TeX needed to pass tests)

- Repo: `cargo new prumo`, Rust 2024, clap, CI (fmt, clippy, test) on Linux + macOS.
- `records.rs`: parse `.prumo` → `Vec<Record>`. Fixture artifacts checked in under
  `fixtures/<name>/artifacts/<engine>/` (`.prumo`, `.fls`, `.log`, `pdffonts.txt`).
- `fls.rs`: parse `.fls`, canonicalize, classify by tree (inject the `kpsewhich` vars in tests).
- `tlpdb.rs`: parse a real excerpt (replace the hand-written `fixtures/tlpdb/excerpt.tlpdb`
  with ~30 stanzas copied from an actual TeX Live, including a collection, a scheme, a binary
  stanza, a font package with `execute addMap`, and a hyphenation package).
- `prumo doctor`: finds engines, kernel date (`\fmtversion`) ≥ 2021-06-01, `-recorder`
  support, Poppler tools, tlpdb path; prints a table; exit code reflects readiness.
- Name check: crates.io, CTAN, GitHub for the final name (Q1).

**Accept:** `cargo test` green without TeX; `prumo doctor` correct on a Debian box (no tlpdb →
"packages: file-level only") and on a vanilla TeX Live.

## M1 — The probe and `prumo time`

- `run.rs`: temp dir, `TEXINPUTS`, `-recorder -jobname`, engine or latexmk invocation,
  `.prumo/<job>/` cache with staleness check.
- `model.rs`: tree building with the five rules in `03-architecture.md`.
- `prumo time` text + JSON; golden files generated from `scripts/prumo_report.py --json`.
- Probe invariance test: build each fixture with and without the probe under
  `SOURCE_DATE_EPOCH=0`; PDFs byte-identical.

**Accept:** all three fixtures × three engines produce a tree whose unit exclusive times sum to
the job total within 1%; golden JSON matches the Python oracle (timings within 2 ms); invariance
test passes. **Demo:** `prumo time fixtures/custom-class/main.tex` shows siunitx at the top.

## M2 — Graph and packages

- `prumo graph --tree|--dot|--json`.
- `texlog.rs` cross-check (`doctor --check-log`): files in `.fls`/log but not in records are
  listed as "outside hook coverage".
- `prumo packages` on a real TeX Live (needs tlpdb): used files → packages → `depend` closure
  → list with "via" attribution.

**Accept:** DOT for custom-class fixture has the edge `obra → siunitx → translations`;
`packages` on the basic fixture includes `hyperref`, `amsmath`, `microtype`, `xcolor`,
`psnfss` (for `t1pcr.fd`), `courier`? — write the expected list from a real run and freeze it.

## M3 — Fonts

- `fonts.rs`: loaded (`.tfm` from `.fls`), embedded (pdfTeX: `.pfb`/`.pk` from `.fls`; others:
  `pdffonts`), Type 3 and `.pk` warnings, attribution via the `.fd` file's position in the tree.
- `prumo fonts`.

**Accept:** basic fixture reports `pcrr8t` loaded-not-embedded on all engines; reports Type 3
when `cm-super` is absent and clean when it is present (two CI images, or `tlmgr remove
cm-super` in the TeX job). **Demo:** the Type 3 warning with the fix hint.

## M4 — Prune

- `prune.rs`: corpus discovery, union, keep set (`keep.rs` with a reason per entry), ordered
  plan, `keep.txt`, `install-tl` profile, `--apply --yes`.
- Docker verification gate from `05-packages-and-pruning.md` as a weekly CI job and a `make
  verify-prune` target.

**Accept:** on `texlive/texlive:latest`, prune to the three fixtures, rebuild, identical
`pdftotext` output and page counts; the plan removes > 90% of packages by count. Then the same
with florilegium as the corpus (real-world gate). **Demo:** the before/after size line.

## M5 — Unused

- `unused.rs`: candidates from the tree, skip list, blocking preamble with recorded options,
  parallel builds, verdict table, `--compare text|render`, cache.

**Accept:** basic fixture → `xcolor: no observable effect`, `amsmath: required`,
`microtype: not tested (side-effect list)`, `hyperref: affects metadata`; custom-class fixture →
`graphicx: no observable effect`, `enumitem (via obra.cls): no observable effect`,
`siunitx (via obra.cls): required`. Runtime for the basic fixture under 10 s on a laptop.

## M6 — Polish and release

- `--html`: a self-contained flame chart page (time) with the tree and fonts tabs. This is the
  screenshot.
- `prumo` as a latexmk-aware wrapper that respects the user's `latexmkrc`.
- README with the three screenshots, a "what it will not do" section, and the texprof pointer.
- Release: the Docker image first (`.github/workflows/release.yml` publishes
  `ghcr.io/kevinraymond/prumo` and `:small` for amd64 and arm64 on a `v*` tag; `docker/prumo`
  and `docker/prumo.cmd` run it as a command), then crates.io, GitHub release binaries
  (Linux x86_64/aarch64, macOS, Windows), CTAN upload of `prumo.sty` alone (so people can
  use the probe without the CLI).
- Blog post + TeX.SE answer on "how do I find which package makes my compile slow".

**Accept:** a stranger installs it from the README and gets a report on their own document in
under five minutes.

## Explicitly later

- `--corpus` mode for `unused` on a shared class (`06`, last section).
- Tectonic bundle spec output from `prune`.
- MiKTeX.
- Memory usage from the log trailer.
- `--pairs` for unused.
