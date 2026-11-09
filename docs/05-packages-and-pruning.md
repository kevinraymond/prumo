# 05 — Package resolution and pruning

Goal: turn "files this build read" into "TeX Live packages this build needs", then invert it
across a corpus into "packages you can remove".

**Not verified in the sandbox** (Debian's TeX Live ships no `texlive.tlpdb`; `tlmgr` there is a
stub). Everything below is from the TeX Live sources and `tlmgr` docs; M0 verifies it against a
real install. `fixtures/tlpdb/excerpt.tlpdb` is hand-written in the documented format for parser
tests — replace it with a real excerpt in M0.

## Inputs

### `.fls` (from `-recorder`)

```
PWD /home/kevin/docs/report
INPUT /usr/share/texlive/texmf-dist/tex/latex/base/article.cls
INPUT /var/lib/texmf/web2c/pdftex/pdflatex.fmt
INPUT main.aux
OUTPUT main.log
```

Rules:
- Resolve relative paths against `PWD`. Canonicalize (symlinks matter: Debian and Homebrew
  symlink `texmf-dist`).
- Deduplicate (`t1pcr.fd` appears three times in the fixture).
- Classify by tree, using `kpsewhich -var-value` for `TEXMFDIST`, `TEXMFVAR`, `TEXMFSYSVAR`,
  `TEXMFLOCAL`, `TEXMFHOME`, `TEXMFSYSCONFIG`; anything under `PWD` is `Project`; the rest is
  `System` (`/etc/texmf/web2c/texmf.cnf`).
- Only `TexmfDist` files map to tlpdb packages. `TexmfVar` files are generated (formats, `.pk`,
  `pdftex.map`, `ls-R`) and map to *actions*, not packages (see keep set).

### `texlive.tlpdb`

Location: `$(kpsewhich -var-value TEXMFROOT)/tlpkg/texlive.tlpdb` (the *installed* database —
exactly what you want; it lists only installed packages). Plain text, UTF-8, stanzas separated
by blank lines. Lines relevant to prumo:

```
name hyperref
category Package            # Package | Collection | Scheme | TLCore | ConTeXt | ...
revision 71231
depend atbegshi             # zero or more; collections list their member packages here
depend ...
execute addMap ...          # font map registration — tells you it is a font package
execute AddFormat name=pdflatex engine=pdftex ...   # format providers
execute AddHyphen name=... file=...                 # hyphenation patterns (baked into formats)
runfiles size=98
 texmf-dist/tex/latex/hyperref/hyperref.sty          # one leading space per file line
 texmf-dist/tex/latex/hyperref/puenc.def
docfiles size=1234
 texmf-dist/doc/latex/hyperref/README.md details="Readme"
srcfiles size=...
 texmf-dist/source/latex/hyperref/hyperref.dtx
binfiles arch=x86_64-linux size=12                   # only in <pkg>.<arch> stanzas
 bin/x86_64-linux/pdftex
catalogue-license lppl1.3
```

Also present and to be skipped or special-cased: `00texlive.installation`, `00texlive.config`,
`00texlive.image`, `00texlive.autopatterns`, `<pkg>.<arch>` binary stanzas, `longdesc`
continuation lines, `relocated 1`.

Parser output:

```rust
struct Tlpdb {
    packages: HashMap<String, Tlpkg>,     // name → stanza
    file_index: HashMap<String, String>,  // "texmf-dist/tex/latex/hyperref/hyperref.sty" → "hyperref"
}
struct Tlpkg { name, category, depends: Vec<String>, runfiles: Vec<String>, executes: Vec<String>, size: u64 }
```

Lookup key: the path relative to the texmf root, i.e. strip everything up to and including the
component before `texmf-dist/`. A file not in the index is reported, not dropped (local
additions in `TEXMFLOCAL`, or a Debian-specific path).

## `prumo packages`: one document

```
used_files   = inputs.filter(tree == TexmfDist)
used_pkgs    = { file_index[f] for f in used_files }          # plus "unresolved: [...]"
needed       = closure(used_pkgs ∪ infra(engine, format)) over `depend`
```

Output: the package list, each with the first file that pulled it in and the LaTeX unit that
loaded that file (from the tree), e.g.

```
siunitx          tex/latex/siunitx/siunitx.sty          via obra.cls
translations     tex/latex/translations/translations.sty via siunitx
amsfonts         fonts/tfm/public/amsfonts/cm/cmmi10.tfm  via (font load)
```

## The keep set (infrastructure the `.fls` does not prove)

Formats are built at install time, so hyphenation patterns, `latex.ltx`, `tex-ini-files` etc.
never appear as INPUT in a normal run — only the `.fmt` does. Keep, always:

- `texlive.infra`, `kpathsea`, `texlive-scripts`, `texlive-msg-translations` (optional), `tlperl.windows` on Windows
- the engine package(s) seen in `start` records: `pdftex`, `xetex`, `luatex`, `luahbtex` (+ their `.<arch>` stanzas)
- format providers for the formats used: the stanza whose `execute AddFormat name=<format>` matches (`latex-bin` for `pdflatex`/`lualatex`, `xetex` for `xelatex`); `tex-ini-files`, `latex`, `latex-fonts`, `latex-firstaid`, `l3kernel`, `l3backend`, `l3packages`, `unicode-data`, `etex-pkg`? (verify: it is `latex`'s own `depend` list — read it from the tlpdb rather than hard-coding)
- `hyphen-base` plus, by `--keep-hyphenation`: `all` (default — keep every installed `hyphen-*`, they are small and removing them regenerates formats) or `used` (keep those whose language appears in the corpus via `babel-<lang>`/`.ldf`/`.ini` loads)
- `dvips`/`dvipdfmx`/`xdvi` etc. are **not** kept unless the corpus used them (XeTeX uses `xdvipdfmx`, which is in the `xetex` package)
- `cm`, `amsfonts` and whatever the `.fls` proves

Everything in the keep set is a `depend` closure root, same as used packages. Put the list in
`src/prune/keep.rs` with a comment per entry saying why, and make `--keep NAME,...` extend it.

## `prumo prune`: a corpus

```
prumo prune 'docs/**/*.tex' --engine pdflatex
```

1. Discover main files: `.tex` files containing `\documentclass` (skip files that are `\input`
   by another file in the set — the `.fls` of the parents tells you).
2. Run each (cached), union `needed` sets.
3. `installed = tlpdb.packages where category ∈ {Package, TLCore, ConTeXt, Collection, Scheme}`.
4. `remove = installed − needed`, then drop every Collection and Scheme from `remove` and list
   them first ("meta-packages; removing them does not remove files but stops `tlmgr update`
   from pulling new members") — actually: remove collections first, *then* packages, because
   `tlmgr remove` refuses to remove a package another installed package depends on unless
   `--force`. Order the plan: schemes, collections, then packages in reverse dependency order.
5. Print:

```
Corpus: 14 documents, 3 engines (pdftex 12, xetex 2)
Installed: 4,612 packages (7.1 GB)   Needed: 211 (640 MB)   Removable: 4,401 (6.5 GB)

Plan (dry run — nothing was changed):
  tlmgr remove --no-depends scheme-full collection-fontsextra ... (31 meta-packages)
  tlmgr remove --no-depends <4,370 packages>      # written to .prumo/prune-plan.txt
Keep list written to .prumo/keep.txt (use with `tlmgr install $(cat keep.txt)` on a fresh scheme-infraonly)
```

6. `--apply` re-computes, prints, asks for `--yes`, then runs `tlmgr remove --no-depends` in
   batches of ~200 and `tlmgr path add`/`fmtutil-sys --all` if tlmgr did not already. Log every
   command to `.prumo/prune.log`.

Also emit the *additive* form, which is the better story for CI images and air-gapped machines:
an `install-tl` profile (`selected_scheme scheme-infraonly`) plus `tlmgr install $(cat keep.txt)`.
That reproduces the pruned install from scratch instead of carving it out of a full one.

## Verification gate (M4 acceptance)

In Docker (`texlive/texlive:latest`, full scheme):
1. Build every corpus document; keep `pdftotext -layout` output and page counts.
2. Run `prumo prune --apply --yes` with the plan.
3. Rebuild every document. Text and page counts must be identical. Any "File not found" in a
   log fails the gate and names the file; the fix is almost always a keep-set entry, not a bug
   in the user's document.

Run this on the fixtures in CI (weekly job; it is slow) and on florilegium as the real-world
case.

## Known hard cases (document, do not solve in v1)

- **Fonts via fontspec/system fonts** (XeTeX/LuaTeX): not in `.fls`; the TeX Live font packages
  that provide the `.otf` are in `texmf-dist/fonts/opentype/...` and LuaTeX's fontloader does
  resolve them there, but through its own cache. Treat font packages as *unknown* on those
  engines in v1 and keep all `fonts/opentype` and `fonts/truetype` providers that `pdffonts`
  names — match by font file name from `luaotfload-tool --find=<name>` / `fc-match`. Open
  question Q7.
- **`\input` of files by `\jobname`-dependent or conditional paths**: the corpus is only as
  complete as the builds you ran. Say so in the output header.
- **Locally added packages in `TEXMFLOCAL`/`TEXMFHOME`**: not tlpdb-managed; list them so the
  user knows they are on their own.
- **Debian/Ubuntu TeX Live**: no tlpdb. `prumo packages` falls back to `dpkg -S <file>` to name
  the Debian package; `prune` refuses with a clear message. (Debian's package granularity is too
  coarse to be worth more.)
