#!/usr/bin/env python3
"""Edge cases, scored one by one: ripgrep vs ast-grep vs adaptive-codegraph.

Part 1 - "who calls X?" on small projects built around one hard case each
  (bench/edge/<lang>/, cases in cases.txt): same-named methods on other
  types, calls through typed fields, aliased and re-exported imports,
  names in comments and strings, shadowing, callbacks, super() calls,
  macro arguments, closures, optional chaining, default exports. Every
  file:line a tool returns is credited to the function that contains it
  (nearest `@def` marker above), then compared with the expected callers.

Part 2 - text search edge cases, codegraph `grep` vs ripgrep, line for line:
  Unicode, case folding, CRLF, BOM, UTF-16, very long lines, files over
  4 MB, binary, ignored and hidden files, no-literal regexes, empty lines,
  word boundaries, a last line with no newline.

Usage:
  python bench/compare/edge.py --ast-grep <ast-grep binary> [--json out]
"""
import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from compare import CG, ROOT, AstGrep, Codegraph, Grep, enclosing, load_fixture, run  # noqa: E402

EDGE = ROOT / "bench" / "edge"
LANG = {"python": "python", "typescript": "typescript", "rust": "rust"}


def load_cases(d):
    cases = []
    for line in (d / "cases.txt").read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        name, sym, short, callers = (p.strip() for p in line.split("|"))
        cases.append(dict(case=name, symbol=sym, name=short,
                          expected=sorted(c.strip() for c in callers.split(",") if c.strip())))
    return cases


def defs_of(cwd):
    """@def markers in every source file (load_fixture wants expected.txt)."""
    (cwd / "expected.txt").write_text("", encoding="utf-8")
    defs, _ = load_fixture(cwd)
    return defs


def score(found, expected):
    found, expected = set(found), set(expected)
    tp = len(found & expected)
    p = tp / len(found) if found else (1.0 if not expected else 0.0)
    r = tp / len(expected) if expected else 1.0
    return p, r


def callers_part(sg):
    rows = []
    for d in sorted(EDGE.iterdir()):
        if not (d / "cases.txt").exists():
            continue
        lang = LANG[d.name]
        tmp = Path(tempfile.mkdtemp(prefix="acg-edge-"))
        cwd = tmp / d.name
        shutil.copytree(d, cwd)
        subprocess.run([CG, "index", "--quiet"], cwd=cwd, capture_output=True)
        defs = defs_of(cwd)
        tools = [Grep(lang), AstGrep(sg, lang), Codegraph(defs)]
        for c in load_cases(d):
            row = dict(lang=d.name, case=c["case"], expected=c["expected"])
            for t in tools:
                q = c["symbol"] if t.name == "adaptive-codegraph" else c["name"]
                hits, size, _, calls = t.callers(q, cwd)
                found = sorted({enclosing(defs, f, l) for f, l in hits})
                p, r = score(found, c["expected"])
                row[t.name] = dict(found=found, precision=p, recall=r, bytes=size, calls=calls)
                if t.name == "adaptive-codegraph":
                    extra = sorted({enclosing(defs, f, l) for f, l in t.possible})
                    row["possible"] = extra
            rows.append(row)
        shutil.rmtree(tmp, ignore_errors=True)
    return rows


# --------------------------------------------------------------- text search
def text_cases():
    big = ("filler line for a large file\n" * 160_000) + "needle_in_big_file\n"
    return {
        # file name -> bytes
        "files": {
            "unicode.txt": "café naïve 日本語 straße\nCAFÉ NAÏVE\n".encode("utf-8"),
            "crlf.txt": b"first line\r\nsecond target line\r\nthird\r\n",
            "bom.py": "﻿import os\nimport sys\n".encode("utf-8"),
            "utf16.txt": "utf16 target here\n".encode("utf-16"),
            "long.txt": ("x" * 200_000 + " long_line_target " + "y" * 1000 + "\n").encode(),
            "big.log": big.encode(),
            "blob.bin": b"\x00\x01binary_target\x00" * 10,
            "ignored.txt": b"ignored_target\n",
            ".hidden.txt": b"hidden_target\n",
            "phone.txt": b"call 555-1234 or 555-9876\nno number\n",
            "words.py": b"run(run_all(run))\nrerun()\n_run = 1\n",
            "empty_lines.txt": b"a\n\n\nb\n",
            "no_newline.txt": b"first\nlast_line_target",
            "latin1.txt": "ol\xe9 latin1_target\n".encode("latin-1"),
            ".gitignore": b"ignored.txt\n",
        },
        "queries": [
            ("Unicode literal", dict(pattern="naïve", fixed=True)),
            ("Unicode, ignore case", dict(pattern="CAFÉ", ignore_case=True)),
            ("Unicode, ignore case (ß)", dict(pattern="STRASSE", ignore_case=True)),
            ("CRLF line endings", dict(pattern="target line$")),
            ("UTF-8 BOM, anchored ^", dict(pattern="^import")),
            ("UTF-16 file", dict(pattern="utf16 target", fixed=True)),
            ("200 KB line", dict(pattern="long_line_target", fixed=True)),
            ("file over 4 MB", dict(pattern="needle_in_big_file", fixed=True)),
            ("binary file", dict(pattern="binary_target", fixed=True)),
            (".gitignore'd file", dict(pattern="ignored_target", fixed=True)),
            ("hidden file", dict(pattern="hidden_target", fixed=True)),
            ("regex, no literal", dict(pattern=r"\b\d{3}-\d{4}\b")),
            ("alternation", dict(pattern="first|last_line")),
            ("whole word vs run_all, _run", dict(pattern="run", word=True)),
            ("empty lines ^$", dict(pattern="^$")),
            ("last line, no newline", dict(pattern="last_line_target$")),
            ("invalid UTF-8 (Latin-1)", dict(pattern="latin1_target", fixed=True)),
        ],
    }


def rg_lines(q, cwd):
    flags = (["-F"] if q.get("fixed") else []) + (["-i"] if q.get("ignore_case") else []) + (["-w"] if q.get("word") else [])
    out, _ = run(["rg", "-n", "--no-heading", "--color", "never", *flags, "-e", q["pattern"], "."], cwd)
    s = set()
    for line in out.splitlines():
        parts = line.split(":", 2)
        if len(parts) >= 2 and parts[1].isdigit():
            s.add((parts[0].replace("\\", "/").removeprefix("./"), int(parts[1])))
    return s


def cg_lines(q, cwd):
    flags = (["-F"] if q.get("fixed") else []) + (["-i"] if q.get("ignore_case") else []) + (["-w"] if q.get("word") else [])
    out, _ = run([CG, "--json", "grep", *flags, "--", q["pattern"]], cwd)
    try:
        return {(m["file"], m["line"]) for m in json.loads(out)["matches"]}
    except (ValueError, KeyError):
        return {("<error>", 0)}


def text_part():
    spec = text_cases()
    tmp = Path(tempfile.mkdtemp(prefix="acg-edge-text-"))
    for name, data in spec["files"].items():
        (tmp / name).write_bytes(data)
    subprocess.run(["git", "init", "-q"], cwd=tmp)  # rg honours .gitignore only in a repo
    rows = []
    for case, q in spec["queries"]:
        a, b = rg_lines(q, tmp), cg_lines(q, tmp)
        rows.append(dict(case=case, pattern=q["pattern"], rg=sorted(a), codegraph=sorted(b), same=a == b))
    shutil.rmtree(tmp, ignore_errors=True)
    return rows


def mark(x):
    if x["precision"] == 1 and x["recall"] == 1:
        return "✅"
    if x["recall"] == 1:
        return f"⚠️ +{len(x['found']) - round(x['precision'] * len(x['found']))} wrong"
    if x["recall"] == 0:
        return "❌ missed"
    return f"⚠️ P {x['precision']:.2f} R {x['recall']:.2f}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ast-grep", required=True)
    ap.add_argument("--json")
    a = ap.parse_args()

    rows = callers_part(a.ast_grep)
    print("| Language | Edge case | ripgrep | ast-grep | adaptive-codegraph |")
    print("|---|---|---|---|---|")
    exact = {"ripgrep": 0, "ast-grep": 0, "adaptive-codegraph": 0}
    for r in rows:
        for t in exact:
            exact[t] += r[t]["precision"] == 1 and r[t]["recall"] == 1
        print(f"| {r['lang']} | {r['case']} | {mark(r['ripgrep'])} | {mark(r['ast-grep'])} | {mark(r['adaptive-codegraph'])} |")
    print(f"| | **exact answers** | **{exact['ripgrep']}/{len(rows)}** | **{exact['ast-grep']}/{len(rows)}** | **{exact['adaptive-codegraph']}/{len(rows)}** |")
    for r in rows:
        bad = {t: r[t]["found"] for t in exact if not (r[t]["precision"] == 1 and r[t]["recall"] == 1)}
        if bad:
            print(f"  {r['lang']} / {r['case']}: expected {r['expected']}; " +
                  "; ".join(f"{t} gave {v}" for t, v in bad.items()) +
                  (f"; codegraph possible {r['possible']}" if r.get("possible") else ""))

    text = text_part()
    print("\n| Text search edge case | ripgrep lines | codegraph lines | Same |")
    print("|---|---|---|---|")
    for r in text:
        print(f"| {r['case']} (`{r['pattern']}`) | {len(r['rg'])} | {len(r['codegraph'])} | {'✅' if r['same'] else '❌'} |")
    print(f"| **same as ripgrep** | | | **{sum(r['same'] for r in text)}/{len(text)}** |")
    for r in text:
        if not r["same"]:
            print(f"  {r['case']}: rg {r['rg']} vs codegraph {r['codegraph']}")
    if a.json:
        Path(a.json).write_text(json.dumps({"callers": rows, "text": text}, indent=1, ensure_ascii=False), encoding="utf-8")


if __name__ == "__main__":
    main()
