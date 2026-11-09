# 04 — The probe: `tex/prumo.sty`

The whole TeX side is one ~90-line package that listens to kernel hooks and writes a record
file. It is in this pack, and it works (TeX Live 2023; pdfTeX, XeTeX, LuaTeX). Read it before
reading this.

## Design constraints

- **Observe, never act.** No macro redefinitions, no `\AtBeginDocument` code that typesets, no
  `\write` to the `.aux`. Acceptance: PDF identical with and without the probe (ignore
  `/CreationDate`, `/ModDate`, `/ID`; or set `SOURCE_DATE_EPOCH` and compare bytes).
- **Load before `\documentclass`.** This is why it is a `\RequirePackage` on the command line
  and not a `\usepackage` in the preamble. It needs `class/before`.
- **Kernel-only dependencies.** expl3 is in the kernel since 2020-02; the hook system since
  2020-10; raw package options since 2021-06. The floor is 2021-06-01 (Q5, decided
  2026-11-08: one floor, no degraded mode). `\NeedsTeXFormat{LaTeX2e}[2021/06/01]` enforces
  it; `prumo doctor` refuses older kernels before running.
- **Never wrap lines.** Records go to a dedicated file via `\iow_now:Nx`, not to the log. TeX
  wraps terminal/log output at `max_print_line` but **not** `\write` to a file stream, so long
  paths survive intact.

## Record format

One event per line, space-separated, **name is always the last field and may contain spaces**:

```
<event> <timer> <depth> <name>
```

| event | name field | fires on |
|---|---|---|
| `start` | `<engine> <fmtversion>` e.g. `pdftex 2023-11-01` | first event; opens the file |
| `file+` / `file-` | the file actually read (`\CurrentFileUsed`), e.g. `t1pcr.fd`; differs from `\CurrentFile` when the kernel substitutes a file (`atbegshi.sty` → `atbegshi-ltx.sty`, beamer; changed 2026-11-07) | `file/before`, `file/after` |
| `pkg+` / `pkg-` | package name without extension | `package/before`, `package/after` |
| `opts` | raw option list as requested, e.g. `a4paper,margin=25mm` | right after `pkg+`, only if the kernel recorded options |
| `cls+` / `cls-` | class name | `class/before`, `class/after` |
| `predoc` | (empty) | `begindocument/before` — the end of the preamble proper; packages loaded between `predoc` and `begindoc` come from other packages' hooks (added 2026-11-06) |
| `begindoc` | (empty) | `begindocument` |
| `ship` | the page about to be shipped | `shipout/before` — the shipout hooks (eso-pic, florilegium, hyperref links) run between `ship` and `page` (added 2026-11-06) |
| `page` | shipout counter (1-based, prumo's own) | `shipout/after` |
| `enddoc` | (empty) | `enddocument` |
| `end` | (empty) | `enddocument/end`; closes the file |

`timer` is `\sys_timer:` — scaled seconds (1/65536 s) since engine start. `depth` is the file
nesting depth at the time of the event (0 = the main file level). The parser does not need
`depth` (the `+`/`-` pairs are enough); it is there for humans and for a cheap sanity check.

Excerpt (basic fixture, pdfTeX):

```
start 5481 0 pdftex 2023-11-01
file+ 5481 0 main.tex
cls+ 5524 1 article
file+ 5553 1 article.cls
file+ 5624 2 size10.clo
file- 5679 2 size10.clo
file- 5763 1 article.cls
cls- 5782 1 article
pkg+ 5828 1 fontenc
opts 5831 1 T1
file+ 5856 1 fontenc.sty
...
begindoc 16471 1
page 18248 1 1
enddoc 18389 1
page 18442 1 2
end 18492 1
```

## Traps found while building it (all handled in the shipped `.sty`)

1. **Job name becomes `texput`** if nothing passes `-jobname`. The record file, PDF, log — all
   `texput.*`. The probe warns; the CLI always passes `-jobname`.
2. **prumo.sty's own `file/after` fires, its `file/before` did not** (the hooks did not exist
   yet). Without the guard the depth counter goes to −1 for the rest of the run.
3. **`main.tex` never gets a `file-`** because `\end{document}` ends the job inside the
   `\input`. The analyzer closes open frames at `end`.
4. **Unit vs file frames.** `pkg+ siunitx` is followed by `file+ siunitx.sty`. Both are real
   events; attribute time to the unit by collapsing its own file frame (see `03-architecture.md`).
6. **Substituted files.** `\RequirePackage{atbegshi}` loads `atbegshi-ltx.sty` (kernel file
   substitution). `\CurrentFile` says `atbegshi.sty`, `\@currname` says `atbegshi-ltx`; with the
   requested name the unit frame and its file frame never collapse and the log cross-check
   reports a file that was never opened. The probe records `\CurrentFileUsed`.
5. **Options are not in `\@classoptionslist`** (that is the class's list). The raw request is
   in `\@raw@opt@<name>.<ext>` (kernel 2021-06+); guard with `\cs_if_exist:cT`.
6. **`page` numbering is prumo's own counter**, not `\thepage`, because `\thepage` restarts
   (front matter) and may be roman.

## What the hooks do *not* see

- Files loaded with the TeX primitive `\input` (`\input foo` without braces, or `\@@input`) — rare
  in packages, common in old plain-TeX-style code. They still appear in the `.fls`. Report them as
  "outside hook coverage"; do not fake a tree position.
- Font files (`.tfm`, `.pfb`, …) — those are not `\input`. They come from the `.fls` only.
- Time spent *between* packages at the top level of the preamble (e.g. a big `\newcommand` block
  in `main.tex`) lands in `main.tex`'s exclusive time. That is correct and worth saying in the
  report ("38 ms in main.tex itself, outside any package").

## Engine notes (verified on TeX Live 2023)

| | pdfTeX | XeTeX | LuaTeX |
|---|---|---|---|
| `\sys_timer:` source | `\pdfelapsedtime` | `\elapsedtime` | Lua `os.clock()` |
| `.fls` records fonts | `.tfm`, `.pfb`, `.pk`, `.enc`, `.map`, `.fd` | `.tfm`, `.fd`; OpenType via system | `.tfm`, `.pfb`, `.fd`, `.lua`; OpenType via fontloader, **not** recorded |
| embed oracle | `.pfb`/`.pk` INPUT lines (exact) | `pdffonts` | `pdffonts` |
| `.fls` size, basic fixture | 134 INPUT lines | 133 | 88 |

Implication for the fonts report: on pdfTeX, "embedded" is read from the `.fls`; on XeTeX and
LuaTeX it is read from `pdffonts`, and the name match has to tolerate subset prefixes
(`AUUZAN+CMR10`), case, and the fact that Type 3 fonts show up as `F44` with no name. When
Type 3 fonts are present the match is partial — say so in the report. TFM names and PostScript
names also differ for most non-CM fonts (`ec-lmr10` vs `LMRoman10-Regular`): resolve through
the map file the run read (`pdftex.map` is in the `.fls` on all three engines), whose lines are
`<tfm> <psname> "<encoding>" <<enc-file> <<pfb-file>`. The Python oracle does not do this yet;
M3 does.

## Future probe additions (not in v0.1, keep the format extensible)

- `fd` attribution: in `file/after` for a `.fd` file, also record the innermost open `pkg`
  frame name. (The analyzer already knows it from the tree; nothing to add in TeX. Noted so
  nobody adds it twice.)
- `mem` events: `\tracingstats`-style memory numbers at `begindoc`/`end` are available via
  `\the\inputlineno`... no — memory usage is only printed to the log at the end. Parse the log's
  "Here is how much of TeX's memory you used" block in `texlog.rs` instead. Cheap, useful for
  TikZ-heavy documents.
- `shipout/before` as well as `/after` to separate page-building time from shipout time
  (hyperref and PDF compression live in shipout). Only if someone asks.

## Running it by hand

```
TEXINPUTS=tex: pdflatex -recorder -jobname=main "\RequirePackage{prumo}\input{main}"
python3 scripts/prumo_report.py main
```
