#!/usr/bin/env python3
"""Charts for docs/benchmarks/README.md, as SVG, from the JSON under results/. Standard
library only. Each chart is written twice, for a light and a dark page; the README picks one
with <picture>, and the exact numbers are in the tables under each chart.

  phases     each document in its own panel and on its own axis: one bar per engine, cut
             into startup, preamble, body and writing the PDF, with every run's wall time
  overhead   every plain and every probed run, against the plain runs' median
  packages   the paper's package load times on pdfLaTeX and LuaLaTeX, by the difference
  whatif     what each experiment saved, as a share of the build, with the probe's bound

Color means phase, in every chart: startup is a neutral gray (no experiment changes it),
dark on a light page and light on a dark one, the preamble violet, the body magenta, writing
the PDF yellow. The same hue marks the experiment that shrinks that phase. Engines are told
apart by position and label, and in the package chart by a hollow or filled dot. The colors
are the dataviz reference palette's. Every pair of the four, the gray included, is at least
OKLab ΔE 13 apart under simulated protan and deutan vision and 19 apart under normal vision,
in both modes (measured with its validate_palette.js; the mid gray it had before sat 1.1
from the body's magenta for deutan readers).

Usage: charts.py [--results DIR] [--out DIR]
"""
import argparse
import json
import statistics
import tomllib
from html import escape
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = tomllib.loads((HERE / "corpus.toml").read_text())

THEMES = {
    "light": {
        "surface": "#fcfcfb", "ink": "#0b0b0b", "ink2": "#52514e", "muted": "#898781",
        "grid": "#e1e0d9", "axis": "#c3c2b7",
        "startup": "#52514e", "preamble": "#6250d6", "body": "#e87ba4", "pdf": "#eda100",
        "inside": {"pdf": "#0b0b0b"},
    },
    "dark": {
        "surface": "#1a1a19", "ink": "#f0efec", "ink2": "#c3c2b7", "muted": "#898781",
        "grid": "#2c2c2a", "axis": "#383835",
        "startup": "#c3c2b7", "preamble": "#9085e9", "body": "#d55181", "pdf": "#c98500",
        "inside": {"pdf": "#0b0b0b", "startup": "#0b0b0b"},
    },
}
ENGINES = ["pdflatex", "xelatex", "lualatex"]
NAME = {"pdflatex": "pdfLaTeX", "xelatex": "XeLaTeX", "lualatex": "LuaLaTeX"}
GROUPS = [
    ("startup", "startup", ["startup"]),
    ("preamble", "preamble", ["preamble"]),
    ("body", "body", ["begin_document", "typesetting", "end_document"]),
    ("pdf", "writing the PDF", ["shipout", "finalization"]),
]
EXPERIMENTS = [("preamble", "preamble", "precompiled preamble"), ("draft", "pdf", "draft mode"),
               ("externalize", "body", "externalized pictures")]
FONT = "system-ui, -apple-system, 'Segoe UI', sans-serif"
W = 860


def title(doc):
    """What a reader calls the document: the title in corpus.toml, else its key."""
    return CORPUS.get(doc, {}).get("title", doc)


def text_width(s, size=12):
    """Rough width of system-ui text; there are no font metrics here, so per-class widths."""
    w = 0.0
    for ch in s:
        if ch in "il.,:;|!'·":
            w += 0.28
        elif ch in "fjrt() -":
            w += 0.36
        elif ch.isdigit() or ch in "%×+−":
            w += 0.58
        elif ch.isupper() or ch in "mw":
            w += 0.74
        else:
            w += 0.55
    return w * size


def fmt_time(ms):
    if ms < 1000:
        return f"{ms:,.0f} ms"
    if ms >= 120_000:
        return f"{ms / 60_000:,.0f} min"
    return f"{ms / 1000:,.2f} s" if ms < 10_000 else f"{ms / 1000:,.1f} s"


def nice_ticks(hi, n=4):
    """Round tick values from 0 to at least hi."""
    raw = hi / n
    mag = 10 ** len(str(int(raw))) / 10 if raw >= 1 else 1
    step = next(s * mag for s in (1, 2, 2.5, 5, 10) if s * mag >= raw)
    ticks, v = [], 0.0
    while v < hi + step * 0.999:
        ticks.append(v)
        v += step
    return ticks


class Svg:
    def __init__(self, height, t, title, desc):
        self.t = t
        self.parts = [
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{height:.0f}" '
            f'viewBox="0 0 {W} {height:.0f}" font-family="{FONT}" role="img">',
            f"<title>{escape(title)}</title><desc>{escape(desc)}</desc>",
            f'<rect width="{W}" height="{height:.0f}" rx="8" fill="{t["surface"]}"/>',
        ]

    def add(self, s):
        self.parts.append(s)

    def text(self, x, y, s, size=12, fill="ink", anchor="start", weight=None):
        w = f' font-weight="{weight}"' if weight else ""
        nums = ' font-variant-numeric="tabular-nums"' if any(c.isdigit() for c in s) else ""
        self.add(f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" fill="{self.t.get(fill, fill)}" '
                 f'text-anchor="{anchor}"{w}{nums}>{escape(s)}</text>')

    def line(self, x1, y1, x2, y2, stroke, width=1, tip=None):
        t = f"<title>{escape(tip)}</title>" if tip else ""
        self.add(f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" '
                 f'stroke="{self.t.get(stroke, stroke)}" stroke-width="{width}" stroke-linecap="round">{t}</line>')

    def bar(self, x, y, w, h, fill, tip, round_end=True):
        """A bar from a baseline at x: square there, 4 px rounded at the data end."""
        if w <= 0:
            return
        r = min(4, w / 2, h / 2) if round_end else 0
        d = (f"M{x:.1f},{y:.1f} h{w - r:.1f} a{r},{r} 0 0 1 {r},{r} v{h - 2 * r:.1f} "
             f"a{r},{r} 0 0 1 -{r},{r} h-{w - r:.1f} z")
        self.add(f'<path d="{d}" fill="{self.t.get(fill, fill)}"><title>{escape(tip)}</title></path>')

    def dot(self, cx, cy, fill, tip, hollow=False, r=4):
        t = self.t
        if hollow:
            self.add(f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r - 1}" fill="{t["surface"]}" '
                     f'stroke="{t.get(fill, fill)}" stroke-width="2"><title>{escape(tip)}</title></circle>')
        else:
            self.add(f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r}" fill="{t.get(fill, fill)}" '
                     f'stroke="{t["surface"]}" stroke-width="1.5"><title>{escape(tip)}</title></circle>')

    def swatch(self, x, y, key, name):
        self.add(f'<rect x="{x:.1f}" y="{y - 9:.1f}" width="10" height="10" rx="2" fill="{self.t[key]}"/>')
        self.text(x + 15, y, name, fill="ink2")
        return x + 15 + text_width(name) + 18

    def header(self, title, subtitle):
        self.text(24, 34, title, size=17, weight="600")
        self.text(24, 56, subtitle, fill="ink2")

    def done(self):
        return "\n".join(self.parts + ["</svg>"]) + "\n"


def load(results_dir):
    def per_doc(sub):
        out = {}
        for doc in CORPUS:
            f = results_dir / sub / f"{doc}.json"
            if f.is_file():
                out[doc] = json.loads(f.read_text())
        return out

    f = results_dir / "results.json"
    return {
        "phases": json.loads(f.read_text()) if f.is_file() else {},
        "whatif": per_doc("whatif"),
        "overhead": per_doc("overhead"),
    }


def caption(data):
    """Machine, TeX Live and runs, for the foot of every chart."""
    p = data["phases"]
    m = p.get("machine") or next(iter(data["whatif"].values()), {}).get("machine", {})
    cpu = m.get("cpu", "?").replace("13th Gen Intel(R) Core(TM) ", "")
    fmt = next((p[d][e]["cold"]["format_date"] for d in CORPUS if d in p for e in ENGINES
                if "cold" in p[d].get(e, {})), None)
    runs = sorted({(CORPUS[d].get("runs") or p.get("runs", 5)) for d in CORPUS})
    by_runs = {}
    for d in CORPUS:
        if CORPUS[d].get("runs"):
            by_runs.setdefault(CORPUS[d]["runs"], []).append(title(d))
    fewer = "; ".join(f"{' and '.join(names)} {n}" for n, names in sorted(by_runs.items(), reverse=True))
    return (f"{cpu}, one P-core per container · TeX Live 2026{f', LaTeX {fmt}' if fmt else ''} · "
            f"warm runs: {max(runs)}{f'; {fewer}' if fewer else ''}")


def walls(entry):
    return [r["wall_ms"] for r in entry["warm"] if r["wall_ms"] is not None]


def doc_order(data):
    """Documents from the quickest build to the slowest (the first engine's median)."""
    p = data["phases"]

    def key(doc):
        for e in ENGINES:
            if "median" in p.get(doc, {}).get(e, {}):
                return p[doc][e]["median"]["wall"]
        return float("inf")

    return sorted((d for d in CORPUS if d in p or d in data["whatif"] or d in data["overhead"]), key=key)


def phases_chart(data, t):
    p = data["phases"]
    docs = [d for d in doc_order(data) if any("median" in p.get(d, {}).get(e, {}) for e in ENGINES)]
    pw = (W - 48 - 24) / 2
    head, row_h, axis_h, gap = 30, 22, 26, 22

    def panel_h(doc):
        n = sum("median" in p[doc].get(e, {}) for e in ENGINES)
        return head + n * row_h + axis_h

    top = 100
    heights = []
    for i in range(0, len(docs), 2):
        heights.append(max(panel_h(d) for d in docs[i:i + 2]))
    height = top + sum(heights) + gap * (len(heights) - 1) + 40
    svg = Svg(height, t, "Where the time goes",
              "One panel per benchmark document, each on its own time axis. For each engine a bar cut into "
              "startup, preamble, body and writing the PDF (medians of the warm runs), with every run's wall "
              "time as a dot.")
    svg.header("Where the time goes", "Median of the warm runs, cut into phases. The dots are each run's wall "
               "time. Each panel has its own axis.")
    x = 24
    for key, name, _ in GROUPS:
        x = svg.swatch(x, 84, key, name)
    svg.dot(x + 4, 80, "ink", "one run's wall time")
    svg.text(x + 14, 84, "one run", fill="ink2")
    y = top
    for i in range(0, len(docs), 2):
        for j, doc in enumerate(docs[i:i + 2]):
            px = 24 + j * (pw + 24)
            engines = [e for e in ENGINES if "median" in p[doc].get(e, {})]
            pages = p[doc][engines[0]]["cold"]["pages"]
            svg.text(px, y + 14, title(doc), size=13, weight="600")
            svg.text(px + text_width(title(doc), 13) * 1.2 + 10, y + 14, f"{pages:,} page{'s' if pages != 1 else ''}",
                     size=11, fill="muted")
            label_w, val_w = 70, 62
            bx, bw = px + label_w, pw - label_w - val_w
            hi = max(max(walls(p[doc][e])) for e in engines)
            ticks = nice_ticks(hi, 3)
            scale = bw / ticks[-1]
            y0, y1 = y + head - 4, y + head + len(engines) * row_h
            for tk in ticks:
                gx = bx + tk * scale
                svg.line(gx, y0, gx, y1, "axis" if tk == 0 else "grid")
                svg.text(gx, y1 + 14, fmt_time(tk) if tk else "0", size=10, fill="muted",
                         anchor="start" if tk == 0 else "middle")
            for k, e in enumerate(engines):
                m = p[doc][e]["median"]
                ry = y + head + k * row_h
                cy = ry + row_h / 2
                svg.text(bx - 8, cy + 4, NAME[e], fill="ink2", anchor="end")
                parts = [(key, name, sum(m[ph] or 0 for ph in phs)) for key, name, phs in GROUPS]
                total = sum(v for *_, v in parts)
                cx = bx
                shown = [(key, name, v) for key, name, v in parts if v * scale >= 1]
                for n, (key, name, v) in enumerate(shown):
                    w = v * scale - (2 if n < len(shown) - 1 else 0)
                    svg.bar(cx, ry + 4, max(w, 1), row_h - 8, key,
                            f"{doc}, {NAME[e]}: {name} {v:,.1f} ms ({v / total * 100:.0f}%)",
                            round_end=n == len(shown) - 1)
                    pct = f"{v / total * 100:.0f}%"
                    # The last segment ends where the run dots are: leave them room.
                    room = 8 if n < len(shown) - 1 else 22
                    if w >= text_width(pct, 10) + room:
                        svg.text(cx + 4, cy + 3.5, pct, size=10, fill=t["inside"].get(key, "#ffffff"), weight="600")
                    cx += w + 2
                for wv in walls(p[doc][e]):
                    svg.dot(bx + wv * scale, cy, "ink", f"{doc}, {NAME[e]}: a run of {fmt_time(wv)}", r=3)
                svg.text(px + pw, cy + 4, fmt_time(m["wall"]), fill="ink2", anchor="end")
        y += heights[i // 2] + gap
    svg.text(24, height - 16, caption(data), size=11, fill="muted")
    return svg.done()


def grouped_rows(data, source):
    """(document or "", engine, entry) rows, the document named on its first row only."""
    rows = []
    for doc in doc_order(data):
        entry = data[source].get(doc)
        if not entry:
            continue
        engines = [e for e in ENGINES if e in entry and "failed" not in entry[e]]
        for i, e in enumerate(engines):
            rows.append((title(doc) if i == 0 else "", e, entry[e], i == 0))
    return rows


def overhead_chart(data, t):
    rows = grouped_rows(data, "overhead")
    if not rows:
        return None
    label_w, row_h, gap, top = 250, 22, 10, 100
    rel = []
    for *_, r, _ in rows:
        base = statistics.median(r["plain_ms"])
        rel += [v / base * 100 - 100 for v in r["plain_ms"] + r["probed_ms"]]
    lo, hi = min(min(rel), -1), max(max(rel), 1)
    step = next(s for s in (1, 2, 5, 10, 20, 50) if (hi - lo) / s <= 8)
    lo, hi = step * (lo // step), step * -(-hi // step)
    plot_x, plot_w = label_w, W - label_w - 150
    xs = lambda v: plot_x + (v - lo) / (hi - lo) * plot_w  # noqa: E731
    height = top + sum(row_h + (gap if first else 0) for *_, first in rows) + 64
    svg = Svg(height, t, "What the probe costs",
              "For each benchmark document and engine, every build without prumo and every build with it "
              "(the probe and -recorder), as a difference from the median build without it.")
    svg.header("What the probe costs", "Builds with the probe and -recorder against builds without, in turns. "
               "0 is the median without, the tick the median with.")
    x = 24
    svg.dot(x + 4, 80, "ink2", "a build without prumo", hollow=True)
    svg.text(x + 14, 84, "without prumo", fill="ink2")
    x += 14 + text_width("without prumo") + 22
    svg.dot(x + 4, 80, "ink", "a build with prumo")
    svg.text(x + 14, 84, "with prumo", fill="ink2")
    y0 = top
    y = top
    for doc, e, r, first in rows:
        if first:
            y += gap
        y += row_h
    y1 = y
    v = lo
    while v <= hi + 1e-9:
        svg.line(xs(v), y0 + gap - 4, xs(v), y1, "axis" if v == 0 else "grid")
        svg.text(xs(v), y1 + 16, f"{v:+.0f}%" if v else "0", size=10, fill="muted", anchor="middle")
        v += step
    y = top
    for doc, e, r, first in rows:
        if first:
            y += gap
            if y > top + gap:
                svg.line(24, y - gap / 2, label_w - 8, y - gap / 2, "grid")
        cy = y + row_h / 2
        if doc:
            svg.text(24, cy + 4, doc, weight="600")
        svg.text(label_w - 12, cy + 4, NAME[e], fill="ink2", anchor="end")
        base = statistics.median(r["plain_ms"])
        for val in r["plain_ms"]:
            svg.dot(xs(val / base * 100 - 100), cy, "ink2", f"{doc}, {NAME[e]}: {fmt_time(val)} without prumo",
                    hollow=True)
        for val in r["probed_ms"]:
            svg.dot(xs(val / base * 100 - 100), cy, "ink", f"{doc}, {NAME[e]}: {fmt_time(val)} with prumo")
        med = statistics.median(r["probed_ms"])
        d = med - base
        svg.line(xs(d / base * 100), y + 3, xs(d / base * 100), y + row_h - 3, "ink", 2,
                 f"{doc}, {NAME[e]}: median with prumo {fmt_time(med)}, without {fmt_time(base)}")
        svg.text(plot_x + plot_w + 14, cy + 4, f"{d:+,.0f} ms ({d / base * 100:+.1f}%)", fill="ink2")
        y += row_h
    svg.text(plot_x + plot_w + 14, top + 2, "median cost", size=11, fill="muted")
    svg.text(24, height - 16, caption(data), size=11, fill="muted")
    return svg.done()


def packages_chart(data, t, doc="standard", a="pdflatex", b="lualatex", n=12):
    entry = data["overhead"].get(doc, {})
    if a not in entry or b not in entry or "units" not in entry[a]:
        return None, None
    med = {e: {k: statistics.median(v) for k, v in entry[e]["units"].items() if k.split()[0] in ("pkg", "cls")}
           for e in (a, b)}
    keys = set(med[a]) | set(med[b])
    rows = sorted(keys, key=lambda k: -max(med[a].get(k, 0), med[b].get(k, 0)))[:n]
    rows.sort(key=lambda k: -(med[b].get(k, 0) - med[a].get(k, 0)))
    label_w, row_h, top = 150, 24, 100
    hi = max(max(med[a].get(k, 0), med[b].get(k, 0)) for k in rows)
    ticks = nice_ticks(hi, 5)
    plot_w = W - label_w - 140
    xs = lambda v: label_w + v / ticks[-1] * plot_w  # noqa: E731
    pre = data["phases"].get(doc, {})
    pa = pre.get(a, {}).get("median", {}).get("preamble")
    pb = pre.get(b, {}).get("median", {}).get("preamble")
    height = top + len(rows) * row_h + 66
    svg = Svg(height, t, f"Package load times on {NAME[a]} and {NAME[b]}",
              f"The {doc} document's slowest packages and classes, exclusive time on {NAME[a]} (hollow) and "
              f"{NAME[b]} (filled), sorted by how much longer {NAME[b]} takes.")
    sub = f"The paper ({doc}): each package's own time, without the packages it loads, median of the runs."
    svg.header(f"Package load times, {NAME[a]} against {NAME[b]}", sub)
    x = 24
    svg.dot(x + 4, 80, "preamble", NAME[a], hollow=True)
    svg.text(x + 14, 84, NAME[a], fill="ink2")
    x += 14 + text_width(NAME[a]) + 22
    svg.dot(x + 4, 80, "preamble", NAME[b])
    svg.text(x + 14, 84, NAME[b], fill="ink2")
    if pa and pb:
        svg.text(x + 14 + text_width(NAME[b]) + 22, 84,
                 f"whole preamble: {fmt_time(pa)} and {fmt_time(pb)}", fill="muted")
    y1 = top + len(rows) * row_h
    for tk in ticks:
        svg.line(xs(tk), top, xs(tk), y1, "axis" if tk == 0 else "grid")
        svg.text(xs(tk), y1 + 16, f"{tk:g} ms", size=10, fill="muted", anchor="middle")
    svg.text(label_w + plot_w + 18, top - 6, "difference", size=11, fill="muted")
    table = [f"| package | {NAME[a]} (ms) | {NAME[b]} (ms) | difference |", "|---|---:|---:|---:|"]
    for i, k in enumerate(rows):
        kind, name = k.split(" ", 1)
        cy = top + i * row_h + row_h / 2
        svg.text(label_w - 12, cy + 4, name + (" (class)" if kind == "cls" else ""), anchor="end")
        va, vb = med[a].get(k), med[b].get(k)
        if va is not None and vb is not None:
            svg.line(xs(min(va, vb)), cy, xs(max(va, vb)), cy, "axis", 2)
            svg.dot(xs(va), cy, "preamble", f"{name}: {va:,.1f} ms on {NAME[a]}", hollow=True)
            svg.dot(xs(vb), cy, "preamble", f"{name}: {vb:,.1f} ms on {NAME[b]}")
            svg.text(label_w + plot_w + 18, cy + 4, f"{vb - va:+,.0f} ms", fill="ink2")
        elif vb is not None:
            svg.dot(xs(vb), cy, "preamble", f"{name}: {vb:,.1f} ms, {NAME[b]} only")
            svg.text(label_w + plot_w + 18, cy + 4, f"{NAME[b]} only", fill="muted")
        else:
            svg.dot(xs(va), cy, "preamble", f"{name}: {va:,.1f} ms, {NAME[a]} only", hollow=True)
            svg.text(label_w + plot_w + 18, cy + 4, f"{NAME[a]} only", fill="muted")
        fa = "–" if va is None else f"{va:,.1f}"
        fb = "–" if vb is None else f"{vb:,.1f}"
        diff = "" if va is None or vb is None else f"{vb - va:+,.1f}"
        table.append(f"| {name}{' (class)' if kind == 'cls' else ''} | {fa} | {fb} | {diff} |")
    svg.text(24, height - 16, caption(data), size=11, fill="muted")
    return svg.done(), "\n".join(table) + "\n"


def whatif_chart(data, t):
    rows = [(d, e, r["report"], f) for d, e, r, f in grouped_rows(data, "whatif") if "report" in r]
    if not rows:
        return None
    label_w, gap, row_h, top = 250, 10, 22, 104
    panel_gap = 16
    panel_w = (W - label_w - 24 - 2 * panel_gap) / 3
    plot_w = panel_w - 70
    height = top + sum(row_h + (gap if first else 0) for *_, first in rows) + 88
    svg = Svg(height, t, "What prumo whatif saves",
              "For each benchmark document and engine, the share of the build each whatif experiment saved, "
              "with the probe's bound for it and the one-time cost.")
    svg.header("What prumo whatif saves", "Share of the build saved (medians). The tick is the probe's bound: the "
               "most the change can save.")
    y_end = top + sum(row_h + (gap if first else 0) for *_, first in rows)
    for j, (name, key, title) in enumerate(EXPERIMENTS):
        px = label_w + j * (panel_w + panel_gap)
        svg.swatch(px, 84, key, title)
        for pct in (0, 50, 100):
            gx = px + pct / 100 * plot_w
            svg.line(gx, top + gap - 4, gx, y_end, "axis" if pct == 0 else "grid")
            svg.text(gx, y_end + 16, f"{pct}%", size=10, fill="muted", anchor="start" if pct == 0 else "middle")
    y = top
    for doc, e, r, first in rows:
        if first:
            y += gap
            if y > top + gap:
                svg.line(24, y - gap / 2, label_w - 8, y - gap / 2, "grid")
        cy = y + row_h / 2
        if doc:
            svg.text(24, cy + 4, doc, weight="600")
        svg.text(label_w - 12, cy + 4, NAME[e], fill="ink2", anchor="end")
        base = r["baseline"]["median_ms"]
        by = {x["experiment"]: x for x in r["experiments"]}
        for j, (name, key, title) in enumerate(EXPERIMENTS):
            px = label_w + j * (panel_w + panel_gap)
            row = by.get(name)
            if not row:
                continue
            label = f"{doc or ''} {NAME[e]}, {title}".strip()
            right = px
            if row.get("bound_ms") is not None and row["status"] == "measured" and base > 0:
                bx = px + min(row["bound_ms"] / base, 1) * plot_w
                svg.line(bx, y + 4, bx, y + row_h - 4, "ink2", 2, f"{label}: bound {fmt_time(row['bound_ms'])}")
                right = bx
            if row["status"] != "measured":
                note = "–" if row["status"] == "not-run" else "fails"
                svg.text(max(px + 4, right + 5), cy + 4, note, size=11, fill="muted")
                continue
            noise = row["within_noise"]
            if row["saved_ms"] > 0:
                w = min(row["saved_pct"], 100) / 100 * plot_w
                changed = row["output"]["status"] == "changed"
                tip = (f"{label}: saved {fmt_time(row['saved_ms'])} of {fmt_time(base)} ({row['saved_pct']:.0f}%)"
                       + (", within noise" if noise else "")
                       + (f"; output changed: {row['output']['evidence']}" if changed else ""))
                svg.bar(px, y + 6, max(w, 2), row_h - 12, "muted" if noise else key, tip)
                right = max(right, px + max(w, 2))
            if noise:
                svg.text(right + 5, cy + 4, "noise", size=11, fill="muted")
                continue
            if row["saved_ms"] < 0:
                svg.text(right + 5, cy + 4, f"{fmt_time(-row['saved_ms'])} slower", size=11, fill="ink2")
                continue
            lab = f"{row['saved_pct']:.0f}%" + (" ≠" if row["output"]["status"] == "changed" else "")
            once = f" · once {fmt_time(row['one_time_ms'])}" if row.get("one_time_ms") else ""
            room = px + panel_w - (right + 5)
            if once and text_width(lab + once, 11) <= room:
                lab += once
            svg.text(right + 5, cy + 4, lab, size=11, fill="ink2")
        y += row_h
    svg.text(24, height - 54, "gray: within noise, no larger than the runs' spread. fails: the variant did not "
             "build. –: not tried here. ≠: the output changed.", size=11, fill="muted")
    svg.text(24, height - 38, "once: paid on the first build only, building the format or every picture.",
             size=11, fill="muted")
    svg.text(24, height - 16, caption(data), size=11, fill="muted")
    return svg.done()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--results", type=Path, default=HERE / "results")
    ap.add_argument("--out", type=Path, default=HERE / "img")
    a = ap.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    data = load(a.results)
    for mode, t in THEMES.items():
        charts = {"phases": phases_chart(data, t) if data["phases"] else None,
                  "overhead": overhead_chart(data, t), "whatif": whatif_chart(data, t)}
        charts["packages"], table = packages_chart(data, t)
        for name, svg in charts.items():
            if svg:
                (a.out / f"{name}-{mode}.svg").write_text(svg)
        if table:
            (a.results / "packages.md").write_text(table)
    print(f"charts in {a.out}")


if __name__ == "__main__":
    main()
