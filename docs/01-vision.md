# 01 — Vision

## The request that started it

A LaTeX user with a working setup (TeXstudio, fast enough compiles, custom classes for building
documentation) said, in effect: *I don't need another compiler. I need something that helps me
tune my classes — what is slow, which packages I actually use, which fonts — and something that
makes my distribution lighter and easier to manage.*

That is two needs, and both are unserved (see `02-prior-art.md`):

1. **Build insight.** Where does compile time go, package by package? What did my class drag in?
   Which packages and fonts are dead weight?
2. **Distribution hygiene.** Of the ~4,500 packages in a full TeX Live, which do my documents
   need? Give me a smaller install I can reason about.

## What prumo is

A command-line build analyzer. It runs the document once with a tiny probe package injected
ahead of `\documentclass`, and reads the three things that run leaves behind — the probe's event
record, the engine's `-recorder` file list, and the PDF — into one model. From that model it
answers:

- `prumo time main.tex` — per-package, per-file and per-page time, inclusive and exclusive.
- `prumo graph main.tex` — which package loaded which, as a tree, DOT, or JSON.
- `prumo fonts main.tex` — fonts loaded vs fonts embedded; bitmap (Type 3) warnings.
- `prumo packages main.tex` — the exact `tlmgr` package set the document needs.
- `prumo prune docs/**/*.tex` — a corpus-wide keep list and a `tlmgr remove` plan (dry-run).
- `prumo unused main.tex` — (phase 2) `\usepackage` lines that can go, with evidence.

One compile for the first five. Everything is derived from data the build already produces; the
probe adds no typesetting and no dependencies beyond the LaTeX kernel.

## Who it is for

- People who maintain **custom classes or templates** (the original requester; anyone who owns a
  house style, a thesis class, a report template).
- People who maintain **TeX installations**: CI images, lab machines, air-gapped boxes, laptops
  with 7 GB of TeX Live they never asked for.
- Package authors who want to know what their package costs.

It is a small audience, but it is the audience that writes TUGboat articles and answers TeX.SE
questions. One good "a flame chart of my LaTeX build" screenshot does the marketing.

## Non-goals

- Not a compiler, build system, editor plugin or language server. It wraps `latexmk`; it does not
  compete with `texlab`.
- Not a per-macro profiler. `texprof` exists for that (pdfTeX). prumo stops at package/file/page.
- Not advice. "Which font should I use" and "what options make xyz happen" are questions for a
  human or a model, not for a static tool. prumo reports facts the user can act on.
- Not MiKTeX, in v1. Different package database. Revisit when someone asks.
- Not a package installer. It emits `tlmgr` commands; it does not become `tlmgr`.

## Why Rust

Single static binary users can drop next to their TeX install, including on air-gapped machines;
fast enough to run the differential pass in parallel; the author's primary language. The TeX
side is deliberately tiny so the Rust side carries the logic.

## Why now

The LaTeX hook management system (kernel 2020-10-01+) is what makes a no-patch probe possible,
and it is now old enough to be in every distribution people actually run. Before it, this tool
would have meant redefining `\@onefilewithoptions`, which is why nobody built it.
