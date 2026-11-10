#!/usr/bin/env python3
"""Lay out the benchmark documents of corpus.toml under one directory.

A document in this repository is copied, a generated one is written by its script, and a
fetched one is taken from its repository at the pinned commit with `git archive`. Clones
are kept in BENCH_REPOS (default ~/.cache/prumo-bench/repos) and fetched shallow, one
commit each, so a second run needs no network. Nothing fetched is stored in this
repository.

Usage: fetch.py --out DIR [--docs a,b]
"""
import argparse
import os
import shutil
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def git(repo_dir, *args, **kw):
    return subprocess.run(["git", "-C", str(repo_dir), *args], check=True, **kw)


def clone_dir(url):
    """`borisveytsman-acmart` for https://github.com/borisveytsman/acmart."""
    parts = url.rstrip("/").removesuffix(".git").split("/")
    return "-".join(parts[-2:])


def ensure_commit(repos, url, commit):
    """A clone of `url` under `repos` that has `commit`."""
    d = repos / clone_dir(url)
    if not (d / ".git").is_dir():
        d.mkdir(parents=True, exist_ok=True)
        git(d, "init", "-q")
        git(d, "remote", "add", "origin", url)
    have = subprocess.run(["git", "-C", str(d), "cat-file", "-e", f"{commit}^{{commit}}"], capture_output=True)
    if have.returncode != 0:
        print(f"fetching {url} at {commit[:12]}", file=sys.stderr, flush=True)
        git(d, "fetch", "-q", "--depth", "1", "origin", commit)
    return d


def archive(repo_dir, commit, dest):
    """`git archive <commit>` extracted into dest. The data filter refuses absolute paths,
    links out of dest and device files: these are other people's trees."""
    dest.mkdir(parents=True)
    p = subprocess.Popen(["git", "-C", str(repo_dir), "archive", "--format=tar", commit], stdout=subprocess.PIPE)
    with tarfile.open(fileobj=p.stdout, mode="r|") as tar:
        tar.extractall(dest, filter="data")
    if p.wait() != 0:
        raise RuntimeError(f"git archive {commit} in {repo_dir} failed")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--docs", help="comma-separated names (default: all)")
    a = ap.parse_args()
    corpus = tomllib.loads((HERE / "corpus.toml").read_text())
    names = a.docs.split(",") if a.docs else list(corpus)
    unknown = [n for n in names if n not in corpus]
    if unknown:
        sys.exit(f"not in corpus.toml: {', '.join(unknown)}")
    repos = Path(os.environ.get("BENCH_REPOS", Path.home() / ".cache/prumo-bench/repos"))
    for name in names:
        doc = corpus[name]
        dest = a.out / name
        if dest.exists():
            shutil.rmtree(dest)
        if "source" in doc:
            shutil.copytree(HERE / doc["source"], dest)
        elif "generate" in doc:
            dest.mkdir(parents=True)
            subprocess.run([sys.executable, "-I", str(HERE / doc["generate"]), "--out", str(dest / doc["main"])], check=True)
        else:
            archive(ensure_commit(repos, doc["repo"], doc["commit"]), doc["commit"], dest)
        print(f"{name}: {dest}", file=sys.stderr)


if __name__ == "__main__":
    main()
