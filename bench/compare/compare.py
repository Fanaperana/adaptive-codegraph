#!/usr/bin/env python3
"""Head-to-head: ripgrep vs ast-grep vs adaptive-codegraph.

Part 1 - fixtures with ground truth (bench/fixtures/*/expected.txt):
  * callers: "who calls X?" for every callee in the ground truth.
  * impact:  "what is affected if X changes?" (reverse transitive closure,
             up to 4 hops). grep and ast-grep need one search per hop and
             per name; codegraph answers with one `impact` call.
  Every file:line a tool returns is mapped to the function that contains it
  (the nearest `@def` tag above it), so all three are scored the same way.

Part 2 - a real repository (no ground truth): "who calls X?" for a fixed
  sample of functions; output size (what an agent has to read) and time.

Usage:
  python bench/compare/compare.py --ast-grep <ast-grep binary> \
      [--repo <checkout of github.com/ast-grep/ast-grep>] [--json out.json]
"""
import argparse
import json
import os
import random
import re
import shutil
import subprocess
import tempfile
import time
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "bench" / "fixtures"
CG = "adaptive-codegraph"

LANG = {"rust": "rust", "rs-holdout": "rust", "python": "python", "py-holdout": "python",
        "typescript": "typescript", "ts-holdout": "typescript", "go": "go"}
EXT = {"rust": ["rs"], "python": ["py"], "typescript": ["ts"], "go": ["go"]}


def patterns(lang, name):
    """The call patterns an agent would give ast-grep for `name`."""
    p = [f"{name}($$$)", f"$O.{name}($$$)"]
    if lang == "rust":
        p.append(f"$P::{name}($$$)")
    if lang == "typescript":
        p.append(f"new {name}($$$)")
    if lang == "go":
        p.append(f"$P.{name}{{$$$}}")  # composite literal pkg.T{...}
        p.append(f"{name}{{$$$}}")
    return p


def run(argv, cwd):
    t = time.perf_counter()
    r = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    return r.stdout, time.perf_counter() - t


def short(sym):
    return re.split(r"::|\.", sym)[-1]


# ---------------------------------------------------------------- tools
class Grep:
    name = "ripgrep"

    def __init__(self, lang):
        self.globs = sum((["-g", f"*.{e}"] for e in EXT[lang]), [])

    def callers(self, name, cwd):
        out, t = run(["rg", "-n", "-w", "--no-heading", "--color", "never", *self.globs, name, "."], cwd)
        hits = []
        for line in out.splitlines():
            m = re.match(r"^(?:\./|\.\\)?(.+?):(\d+):", line)
            if m:
                hits.append((m.group(1).replace("\\", "/"), int(m.group(2))))
        return hits, len(out.encode()), t, 1


class AstGrep:
    name = "ast-grep"

    def __init__(self, binary, lang):
        self.bin, self.lang = binary, lang

    def callers(self, name, cwd):
        hits, size, secs = [], 0, 0.0
        pats = patterns(self.lang, name)
        for p in pats:
            out, t = run([self.bin, "run", "-p", p, "-l", self.lang, "--json=compact", "."], cwd)
            text, t2 = run([self.bin, "run", "-p", p, "-l", self.lang, "--color", "never", "."], cwd)
            secs += t
            size += len(text.encode())
            for m in json.loads(out or "[]"):
                hits.append((m["file"].replace("\\", "/").removeprefix("./"), m["range"]["start"]["line"] + 1))
        return hits, size, secs, len(pats)


class Codegraph:
    name = "adaptive-codegraph"

    def __init__(self, defs=None):
        # id -> file, for the retry an agent makes with a file-qualified name.
        self.home = {sym: f for f, ds in (defs or {}).items() for _, sym in ds}

    last_name = ""

    def ask(self, cmd, sym, cwd, at=None):
        """Ask by name, then retry the way an agent would, counting each call:
        an unknown name (fixture ids such as `util::checksum` are
        module-style) as `file::name`; an ambiguous one by the ID of the
        candidate defined at `at` = (file, line)."""
        self.last_name = sym
        out, t = run([CG, *cmd(sym)], cwd)
        size, calls = len(out.encode()), 1
        if out.startswith("No symbol named") and sym in self.home:
            self.last_name = f"{self.home[sym]}::{short(sym)}"
            out, t2 = run([CG, *cmd(self.last_name)], cwd)
            size, calls, t = size + len(out.encode()), calls + 1, t + t2
        if " matches " in out.split("\n", 1)[0] and at:
            cands = [(f.replace("\\", "/"), int(l), i)
                     for f, l, i in re.findall(r" - (\S+?):(\d+)\s+\[([0-9a-f]+)", out)]
            same = [c for c in cands if c[0] == at[0]]
            if same:
                pick = self.last_name = min(same, key=lambda c: abs(c[1] - at[1]))[2]
                out, t2 = run([CG, *cmd(pick)], cwd)
                size, calls, t = size + len(out.encode()), calls + 1, t + t2
        return out, size, t, calls

    def callers(self, sym, cwd, at=None):
        """Confirmed callers. Calls the resolver could not link are listed
        after a "Possible callers" line; they are kept in `self.possible`
        and scored separately."""
        out, size, t, calls = self.ask(lambda s: ["callers", s], sym, cwd, at)
        sure = out.split("\n+ ", 1)[0]
        # The default answer only counts possible callers; list them with a
        # second call (not counted: scored as a separate row).
        maybe = ""
        if re.search(r"^\+ \d+ possible", out, re.M):
            maybe = run([CG, "callers", "--possible", self.last_name], cwd)[0].partition("Possible callers")[2]
        find = lambda text: [(f.replace("\\", "/"), int(l)) for f, l in re.findall(r" - (\S+?):(\d+)", text)]
        self.possible = find(maybe)
        return find(sure), size, t, calls

    def impact(self, sym, cwd):
        out, size, t, calls = self.ask(lambda s: ["impact", s, "--depth", "4"], sym, cwd)
        body = out.split("\n", 1)[1] if "\n" in out else ""
        hits = [(f.replace("\\", "/"), int(l)) for f, l in re.findall(r" - (\S+?):(\d+)", body)]
        return hits, size, t, calls


# ---------------------------------------------------------------- fixtures
def load_fixture(d):
    defs = defaultdict(list)  # file -> [(line, id)]
    for f in d.rglob("*"):
        if f.suffix.lstrip(".") in sum(EXT.values(), []):
            rel = f.relative_to(d).as_posix()
            for i, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
                m = re.search(r"@def\s+(\S+)", line)
                if m:
                    defs[rel].append((i, m.group(1)))
    edges = set()
    for line in (d / "expected.txt").read_text(encoding="utf-8").splitlines():
        if "->" in line and not line.lstrip().startswith("#"):
            a, b = (s.strip() for s in line.split("->"))
            edges.add((a, b))
    return defs, edges


def enclosing(defs, file, line):
    best = None
    for l, sym in defs.get(file, []):
        if l <= line:
            best = sym
    return best or f"<{file}>"


def prf(pred, truth):
    tp = len(pred & truth)
    p = tp / len(pred) if pred else 1.0
    r = tp / len(truth) if truth else 1.0
    return tp, p, r


def ancestors(edges, sym):
    rev = defaultdict(set)
    for a, b in edges:
        rev[b].add(a)
    seen, frontier = set(), {sym}
    for _ in range(4):
        nxt = set()
        for s in frontier:
            for c in rev[s] - seen - {sym}:
                seen.add(c)
                nxt.add(c)
        frontier = nxt
    return seen


def bench_fixtures(sg_bin):
    res = {t: defaultdict(float) for t in ("ripgrep", "ast-grep", "adaptive-codegraph", "adaptive-codegraph + possible")}
    for d in sorted(FIXTURES.iterdir()):
        if not (d / "expected.txt").exists():
            continue
        lang = LANG[d.name]
        tmp = Path(tempfile.mkdtemp(prefix="acg-cmp-"))
        shutil.copytree(d, tmp / d.name)
        cwd = tmp / d.name
        subprocess.run([CG, "index", "--quiet"], cwd=cwd, capture_output=True)
        defs, edges = load_fixture(cwd)
        callees = sorted({b for _, b in edges})
        text_tools = [Grep(lang), AstGrep(sg_bin, lang)]

        for tool in text_tools + [Codegraph(defs)]:
            r = res[tool.name]
            # --- callers
            pred, plus = set(), set()
            for c in callees:
                q = c if tool.name == "adaptive-codegraph" else short(c)
                hits, size, secs, calls = tool.callers(q, cwd)
                pred |= {(enclosing(defs, f, l), c) for f, l in hits}
                plus |= {(enclosing(defs, f, l), c) for f, l in getattr(tool, "possible", [])}
                r["callers_bytes"] += size
                r["callers_secs"] += secs
                r["callers_calls"] += calls
                r["callers_lines"] += len(hits)
            tp, _, _ = prf(pred, edges)
            r["callers_tp"] += tp
            r["callers_pred"] += len(pred)
            r["callers_truth"] += len(edges)
            if tool.name == "adaptive-codegraph":
                both = pred | plus
                tp, _, _ = prf(both, edges)
                rp = res["adaptive-codegraph + possible"]
                rp["callers_tp"] += tp
                rp["callers_pred"] += len(both)
                rp["callers_truth"] += len(edges)

            # --- impact (reverse closure, 4 hops)
            for c in callees:
                truth = ancestors(edges, c)
                if tool.name == "adaptive-codegraph":
                    hits, size, secs, calls = tool.impact(c, cwd)
                    found = {enclosing(defs, f, l) for f, l in hits} - {c}
                else:
                    found, size, secs, calls = set(), 0, 0.0, 0
                    frontier, asked = {c}, set()
                    for _ in range(4):
                        nxt = set()
                        for s in frontier:
                            n = short(s)
                            if n in asked:
                                continue
                            asked.add(n)
                            h, sz, t, k = tool.callers(n, cwd)
                            size, secs, calls = size + sz, secs + t, calls + k
                            for f, l in h:
                                e = enclosing(defs, f, l)
                                if e != c and e not in found:
                                    found.add(e)
                                    nxt.add(e)
                        frontier = nxt
                tp, _, _ = prf(found, truth)
                r["impact_tp"] += tp
                r["impact_pred"] += len(found)
                r["impact_truth"] += len(truth)
                r["impact_bytes"] += size
                r["impact_secs"] += secs
                r["impact_calls"] += calls
        shutil.rmtree(tmp, ignore_errors=True)
        print(f"  fixture {d.name}: {len(edges)} edges, {len(callees)} callees")
    return res


# ---------------------------------------------------------------- real repo
def bench_repo(sg_bin, src, n=25, seed=7):
    # Work on a copy so the checkout is never written to.
    tmp = Path(tempfile.mkdtemp(prefix="acg-cmp-repo-"))
    repo = tmp / Path(src).name
    shutil.copytree(src, repo, ignore=shutil.ignore_patterns(".git", "target", "node_modules", ".adaptive-codegraph"))
    out, _ = run(["rg", "-n", "--no-heading", "-g", "*.rs", "-g", "!**/tests/**", r"^\s*(pub(\([^)]*\))?\s+)?fn\s+([a-z_][a-z0-9_]{3,})", "crates"], repo)
    found = []
    for line in out.splitlines():
        m = re.match(r"^(.+?):(\d+):.*?fn\s+([a-z_][a-z0-9_]+)", line)
        # Tests are never called, so they say nothing about finding callers.
        if m and not m.group(3).startswith("test") and m.group(3) not in ("main", "new", "default", "from", "into"):
            found.append((m.group(1).replace("\\", "/"), m.group(3), int(m.group(2))))
    found.sort()  # rg's output order varies between runs
    random.Random(seed).shuffle(found)
    sample = found[:n]

    _, idx_secs = run([CG, "index", "--quiet"], repo)
    tools = [Grep("rust"), AstGrep(sg_bin, "rust"), Codegraph()]
    res = {t.name: defaultdict(float) for t in tools}
    for file, name, line in sample:
        for tool in tools:
            if tool.name == "adaptive-codegraph":
                hits, size, secs, calls = tool.callers(f"{file}::{name}", repo, at=(file, line))
            else:
                hits, size, secs, calls = tool.callers(name, repo)
            r = res[tool.name]
            r["lines"] += len(hits)
            r["bytes"] += size
            r["secs"] += secs
            r["calls"] += calls
    for r in res.values():
        for k in list(r):
            r[k] /= len(sample)
    shutil.rmtree(tmp, ignore_errors=True)
    return {"sample": sample, "index_secs": idx_secs, "per_query": res}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ast-grep", required=True)
    ap.add_argument("--repo")
    ap.add_argument("--json")
    a = ap.parse_args()

    print("Fixtures (ground truth):")
    fx = bench_fixtures(a.ast_grep)
    print(f"\n{'tool':20} {'callers P / R':>14} {'calls':>6} {'KB':>6} | {'impact P / R':>13} {'calls':>6} {'KB':>6}")
    for t, r in fx.items():
        cp = r["callers_tp"] / r["callers_pred"] if r["callers_pred"] else 1
        cr = r["callers_tp"] / r["callers_truth"]
        ip = r["impact_tp"] / r["impact_pred"] if r["impact_pred"] else 1
        ir = r["impact_tp"] / r["impact_truth"] if r["impact_truth"] else 0
        r.update(callers_precision=cp, callers_recall=cr, impact_precision=ip, impact_recall=ir)
        if t.endswith("+ possible"):
            print(f"{t:20} {cp:6.2f} / {cr:4.2f}  (confirmed + possible callers, listed on request)")
            continue
        print(f"{t:20} {cp:6.2f} / {cr:4.2f} {r['callers_calls']:6.0f} {r['callers_bytes']/1024:6.1f} | "
              f"{ip:5.2f} / {ir:4.2f} {r['impact_calls']:6.0f} {r['impact_bytes']/1024:6.1f}")

    out = {"fixtures": fx}
    if a.repo:
        print(f"\nReal repo {a.repo}: who calls X? (25 sampled functions, per question)")
        rp = bench_repo(a.ast_grep, a.repo)
        print(f"  codegraph index build: {rp['index_secs']:.1f}s (once)")
        for t, r in rp["per_query"].items():
            print(f"  {t:20} {r['lines']:6.1f} lines  {r['bytes']:8.0f} bytes (~{r['bytes']/4:5.0f} tokens)  "
                  f"{r['secs']*1000:6.0f} ms  {r['calls']:.1f} call(s)")
        out["repo"] = rp
    if a.json:
        Path(a.json).write_text(json.dumps(out, indent=2, default=dict), encoding="utf-8")


if __name__ == "__main__":
    main()
