# 03 — Architecture

## One pass, three artifacts, one model

```
                 ┌────────────────────────────────────────────────┐
  main.tex ────▶ │ engine -recorder -jobname=main                 │
  prumo.sty ───▶ │   "\RequirePackage{prumo}\input{main}"         │
                 └───────┬──────────────┬──────────────┬──────────┘
                         ▼              ▼              ▼
                    main.prumo       main.fls       main.pdf (+ main.log)
                    hook events      files read     pdffonts / pdfinfo
                         │              │              │
                         └──────────────┼──────────────┘
                                        ▼
                                   BuildModel
                                        │
          ┌──────────┬──────────┬───────┴────┬───────────┬──────────┐
          ▼          ▼          ▼            ▼           ▼          ▼
        time       graph      fonts      packages     prune      unused
      (frames)   (edges)   (tfm/pfb/pdf) (tlpdb map) (corpus)   (phase 2:
                                                                  N rebuilds)
```

`prumo run` produces the model and caches it under `<out>/<job>/`, where `<out>` defaults to a
per-document directory in the user's cache (`run::default_out`; `prumo where`). Every report command either
uses the cache or runs the build. `prune` merges models across a corpus. `unused` is the only
command that runs more than one build.

## BuildModel

```rust
struct BuildModel {
    engine: Engine,                 // pdftex | xetex | luatex, from the `start` record
    format_date: String,            // \fmtversion
    job: String,
    total: Scaled,                  // last timestamp - first timestamp
    phases: Phases,                 // start, begindoc, enddoc, end
    root: Frame,                    // tree of Frame
    pages: Vec<(u32, Scaled)>,      // shipout number, timestamp
    inputs: Vec<InputFile>,         // from .fls, deduplicated, canonicalized
    fonts: FontReport,              // derived
    warnings: Vec<Diag>,            // probe warnings, parser warnings
}

struct Frame {
    kind: FrameKind,                // Job | Class | Package | File
    name: String,                   // "siunitx" / "t1pcr.fd" / "main.tex"
    options: Option<String>,        // raw option list for packages
    start: Scaled, end: Scaled,
    children: Vec<Frame>,
}

impl Frame {
    fn inclusive(&self) -> Scaled;  // end - start
    fn exclusive(&self) -> Scaled;  // inclusive - sum(children.inclusive)
}

struct InputFile {
    path: PathBuf,                  // canonical absolute
    kind: InputKind,                // Tex | Sty | Cls | Fd | Def | Cfg | Tfm | Vf | Pfb | Otf | Ttf | Pk | Map | Enc | Fmt | Cnf | Aux | Other
    tree: Tree,                     // TexmfDist | TexmfVar | TexmfLocal | TexmfHome | Project | System
    package: Option<String>,        // tlpdb package name, if resolved
}
```

Scaled = `u64` in 1/65536 s. Keep it integral until display.

### Building the tree from records

Records are a flat event stream (`04-probe.md`). Rules, all verified against fixtures:

1. `file+`/`pkg+`/`cls+` push a frame; the matching `-` pops it. Mismatch is a parser error that
   names the line.
2. A `pkg`/`cls` frame always wraps a `file` frame for its own `.sty`/`.cls`. **Collapse** that
   file frame into the unit: splice its children up one level. Otherwise the unit's exclusive
   time is ~0 and the file gets it, which is useless.
3. `\end{document}` never returns from `\input{main}`, so `main.tex` has no `file-` event. Any
   frame still open at the `end` record closes at that timestamp.
4. `opts` directly after `pkg+` attaches to that frame.
5. `page N` events are leaves of the job, not of any frame. Page time = time since the previous
   page (or since `docstart` for page 1; `begindoc` in records from before probe v0.2).
6. `env+`/`env-` are not frames: they go on their own stack, paired by name (`04-probe.md`).
7. Phases (`BuildModel::run_phases`): startup = the first record's timer; preamble, begin-
   document, typesetting and end-document are the spans between `start`, `predoc`,
   `docstart`, `enddoc` and the last record, each less the shipouts inside it; shipout is
   every `ship`..`page`. The six add up to the last record. Finalization is the process wall
   time (`run.json` `wall_ns`, timed by `run::build_once`) minus the last record; it is not
   measured under latexmk, whose process is several passes.

### Cross-check with the log

The `.log` also encodes nesting (parentheses), with the usual line-wrapping mess. prumo does not
need it, but `prumo doctor --check-log` compares the log's file order with the records to catch
hook misses (e.g. files loaded with raw `\input` primitives bypass the hooks? — no: `\input` the
LaTeX command goes through hooks; the TeX primitive `\input` does not). Report files seen in the
log or `.fls` but not in records as "loaded outside LaTeX file machinery" rather than guessing
their position in the tree.

## CLI surface

```
prumo run      <main.tex> [--engine pdflatex|xelatex|lualatex] [--latexmk] [--build <cmd>] [--out DIR] [--env NAMES]
prumo time     <main.tex|job.prumo> [--by package|file|page] [--top N] [--json] [--html [FILE]] [--plain] [--preamble] [--env NAMES]
prumo graph    <main.tex|job.prumo> [--dot|--json|--tree] [--depth N]
prumo compare  <main.tex> [--engines LIST] [--runs N] [--latexmk] [--env NAMES] [--json]
prumo fonts    <main.tex> [--json]
prumo packages <main.tex> [--tlpdb PATH] [--json]           # tlmgr package set, with the file that caused each
prumo prune    <paths...> [--tlpdb PATH] [--keep LIST] [--keep-hyphenation all|used] [--apply --yes]
prumo unused   <main.tex> [--build <cmd>] [--jobs N] [--compare text|render]   # phase 2
prumo doctor   [--check-log]                                 # engines, kernel date, pdffonts, tlpdb, recorder
prumo where    [<main.tex>] [--out DIR]                      # the cache root, or a document's artifact directory
prumo clean    [<main.tex>] [--stale] [--yes] [--json]       # list the cache (sizes, documents); delete with --yes
```

- `run` is implicit: report commands run the build if the cache is missing or stale (source newer
  than the cached `run.json`, another engine, build command or environment list).
- `time` and `graph` also take a record file from a build prumo did not run (`output.prumo`
  from Overleaf, `docs/overleaf.md`): `run::from_records` reads it and the `.fls`, `.log` and
  PDF next to it when present, builds nothing and writes nothing.
- `--engine` defaults to the document's `% !TEX program =` (or `TS-program`) magic comment in
  its first 20 lines, else pdflatex; the build line says which applied. `prune` and
  `unused --corpus` read the same comment per document.
- `--build` lets the user pass their real build command (`latexmk -pdf -shell-escape`). prumo
  adds `-recorder -jobname=<job> -usepretex='\RequirePackage{prumo}'` (latexmk) or the equivalent
  engine flags. If the user's command already sets `-jobname`, respect it.
- Every command has `--json`. The JSON schema is versioned (`"schema": 1`) and documented in
  `docs/json-schema.md` from M1 on.

## Injection mechanics (verified)

```
TEXINPUTS=<tmpdir-with-prumo.sty>:  \
  pdflatex -interaction=nonstopmode -recorder -jobname=main \
           "\RequirePackage{prumo}\input{main}"
```

- `-jobname` is **mandatory**: loading a package before any `\input` makes TeX pick the job name
  `texput`, and every output file follows. prumo.sty warns if it sees `texput`.
- Works identically via latexmk: `latexmk -pdf -recorder -jobname=main -usepretex='\RequirePackage{prumo}' main.tex`.
  The record file is rewritten on every latexmk pass; the last pass wins, which is what you want.
- `TEXINPUTS` with a trailing `:` keeps the default search path.
- The probe must be loaded **before** `\documentclass` so `class/before` and the class's own
  `\RequirePackage`s are seen.

## Crate layout

```
prumo/
  Cargo.toml
  src/
    main.rs        clap: subcommands above
    lib.rs
    records.rs     .prumo parser → Vec<Record>
    model.rs       Record stream → BuildModel (tree, collapse, pages)
    fls.rs         .fls parser → Vec<InputFile>, canonicalization, tree classification
    texlog.rs      minimal log reader: errors, warnings, file nesting cross-check
    fonts.rs       FontReport from InputFile + pdffonts
    tlpdb.rs       tlpdb parser, file→package index, depend closure
    run.rs         engine/latexmk invocation, temp dirs, TEXINPUTS, cache
    chart.rs       terminal charts for time: Unicode bars, the timeline, the page strip (chart::detect gates them)
    cache.rs       what the cache root holds (per document, prune, strangers) and removal
    report/        text, json, dot renderers
    prune.rs       corpus merge, keep set, plan
    unused.rs      phase 2
  tex/prumo.sty    embedded via include_str!
  scripts/prumo_report.py   reference, frozen
  fixtures/        documents + checked-in build artifacts for no-TeX tests
  tests/golden/    expected JSON per fixture per engine
```

Dependencies to expect: `clap`, `serde`/`serde_json`, `anyhow`, `thiserror`, `tempfile`,
`walkdir`/`globset` (prune corpus), `rayon` (unused, parallel builds), `similar` (text diff).
No PDF library: shell out to `pdffonts`/`pdftotext`/`pdfinfo` (Poppler) and say so in `doctor`.
Consider `lopdf` only if Poppler absence becomes a support burden.

## Output examples (from the reference analyzer, custom-class fixture, pdfTeX)

```
== prumo report: main  (total 195 ms, preamble 158 ms, 1 pages, 101 files read)

-- slowest units (exclusive) --
     54.2 ms     82.1 ms incl  pkg  siunitx
     38.1 ms    194.7 ms incl  file main.tex
     14.4 ms     23.1 ms incl  pkg  translations
     12.9 ms     17.5 ms incl  pkg  geometry
      6.4 ms     14.3 ms incl  pkg  graphics
      5.9 ms    118.0 ms incl  cls  obra

-- load tree --
  main.tex  (194.7 ms incl, 38.1 ms excl)
    obra  (118.0 ms incl, 5.9 ms excl)
      article  (4.7 ms incl, 2.9 ms excl)
      geometry  (17.5 ms incl, 12.9 ms excl)
      siunitx  (82.1 ms incl, 54.2 ms excl)
        translations  (23.1 ms incl, 14.4 ms excl)
      ...
    graphicx  (18.7 ms incl, 4.4 ms excl)

-- fonts --
  TFMs loaded: 12, embedded in PDF: 4, Type 3: 0
  loaded but not embedded: cmmi6, cmmi8, cmr12, cmr6, cmr8, cmsy10, cmsy6, cmsy8
```

The Rust text renderer should look like this, plus a one-line hint under each section ("siunitx
is 42% of your preamble; it is loaded by obra.cls").
