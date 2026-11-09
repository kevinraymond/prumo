# prumo

*prumo* (Portuguese: plumb line) is a build analyzer for LaTeX. One instrumented compile tells
you where the time goes, package by package. It also tells you which package loaded which,
which fonts were loaded but never used, and exactly which TeX Live packages your documents
need. A second, slower pass finds the `\usepackage` lines you can delete.

It is not a compiler and not an editor. It sits next to whatever you already use (`pdflatex`,
`latexmk`, TeXstudio) and reads what the build leaves behind.

## Origin

I made this while working on [florilegium](https://github.com/kevinraymond/florilegium) and as
I learn more about LaTeX, LuaTeX and the hook system. The manual kept taking longer to build and
I had no way to see why. Commenting packages out one at a time is not insight. prumo
is one instrumented build that shows where the time goes, which package loaded which, and what you can
drop.

It has been run on one real corpus so far (mine). Please report what breaks!

This project was made with the assistance of AI (Claude).

## Install

prumo is one binary. It needs a TeX installation to measure (TeX Live or MiKTeX) and
`pdffonts` from poppler-utils for the fonts report. `prumo doctor` tells you what it found.

Linux and macOS:

```
curl -fsSL https://github.com/kevinraymond/prumo/raw/main/install.sh | sh
```

Windows, in PowerShell:

```
irm https://github.com/kevinraymond/prumo/raw/main/install.ps1 | iex
```

Both download the binary for your machine from the latest release, check its sha256, and put
it in `~/.local/bin` or `%LOCALAPPDATA%\Programs\prumo`. They say so if that directory is not
on your `PATH` (the PowerShell one adds it). `PRUMO_VERSION` picks a release and
`PRUMO_INSTALL_DIR` a directory. The other ways in: `cargo install prumo` (Rust 1.88 or
newer), the archives on the [release page](https://github.com/kevinraymond/prumo/releases)
by hand, or the Docker image below when there is no TeX at all.

## Usage

```
prumo doctor                         # what this machine can do
prumo time main.tex                  # per-package, per-file, per-page time; builds and caches. In a terminal: timeline, bars, page strip (--preamble expands the preamble, --plain for text)
prumo time main.tex --json           # schema 1, the stable API
prumo time main.tex --html           # one self-contained page: flame chart, tree, fonts (./main-prumo.html)
prumo time main.tex --engine lualatex --latexmk --by package --top 20   # --engine defaults to the % !TEX program line, else pdflatex
prumo time main.tex --build 'latexmk -pdf -shell-escape -outdir=build'   # your own build command
prumo graph main.tex --dot | dot -Tsvg > graph.svg   # which package loaded which
prumo compare main.tex               # the same document on pdflatex, xelatex and lualatex, one table
prumo fonts main.tex                 # loaded vs embedded, Type 3 warnings, which package loaded each family
prumo packages main.tex              # the tlmgr packages this document needs, and why
prumo prune docs/ --engines pdflatex # corpus-wide keep list and tlmgr remove plan (dry run)
prumo prune docs/ --apply --yes      # run the plan; every tlmgr command is logged
prumo unused main.tex                # which \usepackage lines can go: remove the line, rebuild, compare
prumo unused obra.cls --corpus 'docs/**/*.tex'   # a class judged against every document that uses it
prumo doctor --check-log main.tex    # files that bypassed the hooks; log vs records
prumo where main.tex                 # where this document's artifacts are
prumo clean [--stale] [--yes]        # what the cache holds; delete it (all, or only gone documents)
```

Here is `prumo time main.tex --html` on florilegium's manual (LuaLaTeX, 37 pages). The top
chart is the whole build with the preamble folded into one row. Click that row and the
preamble opens on its own axis, one row per nesting depth.

![prumo time --html: the build over time with the preamble folded into one row, then the preamble expanded on its own axis](docs/img/time-html.png)

Everything the instrumented build leaves behind goes to your cache directory: `~/.cache/prumo`
on Linux, `~/Library/Caches/prumo` on macOS, `%LOCALAPPDATA%\prumo` on Windows. `PRUMO_CACHE`
moves it and `--out` picks a place for one run. Every command prints the directory it used,
and `prumo where main.tex` will tell you again later. Nothing is written next to your document.

## Running from Docker, nothing installed

No TeX Live? No Rust? `docker/Dockerfile.user` builds an image with a full TeX Live, Poppler,
warm font caches and prumo as the entrypoint. The `docker/prumo` script runs it as a command.
Copy it somewhere on your `PATH` and you are done:

```
curl -fsSL https://github.com/kevinraymond/prumo/raw/main/docker/prumo -o ~/.local/bin/prumo
chmod +x ~/.local/bin/prumo
cd ~/thesis               # the directory your document is in
prumo time main.tex
```

Nothing goes into your shell's startup files. The script is twenty lines of `sh` that type the
`docker run` line for you. Here is what that line does:

- Mounts the current directory and your prumo cache directory inside the container at the same
  paths they have on your machine. Printed paths are yours, runs survive the container, and
  `prumo clean` sees them.
- Runs the container as you, so the files it writes are yours too.
- Gives Docker a terminal only when stdout is one. `time` draws its charts at the prompt, and
  `prumo time --json | jq` stays clean.

The container sees only the current directory, so run it from the one your document is in.
`PRUMO_IMAGE` picks the image (a local build, the `small` tag), `PRUMO_CACHE` the cache
directory, `PRUMO_DOCKER=podman` another engine, and `PRUMO_DOCKER_OPTS` adds options to
`docker run`.

On Windows, `docker/prumo.cmd` is the same command for Docker Desktop. Save it in a directory
on your `PATH`. It mounts `C:\Users\you\thesis` at `/C/Users/you/thesis` and prumo prints the
Windows names back (the cache is in `%LOCALAPPDATA%\prumo`). I have not tried the `.cmd`
with a real Docker Desktop yet. The native binary is tested on Windows.

Without the script, the one-off form works too. The image then keeps its cache in `.prumo/`
under the mounted directory, because its own cache directory vanishes with the container:

```
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/doc" ghcr.io/kevinraymond/prumo time main.tex
```

Two flavors of image. `latest` and `X.Y.Z` carry a full TeX Live, about 5 GB on disk, and
should compile anything. `small` and `X.Y.Z-small` carry scheme-small, under 1 GB, and
compile most everyday documents. If yours fails on `small`, move up. Both are built for x86-64
and arm64, so Apple silicon runs them natively. Want to build it yourself?
`docker build -t prumo -f docker/Dockerfile.user .` compiles prumo inside the build, nothing
to install first, and `PRUMO_IMAGE=prumo` points the script at it.

What changes inside the image: nothing for `time`, `graph`, `fonts` and `compare`, since they
are about your document. `packages` gives the same package names any TeX Live uses. `prune`
describes the image's installation, not yours, so `--apply` makes no sense there. Its
`keep.txt` and `install-tl` profile are still the useful part: the minimal TeX Live your
documents need, ready to install from scratch. `doctor` and every build say they run in the
image, and `doctor` says what the image can see of your machine. The command hides
`docker run`, not the fact that the TeX Live being measured is the image's.

## How it works

A ~90-line probe package, `tex/prumo.sty`, is loaded ahead of `\documentclass`:

```
TEXINPUTS=tex: pdflatex -recorder -jobname=main "\RequirePackage{prumo}\input{main}"
```

It only listens to the LaTeX hook management system and writes one event per line to
`main.prumo`. It never changes typesetting: the PDF is byte-identical with and without it. prumo
reads that record, the engine's `-recorder` file list (`main.fls`) and the PDF into one model
and reports from there.

The reference analyzer, `scripts/prumo_report.py`, is the oracle the Rust implementation must
match; its output on the fixtures is frozen under `fixtures/*/artifacts/`.

## How this differs

- **It runs on the TeX Live you already have.** The probe is plain LaTeX hooks, so pdfTeX,
  XeTeX and LuaTeX all report the same way, and the numbers are from your real build, with
  your fonts, your class, your `latexmkrc`. An engine with profiling built in can only measure
  itself. prumo can compare any engine that runs LaTeX hooks and `-recorder` to the others on
  your document.
- **It wraps, it does not replace.** `latexmk` still drives the build (`--latexmk`, or your own
  command with `--build`), `tlmgr` still installs and removes packages (prumo prints the
  commands, and runs them only behind `--apply --yes`), your editor still compiles.
- **Package and page, not macro.** For per-macro profiling use `texprof` (pdfTeX only). prumo
  answers the questions before that one: which package, which file, which page, which font,
  which `\usepackage` line.
- **Experiments, not guesses.** "Unused" means the document was rebuilt without the request and
  nothing observable changed: page count, text, a coarse render. The caveat is printed with it.

## What it will not do

- Profile macros. prumo stops at package, file and page; `texprof` (TeX Live, pdfTeX only)
  goes below that.
- Change your build. The probe adds hooks and writes one record file; the PDF is
  byte-identical with and without it, and that is a test.
- Touch your files. Builds run on a copy; artifacts go to your cache directory (or whatever
  `--out` says), nothing next to your document. `prune --apply` is the one destructive command, it
  re-prints its plan first and needs `--yes`.
- Fetch anything. It reads the local `texlive.tlpdb`; no network, ever.
- Prove a package unused. "No observable effect" means this document, this build, these
  comparisons; a package can matter for input the document does not exercise, and the report
  says so every time.
- Resolve system fonts on XeTeX. XeTeX opens OpenType fonts outside kpathsea, so the loaded
  side of the fonts report stops at what the PDF says; LuaTeX's are paired through luaotfload.
- Plan a `prune` on MiKTeX. MiKTeX has no `texlive.tlpdb`, so `packages` reports files, not
  packages, and `prune` has nothing to work from. Everything else runs there (tested on
  MiKTeX 25.12, Windows 10).

## Building

```
cargo build
cargo test                      # parsers against checked-in artifacts; no TeX needed
cargo test --features tex       # fixtures through a real engine (needs TeX Live + poppler)
scripts/tex_tests.sh            # the same, inside the Docker image, when the host has no TeX
```

Regenerating the artifacts needs TeX Live and poppler-utils; the image in
`docker/Dockerfile.tex` has exactly that:

```
docker build -t prumo-tex -f docker/Dockerfile.tex .
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/w" -w /w prumo-tex bash scripts/make_artifacts.sh
```

## Documentation

`docs/` holds the design: vision, prior art, architecture, the probe, package resolution and
pruning, unused-package detection, phases, open questions, a glossary, and the lab notes of
what was verified on which TeX Live.

## License

The Rust code is licensed under either of Apache License, Version 2.0 (`LICENSE-APACHE`) or
MIT license (`LICENSE-MIT`) at your option. `tex/prumo.sty` is additionally available under
the LaTeX Project Public License 1.3c, so it can be distributed on CTAN on its own.
