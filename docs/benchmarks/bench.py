#!/usr/bin/env python3
"""Benchmarks: prumo's phase table, and `prumo whatif`, for the documents in corpus.toml.

Runs inside a TeX Live image with prumo on PATH and the documents under /bench/docs (run.sh
lays them out with fetch.py). For each document and engine:

  1. Copy the document to a scratch directory, run its `prepare` commands, and build it
     there with latexmk, so the .aux, .toc and .bbl exist. prumo copies the directory it is
     given, so every timed run is a rerun with those files present, the way an editor
     rebuilds.
  2. One cold run: a fresh HOME, so no luaotfload font name database and no fontconfig
     cache. The operating system's file cache is not dropped (that needs root on the host).
  3. The document's `runs` warm runs (--runs by default) with that HOME.

Every timed run is `prumo time --rebuild --json`. The phases come from prumo, the wall time
is prumo's own measure of the engine process. Writes results.json (every run, plus the
medians) and results.md (the tables) to --out.

With --overhead, step 1 is followed by --runs rounds of two builds in turns, after a
warm-up of each: the plain engine command, and the same with what prumo adds (-recorder,
-jobname and the probe, from /probe). Both are timed here the same way, spawn to exit, so the
difference is the probe's cost. Each probed run's records are read with `prumo time --json
<job>.prumo` for the per-package times. Results go to overhead/<document>.json.

With --whatif, step 1 is followed by `prumo whatif --json --shell-escape` with the same
number of runs, in the HOME latexmk warmed. Each document's reports go to
whatif/<document>.json under --out, so several containers can run at once on different
documents, and whatif.md is written from every file there. --report rewrites the two
markdown files from the JSON without building anything.

Usage: bench.py [--runs 5] [--docs minimal,standard,...] [--engines pdflatex,xelatex,lualatex]
                [--whatif | --overhead | --report] [--out results]
"""
import argparse
import json
import os
import platform
import shutil
import statistics
import subprocess
import time
import tomllib
from pathlib import Path

DOCS = tomllib.loads((Path(__file__).resolve().parent / "corpus.toml").read_text())
ENGINES = ["pdflatex", "xelatex", "lualatex"]


def title(doc):
    """What the tables call a document: its title in corpus.toml."""
    return DOCS[doc].get("title", doc)
PHASES = ["startup", "preamble", "begin_document", "typesetting", "shipout", "end_document", "finalization"]
LATEXMK_FLAG = {"pdflatex": "-pdf", "xelatex": "-xelatex", "lualatex": "-lualatex"}


def sh(cmd, **kw):
    # TeX output is not always UTF-8 (a log quotes the bytes a file had).
    return subprocess.run(cmd, capture_output=True, text=True, errors="replace", **kw)


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
        "commits": {n: d["commit"] for n, d in DOCS.items() if "commit" in d},
    }


def doc_dir(doc, scratch):
    """The document's own directory inside its scratch copy."""
    return scratch / DOCS[doc].get("dir", ".")


def prepare(doc, engine, scratch):
    """Copy the document, run its prepare commands, build it once with latexmk so the aux
    files exist. Returns (ok, log tail, HOME used)."""
    src = Path("/bench/docs") / doc
    if scratch.exists():
        shutil.rmtree(scratch)
    shutil.copytree(src, scratch)
    home = scratch.parent / f"home-prep-{doc}-{engine}"
    env = dict(os.environ, HOME=str(home))
    if DOCS[doc].get("prepare"):
        r = sh(["bash", "-ec", DOCS[doc]["prepare"]], cwd=scratch, env=env)
        if r.returncode != 0:
            return False, "prepare: " + r.stdout[-1000:] + r.stderr[-1000:], home
    main = DOCS[doc]["main"]
    d = doc_dir(doc, scratch)
    r = sh(["latexmk", LATEXMK_FLAG[engine], "-interaction=nonstopmode", main], cwd=d, env=env)
    pdf = d / main.replace(".tex", ".pdf")
    return r.returncode == 0 and pdf.is_file(), r.stdout[-2000:] + r.stderr[-2000:], home


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


def phases_md(results):
    m = results["machine"]
    md = [f"Machine: {m['cpu']}, pinned to {m['cpus_allowed']} CPU; {m['pdftex']}; image {m['image']}. "
          f"Warm medians of {results['runs']} runs unless the heading says otherwise; the cold run has a fresh HOME.\n"]
    for doc in (d for d in DOCS if d in results):
        engines = [e for e in ENGINES if e in results[doc]]
        runs = DOCS[doc].get("runs")
        md.append(f"\n### {title(doc)}{f' ({runs} warm runs)' if runs else ''}\n\n" + table(results, doc, engines))
        for e in engines:
            entry = results[doc][e]
            if "failed" in entry:
                md.append(f"\n{e}: the latexmk build failed, so nothing was timed.\n")
            sc = entry.get("scale")
            if sc:
                md.append(
                    f"\n{e}: {sc['record_lines']:,} records, {sc['record_bytes'] / 1024:,.0f} KiB. "
                    f"`prumo time --json` reads the cached run in {sc['analyze_cached_ms']:,.0f} ms, and "
                    f"`--html` writes {sc['html_bytes'] / 1024:,.0f} KiB in {sc['html_ms']:,.0f} ms.\n"
                )
    return "".join(md)


def run_phases(a, out, work):
    results = {"machine": machine(), "runs": a.runs}
    for doc in a.docs.split(","):
        runs = DOCS[doc].get("runs") or a.runs
        results[doc] = {}
        for engine in a.engines.split(","):
            if engine not in DOCS[doc].get("engines", ENGINES):
                continue
            print(f"== {doc} on {engine}", flush=True)
            scratch = work / f"{doc}-{engine}"
            ok, log, _ = prepare(doc, engine, scratch)
            if not ok:
                results[doc][engine] = {"failed": "latexmk", "log": log}
                print(f"   latexmk failed:\n{log[-800:]}", flush=True)
                continue
            d = doc_dir(doc, scratch)
            main_file = DOCS[doc]["main"]
            home = work / f"home-{doc}-{engine}"
            cache = work / f"cache-{doc}-{engine}"
            for p in (home, cache):
                shutil.rmtree(p, ignore_errors=True)
                p.mkdir(parents=True)
            cold = summary(*timed_run(d, main_file, engine, home, cache))
            print(f"   cold  {cold['wall_ms']:9.1f} ms wall, {cold['pages']} pages", flush=True)
            warm = []
            for i in range(runs):
                s = summary(*timed_run(d, main_file, engine, home, cache))
                warm.append(s)
                print(f"   warm {i + 1} {s['wall_ms']:9.1f} ms wall", flush=True)
            entry = {"cold": cold, "warm": warm, "median": median_of(warm)}
            if doc == "large":
                entry["scale"] = scale(d, main_file, engine, home, cache)
                if (cache / f"{engine}.html").is_file():
                    shutil.move(str(cache / f"{engine}.html"), str(out / f"large-{engine}.html"))
            results[doc][engine] = entry
            (out / "results.json").write_text(json.dumps(results, indent=1))
    (out / "results.json").write_text(json.dumps(results, indent=1))
    (out / "results.md").write_text(phases_md(results))
    print(phases_md(results))


def run_whatif(a, out, work):
    """`prumo whatif` on each document and engine; one JSON file per document."""
    (out / "whatif").mkdir(exist_ok=True)
    for doc in a.docs.split(","):
        runs = DOCS[doc].get("runs") or a.runs
        entry = {"machine": machine(), "runs": runs}
        for engine in a.engines.split(","):
            if engine not in DOCS[doc].get("engines", ENGINES):
                continue
            print(f"== whatif {doc} on {engine}", flush=True)
            scratch = work / f"{doc}-{engine}"
            ok, log, home = prepare(doc, engine, scratch)
            if not ok:
                entry[engine] = {"failed": "latexmk", "log": log}
                print(f"   latexmk failed:\n{log[-800:]}", flush=True)
                continue
            cache = work / f"cache-whatif-{doc}-{engine}"
            shutil.rmtree(cache, ignore_errors=True)
            env = dict(os.environ, HOME=str(home), PRUMO_CACHE=str(cache))
            main = doc_dir(doc, scratch) / DOCS[doc]["main"]
            cmd = ["prumo", "whatif", "--json", "--shell-escape", "--engine", engine, "--runs", str(runs), str(main)]
            t0 = time.monotonic()
            r = sh(cmd, env=env)
            elapsed = round(time.monotonic() - t0, 1)
            if r.returncode != 0:
                entry[engine] = {"failed": "prumo whatif", "log": r.stderr[-2000:]}
                print(f"   prumo whatif failed:\n{r.stderr[-800:]}", flush=True)
            else:
                entry[engine] = {"report": json.loads(r.stdout), "elapsed_s": elapsed}
                for row in entry[engine]["report"]["experiments"]:
                    print(f"   {row['experiment']:<12} {cell(row)}", flush=True)
            (out / "whatif" / f"{doc}.json").write_text(json.dumps(entry, indent=1))
    (out / "whatif.md").write_text(whatif_md(out))


# The environments prumo watches by default (run::DEFAULT_ENVIRONMENTS), as it loads the probe.
PROBE_ENVS = "tikzpicture,axis,tcolorbox,tblr,longtable,tabularx,minted,lstlisting,forest"


def run_overhead(a, out, work):
    """Plain and probed builds in turns, timed alike: what prumo's probe costs."""
    (out / "overhead").mkdir(exist_ok=True)
    for doc in a.docs.split(","):
        # --runs for every document: the cost is a few percent, and a long document's two or
        # three runs cannot tell that from one slow run.
        runs = a.runs
        entry = {"machine": machine(), "runs": runs}
        for engine in a.engines.split(","):
            if engine not in DOCS[doc].get("engines", ENGINES):
                continue
            print(f"== overhead {doc} on {engine}", flush=True)
            scratch = work / f"{doc}-{engine}"
            ok, log, home = prepare(doc, engine, scratch)
            if not ok:
                entry[engine] = {"failed": "latexmk", "log": log}
                print(f"   latexmk failed:\n{log[-800:]}", flush=True)
                continue
            d = doc_dir(doc, scratch)
            main = DOCS[doc]["main"]
            job = main.removesuffix(".tex")
            plain = [engine, "-interaction=nonstopmode", main]
            probed = [engine, "-interaction=nonstopmode", "-recorder", f"-jobname={job}",
                      f"\\RequirePackage{{prumo}}\\PrumoEnvironments{{{PROBE_ENVS}}}\\input{{{job}}}"]
            env = dict(os.environ, HOME=str(home))
            env_probed = dict(env, TEXINPUTS="/probe:")

            def timed(cmd, e):
                t0 = time.monotonic()
                r = subprocess.run(cmd, cwd=d, env=e, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                return round((time.monotonic() - t0) * 1000, 3), r.returncode

            timed(plain, env)
            timed(probed, env_probed)
            res = {"plain_ms": [], "probed_ms": [], "units": {}, "exit_codes": []}
            for i in range(runs):
                p, pc = timed(plain, env)
                q, qc = timed(probed, env_probed)
                res["plain_ms"].append(p)
                res["probed_ms"].append(q)
                res["exit_codes"].append([pc, qc])
                r = sh(["prumo", "time", "--json", str(d / f"{job}.prumo")], env=env)
                if r.returncode == 0:
                    j = json.loads(r.stdout)
                    res["pages"] = len(j["pages"])
                    for u in j["units"]:
                        res["units"].setdefault(f"{u['kind']} {u['name']}", []).append(u["exclusive_ms"])
                print(f"   run {i + 1}: plain {p:9.1f} ms, probed {q:9.1f} ms", flush=True)
            res["records"] = sum(1 for _ in open(d / f"{job}.prumo"))
            entry[engine] = res
            (out / "overhead" / f"{doc}.json").write_text(json.dumps(entry, indent=1))
    (out / "overhead.md").write_text(overhead_md(out))


def overhead_md(out):
    """The probe's cost per document and engine."""
    md = ["| document | engine | records | plain (ms) | with prumo (ms) | cost |", "|---|---|---:|---:|---:|---:|"]
    for doc in DOCS:
        f = out / "overhead" / f"{doc}.json"
        if not f.is_file():
            continue
        entry = json.loads(f.read_text())
        for e in ENGINES:
            r = entry.get(e)
            if not r or "plain_ms" not in r:
                continue
            p, q = statistics.median(r["plain_ms"]), statistics.median(r["probed_ms"])
            sp = max(r["plain_ms"]) - min(r["plain_ms"])
            sq = max(r["probed_ms"]) - min(r["probed_ms"])
            md.append(f"| {title(doc)} | {e} | {r['records']:,} | {p:,.1f} ± {sp:,.1f} | {q:,.1f} ± {sq:,.1f} | "
                      f"{q - p:+,.1f} ms ({(q - p) / p * 100:+.1f}%) |")
    return "\n".join(md) + "\n"


def cell(row):
    """One experiment in the summary table."""
    st = row["status"]
    if st == "not-run":
        return "–"
    if st == "failed":
        return "fails"
    if row["within_noise"]:
        text = "noise"
    elif row["saved_ms"] >= 0:
        text = f"{row['saved_ms']:,.0f} ms ({row['saved_pct']:.0f}%)"
    else:
        text = f"{-row['saved_ms']:,.0f} ms slower"
    if row["output"]["status"] == "changed":
        text += ", output changed"
    return text


def whatif_md(out):
    """The summary table, the notes it needs, and every row in full."""
    reports = {}
    for doc in DOCS:
        f = out / "whatif" / f"{doc}.json"
        if f.is_file():
            reports[doc] = json.loads(f.read_text())
    if not reports:
        return ""
    exps = ["preamble", "draft", "externalize"]
    md = ["| document | engine | pages | baseline | " + " | ".join(exps) + " |",
          "|---|---|---:|---:|" + "---|" * len(exps)]
    notes, full = [], []
    for doc, entry in reports.items():
        for e in ENGINES:
            if e not in entry:
                continue
            if "report" not in entry[e]:
                md.append(f"| {title(doc)} | {e} | | | {entry[e]['failed']} failed | | |")
                continue
            r = entry[e]["report"]
            rows = {x["experiment"]: x for x in r["experiments"]}
            md.append(
                f"| {title(doc)} | {e} | {r['baseline_pages']:,} | {r['baseline']['median_ms']:,.0f} ms | "
                + " | ".join(cell(rows[x]) if x in rows else "" for x in exps) + " |"
            )
            for x in exps:
                row = rows.get(x)
                if not row:
                    continue
                if row["status"] == "failed":
                    notes.append(f"- {title(doc)}, {e}, {x}: {row['error']}")
                elif row["status"] == "measured" and row["output"]["status"] == "changed":
                    notes.append(f"- {title(doc)}, {e}, {x}: output changed, {row['output']['evidence']}")
                bound = "" if row.get("bound_ms") is None else f"{row['bound_ms']:,.0f}"
                if row["status"] == "measured":
                    v = row["variant"]
                    one = "" if row["one_time_ms"] is None else f"{row['one_time_ms']:,.0f}"
                    full.append(
                        f"| {title(doc)} | {e} | {x} | {r['baseline']['median_ms']:,.1f} ± {r['baseline']['spread_ms']:,.1f} | "
                        f"{v['median_ms']:,.1f} ± {v['spread_ms']:,.1f} | {row['saved_ms']:,.1f} | {bound} | {one} | "
                        f"{row['output']['status']} |"
                    )
                else:
                    why = row.get("reason") or row.get("error", "")
                    full.append(f"| {title(doc)} | {e} | {x} | | | {row['status']}: {why.replace('|', '/')} | {bound} | | |")
    md.append("")
    if notes:
        md += ["Failures and changed output:", ""] + notes + [""]
    md += ["Every row (ms; ± is the spread, slowest run minus fastest):", "",
           "| document | engine | experiment | baseline | variant | saved | bound | one-time | output |",
           "|---|---|---|---:|---:|---:|---:|---:|---|"] + full
    return "\n".join(md) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--docs", default=",".join(DOCS))
    ap.add_argument("--engines", default=",".join(ENGINES))
    ap.add_argument("--whatif", action="store_true")
    ap.add_argument("--overhead", action="store_true")
    ap.add_argument("--report", action="store_true")
    ap.add_argument("--out", default="results")
    a = ap.parse_args()
    unknown = [d for d in a.docs.split(",") if d not in DOCS]
    if unknown:
        raise SystemExit(f"not in corpus.toml: {', '.join(unknown)}")
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    work = Path("/tmp/bench")
    if a.report:
        if (out / "results.json").is_file():
            (out / "results.md").write_text(phases_md(json.loads((out / "results.json").read_text())))
        (out / "whatif.md").write_text(whatif_md(out))
        (out / "overhead.md").write_text(overhead_md(out))
    elif a.overhead:
        run_overhead(a, out, work)
    elif a.whatif:
        run_whatif(a, out, work)
    else:
        run_phases(a, out, work)


if __name__ == "__main__":
    main()
