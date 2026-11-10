# 10 — Verified findings (lab notes, 2026-11-06)

Environment: Debian TeX Live 2023 (`pdfTeX 1.40.25`, LaTeX kernel `2023-11-01`), XeTeX and
LuaTeX from the same distribution, latexmk, Poppler `pdffonts`/`pdftotext`, Python 3, Rust
1.97. Everything below was run, not reasoned. Re-run `scripts/make_artifacts.sh` on your box
to confirm the same holds on your TeX Live.

## Probe

1. `\RequirePackage{prumo}` before `\documentclass` from the command line works on all three
   engines. Generic hooks `file/before|after`, `package/before|after`, `class/before|after`,
   `begindocument`, `enddocument`, `enddocument/end`, `shipout/after` all fire.
2. `\sys_timer:` resolves on all three engines; values are monotone; unit 1/65536 s. Basic
   fixture total: pdfTeX 153–222 ms, XeTeX ~214 ms, LuaTeX ~212 ms (sandbox CPU; relative
   numbers are what matter).
3. **PDF byte-identical with and without the probe** under `SOURCE_DATE_EPOCH=0` (pdfTeX,
   basic fixture, md5 equal). The probe is observation-only.
4. Without `-jobname`, every output is named `texput.*` — both with the engine directly and
   through `latexmk -usepretex`. With `-jobname=main` both routes work; latexmk's second pass
   rewrites `main.prumo`.
5. `prumo.sty`'s own `file/after` fires (hooks exist by then), its `file/before` did not; the
   guard in the `.sty` keeps the depth counter balanced (verified: no negative depths).
6. `main.tex` gets no `file-` event (`\end{document}` ends the job inside `\input`).
7. `\@raw@opt@<name>.<ext>` exists in this kernel and yields the requested options (`T1` for
   fontenc, `a4paper,margin=25mm` for geometry via the class). 32–33 `opts` events on the basic
   fixture.
8. Record counts: basic 197/204/208 (pdf/xe/lua), custom-class 137/130/141, corpus-b
   124/117/128.

## Recorder (`.fls`)

9. pdfTeX records `.tfm`, `.pfb`, `.pk` (generated under `TEXMFVAR`), `.fd`, `.enc`, `.map`,
   `.fmt`, `texmf.cnf`, `.aux`, `.out`. Duplicates occur (`t1pcr.fd` three times).
10. LuaTeX records fewer files (88 vs 134 on the basic fixture): no `.enc`, but it does record
    `.tfm` and `.pfb` for Type 1 fonts, plus `.lua` modules.
11. `\DeclareFixedFont{\x}{T1}{pcr}{m}{n}{10}` loads `pcrr8t.tfm` and `t1pcr.fd` without
    embedding anything — the "loaded but not used" font case is detectable from the `.fls`
    alone on pdfTeX (no `ucrr8a.pfb` INPUT) and from `pdffonts` on all engines.
12. Without `cm-super` installed, T1-encoded CM text comes out as **Type 3 bitmap fonts** and
    the `.fls` shows `.600pk` files being read from `TEXMFVAR`. Both are reliable signals for the
    font-hygiene warning.

## Font name matching (limitation found)

13. `pdffonts` names Type 3 fonts `F44`, `F37` … — no TFM name. Name matching cannot work for
    them; count them and say "partial match".
14. On XeTeX/LuaTeX with Latin Modern, TFM names (`ec-lmr10`) differ from PostScript names
    (`LMRoman10-Regular`), so the naive loaded-vs-embedded match over-reports. Fix for M3: parse
    the map file that was read (`pdftex.map` appears in the `.fls` on all engines here):
    `ec-lmr10 LMRoman10-Regular "enc" <lm-ec.enc <lmr10.pfb` gives TFM → PS name → PFB. On
    pdfTeX the `.pfb` INPUT lines already settle it.

## Reference analyzer

15. After collapsing each unit's own file frame, exclusive times are meaningful: custom-class
    fixture `siunitx` 54 ms exclusive / 82 ms inclusive of 195 ms total; basic fixture
    `hyperref` 29 ms exclusive / 100 ms inclusive of 222 ms, `xcolor` 18 ms (and unused).
16. Page times: basic fixture page 1 ≈ 27 ms, page 2 ≈ 3 ms (page 1 carries font loading and
    hyperref's first-shipout setup).

## Differential compilation

17. `\@namedef{ver@xcolor.sty}{blocked}\@namedef{opt@xcolor.sty}{}` before `\input{main}`:
    build succeeds, `pdftotext` output identical to baseline → removable.
18. Same for `amsmath` (fixture uses `align`): `! LaTeX Error: Environment align undefined.`,
    exit 1 → required.

## Not verified

- `texlive.tlpdb` parsing and `tlmgr` behaviour (Debian ships neither the file nor a working
  `tlmgr`). `fixtures/tlpdb/excerpt.tlpdb` is hand-written from the documented format.
- Hook behaviour on kernels older than 2023-11-01 (specifically 2020-10 to 2021-05, before
  `\@raw@opt@`).
- Windows paths in `.fls` (`PWD C:/...`, mixed separators).
- Anything about MiKTeX.

## Re-verified on TeX Live 2026 (2026-11-06, `texlive/texlive:latest`, kernel 2026-11-01)

Items 1–18 hold, with these corrections and additions:

19. **Probe invariance, all fixtures:** pdfTeX and XeTeX byte-identical with and without the
    probe. LuaTeX differs only in the trailer `/ID`, which hashes the output *directory*: two
    plain runs in different directories differ too, and with `/ID` masked (or the same
    directory) the probed PDF is identical. Test both runs in one directory.
20. **Cold luaotfload cache:** LuaTeX's first run in a fresh `HOME` scans every system font and
    the `.fls` gets 6,886 INPUT lines (basic fixture) instead of 139. Warm the cache once.
21. **Record counts on TL2026:** basic 187/187/189 (pdf/xe/lua), custom-class 137/128/137,
    corpus-b 214/207/214. `.fls` INPUT lines: basic 129/122/139.
22. **Installed tlpdb grammar** (5,265 stanzas, 302,458 lines, 17 MB): categories
    Package 4992 / TLCore 206 / Collection 40 / ConTeXt 18 / Scheme 9; `execute` verbs
    `addMap` 342, `AddHyphen` 88, `addMixedMap` 46, `AddFormat` 56, `addKanjiMap` 11;
    `relocated 1` on 9 stanzas (schemes); 13 `postaction` lines; file lines carry no
    `details=` attributes in the installed database; no `RELOC/` prefixes. `size=` is in
    4096-byte blocks (`TeXLive::TLConfig::BlockSize`).
23. **`texlive/texlive:latest` has no docs** (2.78 GB compressed), so stanzas have no
    `docfiles`; `latest-doc` is 7.2 GB.
24. **Kernel date source:** `latex.ltx` has `\edef\fmtversion` on one line and `{2026-11-01}`
    on the next; `doctor` reads it from `kpsewhich latex.ltx` without running an engine.
25. **scheme-small gap:** the three fixtures need 55 packages (97 with `depend` closure);
    scheme-small (429 packages) lacks exactly `cm-super courier fontname siunitx translations`.
    That image reproduces the full image's records and `pdffonts` output on 9/9 runs.
26. **XeTeX first-run font cache:** in a cold `HOME`, the first `\selectfont` (which happens
    inside `size10.clo`) makes XeTeX build the fontconfig cache: 757 ms of an 861 ms basic
    fixture run, attributed to `size10.clo` as a *file* frame. The attribution is right; it is
    why the `time` hint leads with the heaviest row even when that row is a file.
27. **XeTeX's `.fls` has no `pdftex.map`** on TL2026 (xdvipdfmx reads it in its own process);
    pdfTeX and LuaTeX record it. Finding 14 was wrong about "all three engines" here.
28. **Format-preloaded fonts are invisible to the recorder:** `cmr10`, `cmmi10`, `cmr7` are
    embedded in the basic fixture's PDF on pdfTeX and XeTeX but never appear as `.tfm`
    INPUTs. LuaTeX reloads them and does record them.
29. **LuaTeX reads virtual fonts eagerly:** `pcrr8t.vf` and `pcrr8r.tfm` are INPUTs even
    though no Courier glyph is set; the report shows the VF as virtual and the raw font as
    not embedded. pdfTeX and XeTeX read neither.
30. **No-text documents read no TFM at all** (corpus-b, one TikZ picture): `tfm_loaded` is
    empty and only `pdftex.map` is read.

## M4 — tlmgr behavior (2026-11-06, `texlive/texlive:latest`, tlmgr r80687)

31. **`tlmgr remove --no-depends` refuses any package that an installed scheme, collection
    or package still lists as a dependency** ("not removing X, needed by Y"), including when
    Y is in the same `remove` call, removes the rest of the batch, and exits non-zero for the
    batch. So: remove schemes in one call, then collections, then packages, and retry what
    was refused. `--force` is not needed.
32. **Binary stanzas go with their base package:** `tlmgr remove a2ping` removes
    `a2ping.x86_64-linux`; listing the stanza explicitly makes tlmgr refuse it ("needed by
    a2ping"). The plan lists a stanza only when its base package stays.
33. **XeTeX names CID-keyed OpenType fonts `<name>-Identity-H` in the PDF**
    (`LMRoman10-Regular-Identity-H`); the file is `lmroman10-regular.otf`. The OpenType
    provider match strips the CMap suffix. Without that, every XeTeX/LuaTeX document fell
    back to keeping all 249 OpenType/TrueType packages (2.9 GB).
34. **Packages can depend on collections** (`yb-book` → `collection-fontsextra` →
    `collection-basic`), so "schemes, then collections, then packages" cannot work; the
    removal order has to be one dependents-first order over all categories.
35. **Gate results:** first run (before 31–34 were handled) 5,263 → 658 packages, 4.4 G →
    3.0 G, every batch with refusals, still 9/9 identical; final run 5,263 → 165 packages,
    5,098 removed, 0 refused, texmf-dist 216 MB, 9/9 identical.
36. **`begindocument/before` loads look like preamble loads.** siunitx loads `color` from
    that hook; it appears under `main.tex` before the probe's `begindoc` record. The probe
    now records `predoc` at `begindocument/before`; nothing typeset changes (invariance test
    still byte-identical). `\AtBeginDocument` loads (graphicx → epstopdf-base) come after
    `begindoc`.
37. **Text comparison cannot see drawings:** blocking florilegium left `pdftotext` output
    identical while every margin vanished. The default `unused` mode now includes a 24 dpi
    ink check; `--compare render` (50 dpi) reported "24368 of 242190 pixels differ".
38. **Shipout hooks can be the whole cost:** with `shipout/before` recorded, florilegium.tex
    pages are 17/3/5/2 ms of typesetting and 367/343/340/380 ms of shipout; showcase page 3
    is 2233 ms of shipout. The extra hook keeps the PDF byte-identical (invariance test).
39. **biblatex reads its own files with the primitive `\input`** (`\blx@inputonce` →
    `\@@input`): `biblatex.def`, `blx-dm.def`, `blx-compat.def`, `standard.bbx`, `numeric.bbx`,
    `numeric.cbx`, `english.lbx`, `biblatex.cfg` fire no file hooks. Their time lands in
    biblatex's exclusive column (131 of 149 ms on the fixture), which is right at the package
    level; `doctor --check-log` lists them as "outside hooks" from the `.fls` side. `logreq.def`,
    `csquotes.def` and the `.bbl` go through the hooks.
40. **Kernel file substitution.** `\RequirePackage{atbegshi}` (beamer) loads
    `atbegshi-ltx.sty`; `\CurrentFile` says `atbegshi.sty`, `\@currname` says `atbegshi-ltx`.
    With the requested name the unit frame and its file frame never collapsed and the log
    cross-check reported a file never opened. The probe records `\CurrentFileUsed`. The `.fls`
    still lists `atbegshi.sty`: the kernel opens the requested file for its existence check.
41. **XeTeX records no font files in the `.fls`**, even for `\setmainfont{lmroman10-regular.otf}`
    by file name: it opens fonts itself, outside kpathsea. LuaTeX records the `.otf` on a cold
    luaotfload cache and only the cached `<stem>.luc` on a warm one. The fonts report pairs
    PDF names with either; on XeTeX they stay unpaired and the report says why.
42. **The Unicode engines set text in Latin Modern OpenType by default** (beamer's sans on
    XeTeX/LuaTeX, a plain article on LuaTeX without `fontenc`), as `CID Type 0C`. The oracle
    had called every embedded font with no TFM "preloaded by the format"; it now separates
    OpenType (engine-opened), preloaded (reverse map hits a TFM never read) and unknown
    (fonts inside included graphics). `xelatex`'s `fmttriggers` name `lm`, so `lm` is kept
    by the format before Q7 gets a say.
43. **Log lines that look like file opens:** fontspec's "- 'normal' (m/n) with NFSS spec." made
    `m/n` a file; `(fontspec)   …` continuation lines (`\MessageBreak`) carry parentheses.
    A path now starts at a root or has an extension in its last component, and
    `(<name>)` followed by two spaces is a message continuation.
44. **Docker builds the last stage by default.** `make images` had built `prumo-tex` from the
    `nocmsuper` stage, so a regeneration produced Type 3 artifacts; the Makefile now passes
    `--target clean`.
45. **TeX never reads biber's, bibtex's or makeindex's files**, so `prune` would remove
    them. The `.fls` proves them: `<job>.bcf` written → biber; `<job>.bbl` read with no
    `.bcf` → bibtex; `<job>.ind` read → makeindex. Keep reason "build tool".
46. **`-output-directory`:** the probe's `\iow_open` follows `openout`, so `<job>.prumo` lands
    with the `.fls` and `.log` in the output directory; with latexmk's `-auxdir` it lands in
    the aux directory and only the PDF in `-outdir`. A bare `latexmk` in the small image
    produces DVI (no `latexmkrc`), which is still a valid run with no PDF.
47. **Windows `.fls` handling is implemented from the documented formats, not verified on
    Windows:** TeX Live writes `C:/…` with the drive letter in either case, MiKTeX writes
    backslashes, both may use CRLF. Parsed into one spelling (forward slashes, lowercase
    drive) and classified against kpsewhich roots case-insensitively. A real Windows `.fls`
    in `fixtures/` would settle it.
48. **Blocking cannot model a soft dependency.** biblatex does `\@ifpackageloaded{csquotes}`
    and then uses csquotes's macros: with the bare marker it said "Outdated 'csquotes'
    package", with a dated marker "Undefined control sequence", either way "required" for a
    package the document never uses. Deleting `\usepackage{csquotes}` from the copy builds
    identically. `unused` now removes the request from the loader's source and blocks only
    when the request cannot be found.
49. **A timeout must kill.** On florilegium's `animation.tex`, removing `fancyhdr` made
    LuaTeX error on every frame in nonstopmode: 124 s for a 5 s document, with the verdict
    "timed out" only stamped afterwards. The build now runs in its own process group and
    the group is killed at the deadline, so latexmk's engine child dies with it.
50. **"No observable effect" can mean "loaded anyway".** florilegium's manual calls
    `\includegraphics` and still passed the removal test for `\usepackage{graphicx}`,
    because tikz loads graphicx. The package is used; the line is redundant. `unused` now
    re-reads the candidate build's records and reports "loaded anyway (tikz.sty via
    pgfcore.sty loads it without this line)". The blocking fallback leaves no `pkg` frame
    and cannot produce the verdict.
51. **A cold luaotfload cache lands in `size11.clo`.** The first LuaTeX build in a fresh
    `TEXMFVAR` took 6.7 s on the manual, 4.8 s of it in `size11.clo`'s exclusive column:
    `\normalsize` is the first font selection, and luaotfload builds its font name database
    there (3,647 files read, 3,358 of them fonts, versus 257 warm; `luaotfload-names.lua.gz`
    appears as an `OUTPUT` in the `.fls`). `time` now warns when that output is present and
    names the unit holding the time. Warm, the same build is 1.5 s.
52. **All three timers are wall clock.** On LuaTeX, l3kernel's `\sys_timer:` is not
    `os.clock()`: since 2020-10 (the same code at the 2021-06-01 and 2023-11-01 tags, and in
    TeX Live 2026's `expl3.lua`) it sets `epoch = os.gettimeofday() - os.clock()` once, when
    its Lua code loads, and reads `gettimeofday() - epoch`. `os.clock()` only moves the zero
    back to about process start. In the TeX Live 2026 image the first record's timer is within
    5 ms of the record file's birth time after the spawn on all three engines (timestamps move
    in 4 ms steps there), so the time before the first record is startup and the process wall
    time minus the last record is finalization. prumo now times the engine process (1 ms
    polling) and reports both.
53. **`begindocument/end` is crowded.** 92 files in TeX Live 2026 add code to it, amsmath and
    tagpdf among them, and prumo's chunk runs first. `docstart` uses `\AddToHookNext`, whose
    code runs after every chunk and after document-level code (checked with `\ShowHook` and
    `\typeout`). Same for `end` on `enddocument/end`. `env/<name>/after` is a reversed hook, so
    prumo's chunk runs last there without help.
54. **Environment records.** `\PrumoEnvironments{...}` adds `env+`/`env-` on the generic env
    hooks. The PDF stays byte-identical on all three engines, on TeX Live 2026 and on Ubuntu
    22.04's TeX Live 2021 (kernel 2021-11-15). tabularx typesets its body three times on the
    fixture, so a `tikzpicture` inside it is recorded three times; a `longtable` across a
    page break has the shipout inside it. A discarded shipout leaves a `ship` with no `page`.
55. **`compare` ranked engines by the wrong total.** Its first table and hint used the span
    from the first record to the last, which leaves out startup and finalization. On the
    minimal document that span made LuaLaTeX fastest (80 ms) while its wall time was the
    slowest (475 ms against 202 ms for pdfLaTeX). It now ranks by wall time.
56. **Loaded from the main file's first line, the probe lost its first event.** The record
    file opened on the first `file/before`; inside the main file the first event is
    `class/before`, so `cls+` was dropped and the parser saw a `cls-` with nothing open. Any
    event opens the file now, nothing reopens it after `end`, and prumo's own
    `package/after` is skipped like its `file/after`. Overleaf's compile service returns every
    file the build writes and its editor hides only `output.fls` and `output.fdb_latexmk`
    (read in overleaf/overleaf, not tried on overleaf.com); `prumo time output.prumo` reads
    the records with no build.
57. **`luaotfload.patch_font` does not count `\addfontfeature` fonts.** 200 identical
    `\addfontfeature{LetterSpace=5}` (or `Numbers=OldStyle`, or `Color=FF0000`) make one new
    font (`font.nextid()` up by one) and no `patch_font` call; 10 new sizes make 11 font ids
    and 10 calls. Repeated identical `\addfontfeature` defines its font once.
58. **Public documents.** ClassicThesis 4.3 (CTAN, 41 pages, biblatex through latexmk), the
    metropolis demo (32 pages), the Llama 2 paper's arXiv source (33 packages, 76 pages) all
    run through `time`, `compare`, `fonts`, `packages` and `graph`. kaobook's documentation
    (master 22225a1) fails on TeX Live 2026 with or without the probe (`Command
    \c@proposition already defined` in kaotheorems.sty); prumo now prints that first error
    instead of only the exit code. On the arXiv paper, 18% of the time is shipout, most of it
    pdfTeX writing images on the pages that carry them, so the shipout hint names page writing
    as well as hooks.
59. **pdfTeX reads `pdftex.map` during the first shipout.** The log shows the map file
    inside page 1's brackets (`[1{.../pdftex.map}]`). On a one-line document the shipout phase
    is 45 ms of 132 ms on pdfLaTeX, all of it page 1; how much of that is the map was not
    isolated.
