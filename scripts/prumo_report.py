#!/usr/bin/env python3
"""Reference analyzer for prumo records — the oracle the Rust port must match.

Reads <job>.prumo (hook events), <job>.fls (-recorder), and optionally the
output PDF (via `pdffonts`) and prints:

  1. per-package / per-class timing, inclusive and exclusive
  2. per-page typesetting time
  3. who-loaded-whom tree
  4. fonts loaded (TFM) vs fonts embedded (PDF), with Type 3 warnings
  5. the phases of the run (startup to end-document) and per-environment totals

Usage:  prumo_report.py <jobname> [--json]

Deliberately dependency-free and ~200 lines. Not the product; a fixture.
"""
import json
import os
import re
import subprocess
import sys
from collections import defaultdict

SCALED = 65536.0  # \sys_timer: unit is 1/65536 s on all engines


def read_records(path):
    """Yield (event, t_seconds, depth, name) from a .prumo file."""
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not line:
                continue
            parts = line.split(" ", 3)
            ev, t, depth = parts[0], int(parts[1]) / SCALED, int(parts[2])
            name = parts[3] if len(parts) > 3 else ""
            yield ev, t, depth, name


def build_tree(records):
    """Return (root, frames, pages, phases, events).

    Each frame: {kind, name, start, end, children, parent}. pkg/cls frames
    wrap the file frame of the same unit; we attribute time to the pkg/cls
    frame when present and to the file frame otherwise.

    events: {"ships": [(start, end)] for shipouts that completed, "discarded":
    [page] for a `ship` with no `page` (a package discarded the box), "envs":
    [(name, start, end, nested)] where nested means inside another of the same
    name}.
    """
    root = {"kind": "job", "name": "<job>", "start": None, "end": None,
            "children": [], "parent": None}
    stack = [root]
    frames = []
    pages = []
    phases = {}
    ships, discarded, envs, env_stack = [], [], [], []
    pending_ship = None
    last_t = None
    for ev, t, depth, name in records:
        if root["start"] is None:
            root["start"] = t
        last_t = t
        if ev in ("file+", "pkg+", "cls+"):
            fr = {"kind": ev[:-1], "name": name, "start": t, "end": None,
                  "children": [], "parent": stack[-1]}
            stack[-1]["children"].append(fr)
            stack.append(fr)
            frames.append(fr)
        elif ev in ("file-", "pkg-", "cls-"):
            fr = stack.pop()
            assert fr["kind"] == ev[:-1], f"unbalanced: {ev} {name} vs {fr}"
            fr["end"] = t
        elif ev == "opts":
            stack[-1]["options"] = name
        elif ev == "ship":
            if pending_ship is not None:
                discarded.append(pending_ship[0])
            pending_ship = (int(name), t)
        elif ev == "page":
            pages.append((int(name), t))
            if pending_ship is not None and pending_ship[0] == int(name):
                ships.append((pending_ship[1], t))
            elif pending_ship is not None:
                discarded.append(pending_ship[0])
            pending_ship = None
        elif ev == "env+":
            env_stack.append((name, t))
        elif ev == "env-":
            # Close the innermost open instance of this name; an unmatched env- is ignored.
            for i in range(len(env_stack) - 1, -1, -1):
                if env_stack[i][0] == name:
                    n, t0 = env_stack.pop(i)
                    envs.append((n, t0, t, any(e[0] == n for e in env_stack)))
                    break
        elif ev in ("predoc", "begindoc", "docstart", "enddoc", "lastpage", "end", "start"):
            phases[ev] = t
    if pending_ship is not None:
        discarded.append(pending_ship[0])
    # Environments still open at the last record (a failed build) close there.
    for i, (n, t0) in enumerate(env_stack):
        envs.append((n, t0, last_t, any(e[0] == n for e in env_stack[:i])))
    root["end"] = last_t
    # \end{document} never returns from \input{main}, so main.tex (and the
    # job) have no file- event: close them at the last timestamp.
    for fr in frames:
        if fr["end"] is None:
            fr["end"] = last_t
    collapse_unit_files(root)
    frames = [fr for fr in frames if not fr.get("collapsed")]
    events = {"ships": ships, "discarded": discarded, "envs": envs}
    return root, frames, pages, phases, events


def collapse_unit_files(fr):
    """A pkg/cls frame wraps the file frame of its own .sty/.cls. Splice that
    file frame's children into the unit so exclusive time lands on the unit."""
    new_children = []
    for c in fr["children"]:
        if fr["kind"] in ("pkg", "cls") and c["kind"] == "file" \
                and os.path.splitext(c["name"])[0] == fr["name"]:
            c["collapsed"] = True
            for g in c["children"]:
                g["parent"] = fr
            new_children.extend(c["children"])
        else:
            new_children.append(c)
    fr["children"] = new_children
    for c in fr["children"]:
        collapse_unit_files(c)


def exclusive(fr):
    inc = (fr["end"] or 0) - fr["start"]
    kids = sum((c["end"] or 0) - c["start"] for c in fr["children"])
    return inc, inc - kids


def unit_table(frames):
    """Aggregate by LaTeX unit (pkg/cls); files not wrapped by a unit are
    reported by file name. Returns rows sorted by exclusive time."""
    rows = {}
    for fr in frames:
        key = (fr["kind"], fr["name"]) if fr["kind"] in ("pkg", "cls") else ("file", fr["name"])
        inc, exc = exclusive(fr)
        r = rows.setdefault(key, {"kind": key[0], "name": key[1],
                                  "inclusive": 0.0, "exclusive": 0.0, "loads": 0})
        r["inclusive"] += inc
        r["exclusive"] += exc
        r["loads"] += 1
    return sorted(rows.values(), key=lambda r: -r["exclusive"])


def page_times(pages, phases):
    """Page 1 counts from docstart (the end of \\begin{document}), else begindoc."""
    out, prev = [], phases.get("docstart", phases.get("begindoc"))
    for n, t in pages:
        out.append((n, t - prev if prev is not None else 0.0))
        prev = t
    return out


def ship_in(ships, a, b):
    """Shipout time inside [a, b]: the overlap of every completed shipout with the range."""
    return sum(max(0.0, min(e, b) - max(s, a)) for s, e in ships)


def run_phases(root, phases, events):
    """The run cut into consecutive phases, each without the shipouts inside it, so the six
    add up to the timer at the last record. A phase whose boundary records are missing (an
    older probe, a build that stopped) is None. Finalization, after the last record, needs
    the process wall time and is the CLI's (the Rust side adds it).

      startup         timer zero (about process start) to the first record
      preamble        first record to predoc (begindocument/before)
      begin_document  predoc to docstart (begindocument/end): \\AtBeginDocument and the .aux
      typesetting     docstart to enddoc, less shipouts
      shipout         every ship..page, wherever it happened
      end_document    enddoc to the last record, less shipouts
    """
    ships = events["ships"]
    t0, end = root["start"], root["end"]
    if t0 is None:
        return None
    predoc = phases.get("predoc", phases.get("begindoc"))
    docstart = phases.get("docstart", phases.get("begindoc"))
    enddoc = phases.get("enddoc")

    def span(a, b):
        return None if a is None or b is None else (b - a) - ship_in(ships, a, b)
    return {
        "startup_s": t0,
        "preamble_s": span(t0, predoc),
        "begin_document_s": span(predoc, docstart),
        "typesetting_s": span(docstart, enddoc if enddoc is not None else (end if docstart is not None else None)),
        "shipout_s": sum(e - s for s, e in ships),
        "end_document_s": span(enddoc, end),
        "after_last_page_s": span(phases.get("lastpage"), end),
        "end_s": end,
    }


def environment_table(events):
    """Per environment name: instances, time (outermost instances of the name only, less
    the shipouts inside them) and the slowest instance. Sorted by time, descending."""
    rows = {}
    for name, s, e, nested in events["envs"]:
        dt = (e - s) - ship_in(events["ships"], s, e)
        r = rows.setdefault(name, {"name": name, "count": 0, "total_s": 0.0, "max_s": 0.0})
        r["count"] += 1
        if not nested:
            r["total_s"] += dt
        r["max_s"] = max(r["max_s"], dt)
    return sorted(rows.values(), key=lambda r: (-r["total_s"], r["name"]))


def print_tree(fr, indent=0, out=None):
    out = out if out is not None else []
    for c in fr["children"]:
        inc, exc = exclusive(c)
        out.append(f"{'  ' * indent}{c['name']}  ({inc * 1000:.1f} ms incl, {exc * 1000:.1f} ms excl)")
        print_tree(c, indent + 1, out)
    return out


FONT_EXT = {".tfm": "metric", ".pfb": "type1", ".pfa": "type1", ".otf": "opentype",
            ".ttf": "truetype", ".pk": "bitmap", ".vf": "virtual", ".fd": "fd"}


def read_fls(path):
    inputs = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if line.startswith("INPUT "):
                inputs.append(line[6:].strip())
    return inputs


def read_font_map(fls_inputs):
    """TFM -> (PostScript name, font file) from the pdftex.map the run read.

    Map lines look like `ecrm1000 SFRM1000 " T1Encoding ReEncodeFont " <cm-super-t1.enc <sfrm1000.pfb`;
    the font file is the `<` token with an outline extension (`<<` and `<[` prefixes are
    pdfTeX's partial/full-download markers). XeTeX and LuaTeX read pdftex.map too.
    """
    out = {}
    maps = [p for p in fls_inputs if os.path.basename(p) == "pdftex.map" and os.path.exists(p)]
    if not maps:
        # XeTeX's .fls has no pdftex.map (xdvipdfmx, a separate process, reads it).
        try:
            found = subprocess.run(["kpsewhich", "--format=map", "pdftex.map"],
                                   capture_output=True, text=True, check=True).stdout.strip()
            maps = [found] if found else []
        except (OSError, subprocess.CalledProcessError):
            maps = []
    for p in maps:
        with open(p, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if line.startswith("%") or not line.strip():
                    continue
                toks = line.split()
                tfm = toks[0]
                ps = toks[1] if len(toks) > 1 and not toks[1].startswith(("<", '"')) else None
                files = [t.lstrip("<[") for t in toks if t.startswith("<")]
                font_file = next((f for f in files if f.lower().endswith((".pfb", ".pfa", ".ttf", ".otf"))), None)
                out.setdefault(tfm, (ps, font_file))
    return out


def font_report(fls_inputs, pdf):
    loaded = sorted({os.path.basename(p)[:-4] for p in fls_inputs if p.endswith(".tfm")})
    bitmap = sorted({os.path.basename(p) for p in fls_inputs if re.search(r"\.\d+pk$", p)})
    virtual = sorted({os.path.basename(p)[:-3] for p in fls_inputs if p.endswith(".vf")})
    outline_files = {os.path.basename(p) for p in fls_inputs if p.lower().endswith((".pfb", ".pfa", ".ttf", ".otf"))}
    # LuaTeX with a warm luaotfload cache reads only the cached tables (<stem>.luc), not the .otf.
    cached_fonts = {os.path.basename(p) for p in fls_inputs if p.lower().endswith(".luc")}
    fontmap = read_font_map(fls_inputs)
    embedded, type3 = [], 0
    if pdf and os.path.exists(pdf):
        try:
            txt = subprocess.run(["pdffonts", pdf], capture_output=True, text=True, check=True).stdout
            for line in txt.splitlines()[2:]:
                cols = line.split()
                if not cols:
                    continue
                name, ftype = cols[0], " ".join(cols[1:3])
                if "Type 3" in line:
                    type3 += 1
                embedded.append((name.split("+")[-1], ftype))
        except (OSError, subprocess.CalledProcessError):
            pass
    emb_lower = {n.lower() for n, _ in embedded}
    unused = [f for f in loaded if f.lower() not in emb_lower]
    # Resolved through the map file: a TFM is embedded when its outline file was read or
    # its PostScript name is in the PDF. Virtual fonts (.vf read) delegate to their raw
    # fonts, which appear as TFMs of their own. Format-preloaded fonts (cmr10 and friends)
    # never show up as .tfm INPUTs; name them from the PDF through the reverse map.
    ps_names = {t: fontmap[t][0] for t in loaded if t in fontmap and fontmap[t][0]}
    embedded_resolved = sorted(
        t for t in loaded if t in fontmap and (
            (fontmap[t][1] in outline_files) or (fontmap[t][0] or "").lower() in emb_lower
            or t.lower() in emb_lower))
    unresolved = sorted(t for t in loaded if t not in fontmap and t not in virtual)
    not_embedded = [t for t in loaded if t not in embedded_resolved and t not in virtual]
    reverse = {}
    for t, (ps, _) in fontmap.items():
        if ps:
            reverse.setdefault(ps.lower(), t)
    loaded_ps = {ps.lower() for ps in ps_names.values()}
    loaded_lower = {t.lower() for t in loaded}
    # Embedded fonts no loaded TFM accounts for, in three kinds. OpenType (CID or TrueType in
    # the PDF): fontspec on XeTeX/LuaTeX. XeTeX opens them itself, so the .fls lists none;
    # LuaTeX reads them through kpathsea and the file is paired by its stem (LMRoman10-Regular
    # -> lmroman10-regular.otf, or lmroman10-regular.luc from a warm luaotfload cache); XeTeX's
    # -Identity-H suffix is dropped.
    # Preloaded: a PostScript name the map resolves to a TFM that was never read (cmr10 and
    # friends live in the format). Unknown: neither, e.g. a font inside an included PDF.
    opentype, preloaded, unknown = [], set(), set()
    stems = {f.lower().rsplit(".", 1)[0]: f for f in cached_fonts}
    stems.update({f.lower().rsplit(".", 1)[0]: f for f in outline_files})
    for n, ftype in embedded:
        if "Type 3" in ftype or n.lower() in loaded_ps or n.lower() in loaded_lower:
            continue
        if "CID" in ftype or "TrueType" in ftype:
            name = re.sub(r"-Identity-[HV]$", "", n)
            opentype.append({"name": name, "file": stems.get(name.lower())})
        elif n.lower() in reverse:
            preloaded.add(reverse[n.lower()])
        else:
            unknown.add(n)
    opentype = sorted({o["name"]: o for o in opentype}.values(), key=lambda o: o["name"])
    return {"tfm_loaded": loaded, "embedded": embedded, "type3": type3,
            "bitmap_pk": bitmap, "loaded_not_embedded": unused,
            "ps_names": ps_names, "virtual": virtual, "embedded_resolved": embedded_resolved,
            "loaded_not_embedded_resolved": not_embedded, "no_map_entry": unresolved,
            "preloaded": sorted(preloaded), "opentype": opentype, "unknown_embedded": sorted(unknown),
            "partial": type3 > 0, "map_entries": len(fontmap)}


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    job = sys.argv[1]
    as_json = "--json" in sys.argv
    root, frames, pages, phases, events = build_tree(read_records(job + ".prumo"))
    table = unit_table(frames)
    ptimes = page_times(pages, phases)
    run = run_phases(root, phases, events)
    envs = environment_table(events)
    fls = read_fls(job + ".fls") if os.path.exists(job + ".fls") else []
    fonts = font_report(fls, job + ".pdf")
    total = (root["end"] or 0) - (root["start"] or 0)
    preamble = (phases.get("begindoc", 0) - root["start"]) if root["start"] is not None else 0

    if as_json:
        print(json.dumps({"total_s": total, "preamble_s": preamble, "units": table,
                          "pages": ptimes, "fonts": fonts, "phases": run,
                          "environments": envs, "discarded_shipouts": events["discarded"]},
                         indent=2))
        return

    print(f"== prumo report: {job}  (total {total * 1000:.0f} ms, preamble {preamble * 1000:.0f} ms, "
          f"{len(pages)} pages, {len(fls)} files read)")
    print("\n-- slowest units (exclusive) --")
    for r in table[:12]:
        print(f"  {r['exclusive'] * 1000:7.1f} ms  {r['inclusive'] * 1000:7.1f} ms incl  {r['kind']:4} {r['name']}")
    if run:
        print("\n-- phases --")
        for k in ("startup", "preamble", "begin_document", "typesetting", "shipout", "end_document"):
            v = run[k + "_s"]
            print(f"  {k.replace('_', '-'):15} " + (f"{v * 1000:8.1f} ms" if v is not None else "       -"))
    if envs:
        print("\n-- environments --")
        for r in envs:
            print(f"  {r['total_s'] * 1000:7.1f} ms  {r['count']:4}x  max {r['max_s'] * 1000:6.1f} ms  {r['name']}")
    print("\n-- pages --")
    for n, dt in ptimes:
        print(f"  page {n}: {dt * 1000:.1f} ms")
    if events["discarded"]:
        print(f"  discarded shipouts: {len(events['discarded'])} (a package threw the page away)")
    print("\n-- load tree --")
    for line in print_tree(root)[:40]:
        print("  " + line)
    print("\n-- fonts --")
    print(f"  TFMs loaded: {len(fonts['tfm_loaded'])}, embedded in PDF: {len(fonts['embedded'])}, "
          f"Type 3: {fonts['type3']}, bitmap .pk read: {len(fonts['bitmap_pk'])}")
    if fonts["loaded_not_embedded_resolved"]:
        print("  loaded but not embedded: " + ", ".join(fonts["loaded_not_embedded_resolved"]))
    if fonts["preloaded"]:
        print("  preloaded by the format, embedded: " + ", ".join(fonts["preloaded"]))
    if fonts["opentype"]:
        print("  OpenType, opened by the engine: " + ", ".join(
            o["name"] + (" <- " + o["file"] if o["file"] else " (not in the .fls)") for o in fonts["opentype"]))
    if fonts["unknown_embedded"]:
        print("  in the PDF, from included graphics or unknown: " + ", ".join(fonts["unknown_embedded"]))
    if fonts["no_map_entry"]:
        print("  no map entry (bitmap or unknown): " + ", ".join(fonts["no_map_entry"]))
    if fonts["type3"]:
        print("  WARNING: Type 3 (bitmap) fonts in output — usually a missing cm-super/lmodern")


if __name__ == "__main__":
    main()
