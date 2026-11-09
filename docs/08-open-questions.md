# 08 — Open questions

For Kevin. Claude Code should not pick defaults for these; it should ask, or build around a
clearly marked placeholder.

## Decide before M0

1. **Name.** `prumo` is a working codename (Portuguese: plumb line). Check crates.io, CTAN and
   GitHub before the first push; one web search on 2026-11-06 found no crate or CTAN package by
   that name, but it was not conclusive. If it is taken, same lineage: `esquadro` (set square),
   `nível` (spirit level; the accent is a problem for a crate name), `trena` (tape measure),
   `fiel` (the pointer on a balance).
2. **License.** Apache-2.0 (matches your OSCAL and trust-stack repos) or MIT/Apache dual
   (Rust convention). `prumo.sty` on CTAN would want LPPL 1.3c or MIT — decide whether the
   `.sty` is dual-licensed so it can go to CTAN separately.
3. **GitHub home.** `kevinraymond/prumo` next to florilegium, or under Keyec.
4. **CI TeX image.** `texlive/texlive:latest` (full, ~5 GB, slow pulls) for the TeX jobs, or a
   custom `scheme-small` + the fixture packages image you maintain. Full is simpler; custom is
   faster and is itself a prumo demo.

## Decide before M2

5. **Minimum LaTeX kernel.** Hooks need 2020-10-01; raw options need 2021-06-01. Require
   2021-06 outright (simpler), or degrade gracefully (no `opts` records → unused detection
   disabled) for 2020-10 to 2021-05 kernels? TeX Live 2020 users are rare; Debian stable users
   are not, and Debian bookworm ships 2022.
   **Decided 2026-11-08: require 2021-06-01.** `doctor` refuses older kernels with the date;
   `prumo.sty` says `\NeedsTeXFormat{LaTeX2e}[2021/06/01]`. No degraded mode.

## Decide before M4

6. **Default for `--keep-hyphenation`.** `all` (safe, small) or `used` (purer). Recommend `all`.
7. **System fonts on XeTeX/LuaTeX.** v1 ships with "font packages unknown on these engines,
   keep all OpenType/TrueType font packages the PDF names" — acceptable, or must v1 resolve them
   properly via `luaotfload-tool`/`fc-match`? Affects whether XeTeX/LuaTeX corpora can be pruned
   safely in v1.
8. **`--apply` at all in v1?** Printing the plan and the `keep.txt` may be enough; letting the
   tool run `tlmgr remove` on someone's machine is the one way this project generates angry
   issues. Recommend: ship `--apply` behind `--yes` and with the Docker gate green, but
   consider leaving it out of the first release.

## Decide before M5

9. **Side-effect skip list.** Ship a list (see `06`) or test everything and let the user judge
   from the "no observable effect" verdict plus a note? A list is friendlier; no list is more
   honest. Recommend: list, with `--test-all`.
10. **Default comparison.** `text` (fast) or `render` (thorough, ~10×)? Recommend `text`
    with a hint to re-run with `render` when the candidate is layout-related.

## Product

11. **Scope of the first public release.** M1–M3 (profiler + graph + fonts: the screenshot
    features) as 0.1, with prune and unused following? Or wait for M4 so the launch has the
    "removed 6.5 GB" line? Recommend 0.1 at M3 — it is already more than exists, and the feedback
    will reshape M4/M5.
12. **CTAN upload of the probe.** Yes/no, and whether to name it `prumo.sty` or something
    generic like `buildprobe.sty` so the CTAN package outlives the tool's name.
