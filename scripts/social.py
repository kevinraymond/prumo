#!/usr/bin/env python3
"""Write the TikZ source of the repository's social image from a prumo record.

The image is a plumb line hanging through the preamble's flame chart. Every bar comes from
the record: one row per nesting depth, a package and its own .sty folded into one bar (a
class and its .cls too), as in `prumo time --html`. The line drops through the deepest
point of the stack unless --at names a time.

Usage:  social.py RECORD.prumo OUT.tex [--light] [--at MS]

Coordinates are bp on a 1280 x 640 canvas, so `pdftoppm -r 72` gives the 1280 x 640 px
GitHub asks for. scripts/social.sh runs this, LuaLaTeX and pdftoppm.
"""
import sys

SCALE = 65536.0  # record times are scaled seconds
W, H = 1280, 640
CHART_L, CHART_R = 80, 1200
ROW_TOP, ROW_H, ROW_GAP = 378, 22, 2.5
MONO_W = 0.6 * 10.5  # DejaVu Sans Mono's advance at 10.5 bp

# The colors of prumo's HTML report (src/html.rs). `ink` is the text on a bar.
THEMES = {
    "dark": {"bg": "151719", "fg": "E6E3DC", "ink": "151719", "mute": "9AA0A6",
             "line": "2D3136", "cls": "7FA0D4", "pkg": "7CC488", "file": "E0BB5A",
             "brass": "D2A04A", "brassdark": "8C6828"},
    "light": {"bg": "FBFAF7", "fg": "1F2328", "ink": "1F2328", "mute": "6B7280",
              "line": "E2DFD8", "cls": "5B7DB1", "pkg": "5FA36B", "file": "D0A540",
              "brass": "B8893A", "brassdark": "7A5A22"},
}


def read_record(path):
    """The cls/pkg/file spans as a tree, and the start and begindoc times."""
    root = {"children": []}
    stack = [root]
    marks = {}
    for line in open(path, encoding="utf-8"):
        parts = line.rstrip("\n").split(" ", 3)
        if len(parts) < 3:
            continue
        kind, t = parts[0], int(parts[1])
        name = parts[3] if len(parts) > 3 else ""
        if kind in ("start", "begindoc"):
            marks[kind] = t
        base, sign = kind[:-1], kind[-1:]
        if base not in ("cls", "pkg", "file") or sign not in "+-":
            continue
        if sign == "+":
            node = {"kind": base, "name": name, "start": t, "end": None, "children": []}
            stack[-1]["children"].append(node)
            stack.append(node)
            continue
        while len(stack) > 1:
            node = stack.pop()
            node["end"] = t
            if node["kind"] == base and node["name"] == name:
                break
    return root["children"], marks["start"], marks["begindoc"]


def fold(node):
    """A package and its own .sty are one bar, and so are a class and its .cls."""
    if node["kind"] in ("pkg", "cls"):
        own = node["name"] + (".sty" if node["kind"] == "pkg" else ".cls")
        kids = []
        for k in node["children"]:
            kids.extend(k["children"] if k["kind"] == "file" and k["name"] == own else [k])
        node["children"] = kids
    for k in node["children"]:
        fold(k)


def bars(nodes, depth, t0, t1, out):
    """(depth, kind, name, start, end) for every span that starts in the preamble."""
    for n in nodes:
        if n["start"] >= t1:
            continue
        end = t1 if n["end"] is None else min(n["end"], t1)
        out.append((depth, n["kind"], n["name"], n["start"] - t0, end - t0))
        bars(n["children"], depth + 1, t0, t1, out)


def fit(name, width):
    """The name, or the name cut short with an ellipsis, in a bar this wide."""
    room = int((width - 8) / MONO_W)
    if room >= len(name):
        return name
    return name[: room - 1] + "…" if room >= 5 else None


def place(name, x0, x1, px):
    """A label starts at the bar's left edge. If the line would cut it, it moves to the
    right of the line, or stops short of it."""
    text = fit(name, x1 - x0)
    if not text:
        return None, None
    lx = x0 + 4
    if not lx - 2 <= px <= lx + len(text) * MONO_W + 2:
        return text, lx
    after = fit(name, x1 - (px + 2))
    if after == name:
        return name, px + 6
    before = fit(name, px - 2 - x0)
    if before:
        return before, lx
    return (after, px + 6) if after else (None, None)


def tex(s):
    for a, b in (("\\", r"\textbackslash{}"), ("_", r"\_"), ("&", r"\&"), ("#", r"\#"),
                 ("%", r"\%"), ("$", r"\$")):
        s = s.replace(a, b)
    return s


def main():
    args = sys.argv[1:]
    theme = THEMES["light" if "--light" in args else "dark"]
    args = [a for a in args if a != "--light"]
    at_ms = None
    if "--at" in args:
        i = args.index("--at")
        at_ms = float(args[i + 1])
        del args[i : i + 2]
    if len(args) != 2:
        sys.exit(__doc__)
    record, out_path = args

    roots, t0, t1 = read_record(record)
    for r in roots:
        fold(r)
    rows = []
    bars(roots, 0, t0, t1, rows)
    span = t1 - t0
    units = sum(1 for _, kind, *_ in rows if kind in ("pkg", "cls"))
    deepest = max(d for d, *_ in rows)

    def x(t):
        return CHART_L + (CHART_R - CHART_L) * t / span

    def top(depth):
        return ROW_TOP - depth * (ROW_H + ROW_GAP)

    if at_ms is None:
        first = next(r for r in rows if r[0] == deepest)
        at = (first[3] + first[4]) / 2
    else:
        at = at_ms / 1000 * SCALE
    px = round(x(at), 1)
    hit = max((d for d, _k, _n, s, e in rows if s <= at <= e), default=0)
    bob = top(hit) - ROW_H - 14

    out = [r"\documentclass{standalone}", r"\usepackage{fontspec}", r"\usepackage{tikz}",
           r"\setmainfont{LibertinusSerif}[Extension=.otf,UprightFont=*-Regular,BoldFont=*-Semibold]",
           r"\newfontfamily\mono{DejaVuSansMono}[Extension=.ttf]"]
    out += [rf"\definecolor{{{k}}}{{HTML}}{{{v}}}" for k, v in theme.items()]
    out += [r"\begin{document}", r"\begin{tikzpicture}[x=1bp,y=1bp]",
            rf"\fill[bg] (0,0) rectangle ({W},{H});",
            rf"\useasboundingbox (0,0) rectangle ({W},{H});",
            rf"\draw[line, line width=1bp] (22,22) rectangle ({W - 22},{H - 22});",
            rf"\draw[line, line width=0.6bp] (28,28) rectangle ({W - 28},{H - 28});",
            rf"\node[anchor=base west, inner sep=0, text=fg] at ({CHART_L - 7},486) "
            r"{\fontsize{148}{148}\selectfont\bfseries prumo};",
            rf"\node[anchor=base west, inner sep=0, text=mute] at ({CHART_L},414) "
            r"{\fontsize{25}{25}\selectfont\addfontfeature{LetterSpace=8}\scshape where a "
            r"{\addfontfeature{LetterSpace=0}\upshape\fontsize{21}{21}\selectfont\LaTeX} "
            r"build spends its time};"]
    for depth, kind, name, s, e in rows:
        x0, x1 = x(s), x(e)
        if x1 - x0 < 0.6:
            continue
        y = top(depth)
        out.append(rf"\fill[{kind}] ({x0:.2f},{y - ROW_H:.2f}) rectangle "
                   rf"({max(x1 - 0.8, x0 + 0.6):.2f},{y:.2f});")
        text, lx = place(name, x0, x1, px)
        if text:
            out.append(rf"\node[anchor=base west, inner sep=0, text=ink] at ({lx:.2f},{y - ROW_H + 7.5:.2f}) "
                       rf"{{\mono\fontsize{{10.5}}{{10.5}}\selectfont {tex(text)}}};")
    out += [rf"\fill[fg] ({px},{H - 28}) circle (2.2bp);",
            rf"\draw[fg, line width=0.9bp] ({px},{H - 28}) -- ({px},{bob});",
            rf"\begin{{scope}}[shift={{({px},{bob})}}, scale=1.35]",
            r"  \fill[brassdark] (-4,2) rectangle (4,-5);",
            r"  \fill[brass] (-12,-5) -- (12,-5) .. controls (14,-9) and (14,-15) .. (12,-19)"
            r" -- (0,-52) -- (-12,-19) .. controls (-14,-15) and (-14,-9) .. (-12,-5) -- cycle;",
            r"  \fill[brassdark] (-12,-19) -- (12,-19) -- (0,-52) -- cycle;",
            r"  \fill[brass!70!white] (-8,-7) .. controls (-9,-10) and (-9,-14) .. (-8,-17)"
            r" -- (-6,-17) .. controls (-7,-14) and (-7,-10) .. (-6,-7) -- cycle;",
            r"\end{scope}",
            rf"\node[anchor=base west, inner sep=0, text=mute] at ({px + 8},{top(0) + 10}) "
            rf"{{\mono\fontsize{{10.5}}{{10.5}}\selectfont {at / SCALE * 1000:.0f} ms}};",
            rf"\node[anchor=base east, inner sep=0, text=mute] at ({CHART_R},52) "
            rf"{{\mono\fontsize{{10.5}}{{10.5}}\selectfont florilegium's manual, LuaLaTeX: {units} classes "
            rf"and packages, {span / SCALE * 1000:.0f} ms to \textbackslash begin\{{document\}}}};",
            r"\end{tikzpicture}", r"\end{document}"]
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")
    print(f"{len(rows)} bars to depth {deepest}, {units} classes and packages, "
          f"{span / SCALE * 1000:.1f} ms; the line at {at / SCALE * 1000:.1f} ms reaches depth {hit}")


if __name__ == "__main__":
    main()
