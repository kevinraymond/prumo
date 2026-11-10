#!/usr/bin/env python3
"""Benchmarks: prumo's phase table for four documents on three engines.

Runs inside a TeX Live image with prumo on PATH and the documents under /bench/docs (run.sh
sets that up). For each document and engine:

  1. Copy the document to a scratch directory and build it there with latexmk, so the .aux,
     .toc and .bbl exist. prumo copies the directory it is given, so every timed run is a
     rerun with those files present, the way an editor rebuilds.
  2. One cold run: a fresh HOME, so no luaotfload font name database and no fontconfig
     cache. The operating system's file cache is not dropped (that needs root on the host).
  3. --runs warm runs with that HOME.

Every timed run is `prumo time --rebuild --json`. The phases come from prumo, the wall time
is prumo's own measure of the engine process. Writes results.json (every run, plus the
medians) and results.md (the tables) to --out.

Usage: bench.py [--runs 5] [--docs minimal,standard,florilegium,large]
                [--engines pdflatex,xelatex,lualatex] [--out results]
"""
import argparse
import json
import os
import platform
import shutil
import statistics
import subprocess
import time
from pathlib import Path

DOCS = {
    # name: (directory under /bench/docs, main file, engines it supports, warm runs or None)
    "minimal": ("minimal", "main.tex", None, None),
    "standard": ("standard", "main.tex", None, None),
    "florilegium": ("florilegium", "florilegium-doc.tex", ["lualatex"], None),
    "large": ("large", "main.tex", None, 3),
}
PHASES = ["startup", "preamble", "begin_document", "typesetting", "shipout", "end_document", "finalization"]
LATEXMK_FLAG = {"pdflatex": "-pdf", "xelatex": "-xelatex", "lualatex": "-lualatex"}


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def version_line(cmd):
    try:
        return sh(cmd).stdout.splitlines()[0].strip()
    except (OSError, IndexError):
        return "not found"


def machine():
    cpu = next((line.split(":", 1)[1].strip() for line in open("/proc/cpuinfo") if line.startswith("model name")), "?")
    return {
        "cpu": cpu,
        "cpus_allowed": len(os.sched_getaffinity(0)),
        "kernel": platform.release(),
        "image": os.environ.get("BENCH_IMAGE", "?"),
        "image_id": os.environ.get("BENCH_IMAGE_ID", "?"),
        "load_average_at_start": open("/proc/loadavg").read().split()[:3],
        "pdftex": version_line(["pdftex", "--version"]),
        "xetex": version_line(["xetex", "--version"]),
        "luahbtex": version_line(["luahbtex", "--version"]),
        "prumo": version_line(["prumo", "--version"]),
        "florilegium_commit": os.environ.get("FLORILEGIUM_COMMIT", "?"),
    }


def prepare(doc, engine, scratch):
    """Copy the document and build it once with latexmk so the aux files exist."""
    src = Path("/bench/docs") / DOCS[doc][0]
    if scratch.exists():
        shutil.rmtree(scratch)
    shutil.copytree(src, scratch)
    env = dict(os.environ, HOME=str(scratch.parent / f"home-prep-{doc}-{engine}"))
    main = DOCS[doc][1]
    r = sh(["latexmk", LATEXMK_FLAG[engine], "-interaction=nonstopmode", main], cwd=scratch, env=env)
    pdf = scratch / main.replace(".tex", ".pdf")
    return r.returncode == 0 and pdf.is_file(), r.stdout[-2000:] + r.stderr[-2000:]


def timed_run(scratch, main, engine, home, out):
    """One `prumo time --rebuild --json`; the parsed JSON plus prumo's own elapsed time."""
    env = dict(os.environ, HOME=str(home), PRUMO_CACHE=str(out))
    t0 = time.monotonic()
    r = sh(["prumo", "time", "--rebuild", "--json", "--engine", engine, str(scratch / main)], env=env)
    elapsed = time.monotonic() - t0
    if r.returncode != 0:
        raise RuntimeError(f"prumo failed ({r.returncode}): {r.stderr[-1500:]}")
    j = json.loads(r.stdout)
    return j, elapsed


def summary(j, elapsed):
    p = j["phases"]
    return {
        "phases_ms": {k: p[f"{k}_ms"] for k in PHASES},
        "wall_ms": p["wall_ms"],
        "last_record_ms": p["last_record_ms"],
        "pages": len(j["pages"]),
        "environments": j["environments"],
        "warnings": j["warnings"],
        "timer": j["timer"],
        "format_date": j["format_date"],
        "prumo_elapsed_s": round(elapsed, 3),
    }


def scale(scratch, main, engine, home, out):
    """For the large document: records, the time prumo itself takes, and the HTML page."""
    env = dict(os.environ, HOME=str(home), PRUMO_CACHE=str(out))
    where = sh(["prumo", "where", str(scratch / main)], env=env).stdout.split()[0]
    records = Path(where) / main.replace(".tex", ".prumo")
    t0 = time.monotonic()
    r = sh(["prumo", "time", "--json", "--engine", engine, str(scratch / main)], env=env)
    analyze = time.monotonic() - t0
    if r.returncode != 0 or "cached run" not in r.stderr and r.stderr:
        raise RuntimeError(f"prumo time --json on the cached run: {r.returncode} {r.stderr[-500:]}")
    html = out / f"{engine}.html"
    t0 = time.monotonic()
    r = sh(["prumo", "time", "--engine", engine, "--html", str(html), str(scratch / main)], env=env)
    html_s = time.monotonic() - t0
    if r.returncode != 0:
        raise RuntimeError(f"prumo time --html: {r.returncode} {r.stderr[-500:]}")
    return {
        "record_lines": sum(1 for _ in open(records)),
        "record_bytes": records.stat().st_size,
        "analyze_cached_ms": round(analyze * 1000, 1),
        "html_ms": round(html_s * 1000, 1),
        "html_bytes": html.stat().st_size if html.is_file() else None,
    }


def median_of(runs):
    out = {}
    for k in PHASES:
        vals = [r["phases_ms"][k] for r in runs if r["phases_ms"][k] is not None]
        out[k] = round(statistics.median(vals), 1) if vals else None
    walls = [r["wall_ms"] for r in runs if r["wall_ms"] is not None]
    out["wall"] = round(statistics.median(walls), 1) if walls else None
    out["wall_min"] = round(min(walls), 1) if walls else None
    out["wall_max"] = round(max(walls), 1) if walls else None
    return out


def table(results, doc, engines):
    """One markdown table: phases down, engines across, warm medians, and the cold wall."""
    cols = [e for e in engines if e in results[doc] and "median" in results[doc][e]]
    if not cols:
        return ""
    lines = ["| phase (ms) | " + " | ".join(cols) + " |", "|---|" + "---:|" * len(cols)]
    for k in PHASES:
        row = [results[doc][e]["median"][k] for e in cols]
        lines.append(f"| {k.replace('_', '-')} | " + " | ".join("–" if v is None else f"{v:,.1f}" for v in row) + " |")
    lines.append("| **total (wall)** | " + " | ".join(f"**{results[doc][e]['median']['wall']:,.1f}**" for e in cols) + " |")
    lines.append(
        "| spread (min–max) | "
        + " | ".join(f"{results[doc][e]['median']['wall_min']:,.0f}–{results[doc][e]['median']['wall_max']:,.0f}" for e in cols)
        + " |"
    )
    lines.append("| cold run (wall) | " + " | ".join(f"{results[doc][e]['cold']['wall_ms']:,.1f}" for e in cols) + " |")
    lines.append("| pages | " + " | ".join(str(results[doc][e]["cold"]["pages"]) for e in cols) + " |")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--docs", default="minimal,standard,florilegium,large")
    ap.add_argument("--engines", default="pdflatex,xelatex,lualatex")
    ap.add_argument("--out", default="results")
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    engines = a.engines.split(",")
    work = Path("/tmp/bench")
    results = {"machine": machine(), "runs": a.runs}
    for doc in a.docs.split(","):
        _, main_file, supported, runs_override = DOCS[doc]
        runs = runs_override or a.runs
        results[doc] = {}
        for engine in engines:
            if supported and engine not in supported:
                results[doc][engine] = {"skipped": f"the document needs {', '.join(supported)}"}
                continue
            print(f"== {doc} on {engine}", flush=True)
            scratch = work / f"{doc}-{engine}"
            ok, log = prepare(doc, engine, scratch)
            if not ok:
                results[doc][engine] = {"failed": "latexmk", "log": log}
                print(f"   latexmk failed:\n{log[-800:]}", flush=True)
                continue
            home = work / f"home-{doc}-{engine}"
            cache = work / f"cache-{doc}-{engine}"
            for d in (home, cache):
                shutil.rmtree(d, ignore_errors=True)
                d.mkdir(parents=True)
            cold = summary(*timed_run(scratch, main_file, engine, home, cache))
            print(f"   cold  {cold['wall_ms']:9.1f} ms wall, {cold['pages']} pages", flush=True)
            warm = []
            for i in range(runs):
                s = summary(*timed_run(scratch, main_file, engine, home, cache))
                warm.append(s)
                print(f"   warm {i + 1} {s['wall_ms']:9.1f} ms wall", flush=True)
            entry = {"cold": cold, "warm": warm, "median": median_of(warm)}
            if doc == "large":
                entry["scale"] = scale(scratch, main_file, engine, home, cache)
                if (cache / f"{engine}.html").is_file():
                    shutil.move(str(cache / f"{engine}.html"), str(out / f"large-{engine}.html"))
            results[doc][engine] = entry
            (out / "results.json").write_text(json.dumps(results, indent=1))
    md = [f"Machine: {results['machine']['cpu']}, pinned to {results['machine']['cpus_allowed']} CPU; "
          f"{results['machine']['pdftex']}; image {results['machine']['image']}. "
          f"Warm medians of {a.runs} runs (3 for the large document); the cold run has a fresh HOME.\n"]
    for doc in a.docs.split(","):
        md.append(f"\n### {doc}\n\n" + table(results, doc, engines))
        if doc == "large":
            for e in engines:
                sc = results[doc].get(e, {}).get("scale")
                if sc:
                    md.append(
                        f"\n{e}: {sc['record_lines']:,} records ({sc['record_bytes'] / 1024:,.0f} KiB); "
                        f"`prumo time --json` on the cached run {sc['analyze_cached_ms']:,.0f} ms; "
                        f"`--html` {sc['html_ms']:,.0f} ms, {sc['html_bytes'] / 1024:,.0f} KiB.\n"
                    )
    (out / "results.md").write_text("".join(md))
    (out / "results.json").write_text(json.dumps(results, indent=1))
    print("".join(md))


if __name__ == "__main__":
    main()
