#!/usr/bin/env python3
"""Text search head-to-head: ripgrep vs git grep vs adaptive-codegraph.

For each repository a fixed, seeded set of queries is generated from the
repository itself (no hand-picking):
  * 10 identifiers taken from definitions (`fn x`, `class X`, ...), whole word
  * 5 string literals found in the code, as literal (-F) searches
  * 5 common searches: TODO, fixme (ignore case), version numbers,
    URLs and `...Error` names (regexes, two with no literal to filter on)

Each tool runs every query. Measured:
  * latency: median of 3 runs. ripgrep, git grep and the codegraph CLI are
    separate processes (cold). "codegraph (server)" is the same search sent
    to a running MCP server (warm), which is how an agent uses it.
  * agreement with ripgrep: precision / recall of the (file, line) pairs,
    with ripgrep's answer taken as the truth.
  * output: bytes an agent reads (ripgrep `-n --no-heading` vs codegraph's
    grouped answer with every match), and how many files codegraph actually
    scanned after its trigram filter.

The index is built in the repository (in `.adaptive-codegraph/`, which
ignores itself) and removed afterwards unless it was already there.

Usage:
  python bench/compare/text.py --repo <path> [--repo <path> ...]
      [--cg adaptive-codegraph] [--mcp adaptive-codegraph-mcp] [--json out]
"""
import argparse
import json
import os
import random
import re
import shutil
import statistics
import subprocess
import time
from pathlib import Path

ENV = dict(os.environ, ACG_NO_EMBED="1")
RUNS = 3


def run(argv, cwd):
    t = time.perf_counter()
    # stdin closed: with no terminal, rg would otherwise search stdin.
    r = subprocess.run(argv, cwd=cwd, capture_output=True, env=ENV, stdin=subprocess.DEVNULL)
    return r.stdout.decode("utf-8", "replace"), time.perf_counter() - t, r.returncode


def timed(argv, cwd):
    """Output of the first run and the median time of RUNS runs."""
    out, t0, code = run(argv, cwd)
    ts = [t0] + [run(argv, cwd)[1] for _ in range(RUNS - 1)]
    return out, statistics.median(ts), code


def pairs(out):
    s = set()
    for line in out.splitlines():
        m = re.match(r"^(.+?):(\d+):", line)
        if m:
            s.add((m.group(1).replace("\\", "/").removeprefix("./"), int(m.group(2))))
    return s


# ------------------------------------------------------------------ tools
def rg(q, cwd):
    flags = (["-F"] if q["fixed"] else []) + (["-i"] if q["ignore_case"] else []) + (["-w"] if q["word"] else [])
    out, t, _ = timed(["rg", "-n", "--no-heading", "--color", "never", *flags, "-e", q["pattern"], "."], cwd)
    return pairs(out), t, len(out.encode())


def git_grep(q, cwd):
    mode = ["-F"] if q["fixed"] else ["-P"]
    flags = (["-i"] if q["ignore_case"] else []) + (["-w"] if q["word"] else [])
    out, t, code = timed(["git", "grep", "-n", "-I", "--no-color", *mode, *flags, "-e", q["pattern"]], cwd)
    if code not in (0, 1):
        return None, None, None
    return pairs(out), t, len(out.encode())


def cg_cli(cg, q, cwd):
    flags = (["-F"] if q["fixed"] else []) + (["-i"] if q["ignore_case"] else []) + (["-w"] if q["word"] else [])
    out, t, _ = timed([cg, "grep", "--flat", *flags, "--", q["pattern"]], cwd)
    return pairs(out), t


class Server:
    """A running MCP server, used the way an agent uses it."""

    def __init__(self, mcp, cwd):
        self.p = subprocess.Popen([mcp, "--base", "."], cwd=cwd, env=ENV, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.i = 0
        self.call("index_status", {})

    def call(self, name, args):
        self.i += 1
        msg = {"jsonrpc": "2.0", "id": self.i, "method": "tools/call", "params": {"name": name, "arguments": args}}
        self.p.stdin.write((json.dumps(msg) + "\n").encode())
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())["result"]["content"][0]["text"]

    def search(self, q):
        args = {"pattern": q["pattern"], "fixed": q["fixed"], "ignore_case": q["ignore_case"],
                "word": q["word"], "limit": 0}
        ts, out = [], ""
        for _ in range(RUNS):
            t = time.perf_counter()
            out = self.call("text_search", args)
            ts.append(time.perf_counter() - t)
        m = re.search(r"\((\d+) of (\d+) files scanned\)", out)
        return out, statistics.median(ts), (int(m.group(1)), int(m.group(2))) if m else None

    def close(self):
        self.p.kill()


# ---------------------------------------------------------------- queries
def queries(repo, seed=7):
    rnd = random.Random(seed)
    out, _, _ = run(["rg", "-o", "-N", "--no-filename", "-r", "$1",
                     r"\b(?:fn|function|def|class|struct|interface|func|type|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]{5,})", "."], repo)
    idents = sorted(set(out.split()))
    out, _, _ = run(["rg", "-o", "-N", "--no-filename", r"\"[A-Z][a-z]+(?: [a-z]+){2,5}\"", "."], repo)
    strings = sorted({s.strip('"') for s in out.splitlines()})
    qs = []
    for name in rnd.sample(idents, min(10, len(idents))):
        qs.append(dict(kind="identifier", pattern=name, fixed=True, ignore_case=False, word=True))
    for s in rnd.sample(strings, min(5, len(strings))):
        qs.append(dict(kind="string", pattern=s, fixed=True, ignore_case=False, word=False))
    qs += [
        dict(kind="common", pattern="TODO", fixed=True, ignore_case=False, word=False),
        dict(kind="common", pattern="fixme", fixed=True, ignore_case=True, word=False),
        dict(kind="regex, no literal", pattern=r"\d+\.\d+\.\d+", fixed=False, ignore_case=False, word=False),
        dict(kind="regex", pattern=r"https?://[a-z0-9.-]+", fixed=False, ignore_case=False, word=False),
        dict(kind="regex, no literal", pattern=r"\b[A-Z][a-zA-Z]+Error\b", fixed=False, ignore_case=False, word=False),
    ]
    return qs


def prf(pred, truth):
    tp = len(pred & truth)
    p = tp / len(pred) if pred else (1.0 if not truth else 0.0)
    r = tp / len(truth) if truth else 1.0
    return p, r


def bench(repo, cg, mcp):
    repo = Path(repo).resolve()
    idx = repo / ".adaptive-codegraph"
    existed = idx.exists()
    sha = run(["git", "rev-parse", "--short", "HEAD"], repo)[0].strip()
    files = len(run(["git", "ls-files"], repo)[0].splitlines())
    print(f"\n## {repo.name} @ {sha} ({files} tracked files)")
    t = time.perf_counter()
    subprocess.run([cg, "index", "-q"], cwd=repo, env=ENV, check=True)
    build = time.perf_counter() - t
    print(f"index build (graph + text, no embeddings): {build:.1f}s")
    qs = queries(repo)
    srv = Server(mcp, repo)
    rows = []
    try:
        for q in qs:
            truth, t_rg, b_rg = rg(q, repo)
            gg, t_gg, _ = git_grep(q, repo)
            cgp, t_cli = cg_cli(cg, q, repo)
            text, t_srv, scanned = srv.search(q)
            p, r = prf(cgp, truth)
            gp, gr = prf(gg, truth) if gg is not None else (None, None)
            rows.append(dict(q, matches=len(truth), rg_ms=t_rg * 1000, git_ms=t_gg * 1000 if t_gg else None,
                             cli_ms=t_cli * 1000, server_ms=t_srv * 1000, precision=p, recall=r,
                             git_precision=gp, git_recall=gr, rg_bytes=b_rg, cg_bytes=len(text.encode()),
                             scanned=scanned[0] if scanned else None, indexed=scanned[1] if scanned else None,
                             missed=sorted(truth - cgp)[:5], extra=sorted(cgp - truth)[:5]))
            print(f"  {q['kind']:18} {q['pattern'][:28]!r:32} n={len(truth):5}  rg {t_rg*1000:6.0f}ms  "
                  f"git {t_gg*1000 if t_gg else float('nan'):6.0f}ms  cli {t_cli*1000:6.0f}ms  "
                  f"server {t_srv*1000:7.1f}ms  P/R {p:.2f}/{r:.2f}  scanned {scanned}")
    finally:
        srv.close()
        if not existed:
            shutil.rmtree(idx, ignore_errors=True)
    return dict(repo=repo.name, commit=sha, files=files, index_seconds=build, queries=rows)


def summary(res):
    rows = res["queries"]
    med = lambda k: statistics.median(r[k] for r in rows if r[k] is not None)
    tp = sum(r["matches"] * r["recall"] for r in rows)
    found = sum(r["matches"] * r["recall"] / r["precision"] if r["precision"] else 0 for r in rows)
    truth = sum(r["matches"] for r in rows)
    return dict(
        rg_ms=med("rg_ms"), git_ms=med("git_ms"), cli_ms=med("cli_ms"), server_ms=med("server_ms"),
        precision=tp / found if found else 1.0, recall=tp / truth if truth else 1.0,
        exact=sum(1 for r in rows if r["precision"] == 1 and r["recall"] == 1), n=len(rows),
        rg_kb=sum(r["rg_bytes"] for r in rows) / 1024, cg_kb=sum(r["cg_bytes"] for r in rows) / 1024,
        scanned=statistics.median(r["scanned"] / r["indexed"] for r in rows if r["indexed"]),
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", action="append", required=True)
    ap.add_argument("--cg", default="adaptive-codegraph")
    ap.add_argument("--mcp", default="adaptive-codegraph-mcp")
    ap.add_argument("--json")
    a = ap.parse_args()
    results = [bench(r, a.cg, a.mcp) for r in a.repo]
    print("\n| repo | ripgrep | git grep | codegraph CLI | codegraph server | same lines as rg | queries exact | files scanned |")
    print("|---|---|---|---|---|---|---|---|")
    for res in results:
        s = res["summary"] = summary(res)
        print(f"| {res['repo']} ({res['files']} files) | {s['rg_ms']:.0f} ms | {s['git_ms']:.0f} ms | {s['cli_ms']:.0f} ms | "
              f"{s['server_ms']:.1f} ms | P {s['precision']:.3f} / R {s['recall']:.3f} | {s['exact']}/{s['n']} | "
              f"{s['scanned']*100:.1f}% |")
    if a.json:
        Path(a.json).write_text(json.dumps(results, indent=1, default=list))


if __name__ == "__main__":
    main()
