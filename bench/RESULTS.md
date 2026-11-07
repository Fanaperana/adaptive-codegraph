# Benchmark results

Produced by `acg-bench` (`crates/bench`). Raw numbers are in
`bench/results/*.json`. Reproduce:

```bash
cargo build --release -p adaptive-codegraph-bench            # lexical only
cargo run --release -p adaptive-codegraph-bench -- --label run \
  --repo <repo> --search-repo <ast-grep checkout> --search-queries bench/search/ast-grep.txt
cargo build --release -p adaptive-codegraph-bench --features fastembed   # + embeddings
```

## What is measured

- **Call-graph accuracy**: small multi-file projects in `bench/fixtures/`
  with every definition tagged `@def <id>` and the true `caller -> callee`
  edges listed in `expected.txt`. *Design* fixtures (rust, python,
  typescript, go) were written together with the resolver; *held-out*
  fixtures (`*-holdout`) were written afterwards, with patterns the resolver
  had not been built for (inheritance, `super()`, interface dispatch,
  attribute types, default imports, trait objects). Held-out numbers are the
  honest ones.
- **Incremental consistency**: index, edit a file, re-index incrementally,
  compare with a fresh full index (Δ edges / Δ symbols must be 0).
- **Impact analysis**: for every function, `impact()` vs the reverse
  transitive closure of the expected call edges. *Static* follows calls
  only; the default mode also follows dynamic dispatch (callers of an
  overridden interface method, implementors of a changed trait method),
  which are real effects that the call-only ground truth does not count.
- **Search**: 24 natural-language queries against
  [ast-grep](https://github.com/ast-grep/ast-grep) at `cab768cd`, each with
  one expected function (`bench/search/ast-grep.txt`).
- **Context**: whether `context(query, ~1500 tokens)` contains the expected
  function's source.
- **Scale**: real repositories indexed into a temp folder (never written to).

## Call graph (precision / recall, edges at any confidence)

| | Baseline | Phase 1 (resolver) | Phase 2 (supertypes, attrs) |
|---|---|---|---|
| Design fixtures | 0.51 / 0.45 | 1.00 / 1.00 | 1.00 / 1.00 |
| Held-out fixtures | ~0.22 / ~0.24 | 1.00 / 0.71 | **1.00 / 0.94** |
| Incremental vs full (Δ edges) | −5 to −8 per fixture | 0 | 0 |

The two remaining held-out misses need flow analysis the graph does not do:
a call on an untyped constructor parameter stored in `self.db`, and a call on
a closure parameter inside an iterator chain (`items.iter().map(|c| c.area())`).

## Impact analysis (all fixtures)

| Mode | Precision | Recall |
|---|---|---|
| Static (calls only) | 1.00 | 0.97 |
| Default (+ dynamic dispatch) | 0.89* | 0.97 |

\* every extra entry was checked by hand: implementors of a changed trait
method, or callers that reach the method through an interface / base class.

## Search (ast-grep, 24 queries)

| | R@1 | R@5 | R@10 | MRR |
|---|---|---|---|---|
| Baseline (names / paths / signatures) | 0.25 | 0.46 | 0.50 | 0.32 |
| Lexical (split identifiers, docs, bodies, weighting) | 0.42 | 0.71 | 0.79 | 0.56 |
| Hybrid (lexical + BGE-small embeddings) | **0.71** | **0.88** | **0.92** | **0.78** |

`context()` within ~1500 tokens contained the target's source for 71% of
queries (lexical) / **88%** (hybrid), at about half the size of the target's
file alone (~5 KB vs ~10 KB), and also lists the signatures of related
callers and callees.

## Real repositories (lexical build, same settings as baseline)

| Repo | Version | Files | Symbols | Duplicate IDs | Duplicate edges | Calls into non-callables | Index time | Avg neighborhood answer |
|---|---|---|---|---|---|---|---|---|
| this repo (`crates/`) | baseline | 20 | 550 | 21 | 262 | 44 | 0.1 s | 2.9 KB |
| | now | 24 | 978 | 0 | 0 | 0 | 0.3 s | 3.3 KB |
| ast-grep | baseline | 182 | 5,774 | 534 | 4,884 | 2,244 | 0.4 s | 17 KB |
| | now | 185 | 5,853 | 0 | 0 | 0 | 0.4 s | 5.7 KB |
| private-rust-app | baseline | 784 | 28,944 | 1,676 | 67,459 | 36,882 | 1.4 s | 181 KB |
| | now | 377† | 16,468 | 0 | 0 | 14 | 1.2 s | 8.3 KB |
| vscode `src/` | baseline | 6,051 | 320,833 | 56,838 | 196,578 | 114,453 | 6.7 s | 504 KB |
| | now | 6,339 | 224,215 | 0 | 0 | 0 | 9.7 s | 9.3 KB |

† the configured `exclude` globs (`vendor/`, `build/`, `dist/`, …) were
never applied before; they are now. Fewer symbols overall because local
variables inside functions are no longer indexed as symbols.

Baseline call-edge counts were inflated by name-only linking (every `new()`,
`len()`, `get()` linked to an arbitrary project method), which is why the
current counts are lower on large repos while accuracy is far higher.

Embedding all symbols with the transformer model costs extra time on a full
index (≈120 s for vscode on CPU; incremental updates embed only changed
files). Builds with `--no-default-features` skip it and use lexical search.

## Compared with ripgrep and ast-grep

`bench/compare/compare.py`, raw numbers in `bench/results/compare.json`.
ripgrep 14.1, ast-grep 0.51.1, adaptive-codegraph at this commit.

Each tool is asked the way an agent would ask it:

- **ripgrep**: `rg -n -w <name>` over the language's files.
- **ast-grep**: call patterns `name($$$)`, `$O.name($$$)`, plus
  `$P::name($$$)` (Rust), `new name($$$)` (TypeScript), `name{$$$}` /
  `$P.name{$$$}` (Go composite literals): 3-4 searches per question.
- **adaptive-codegraph**: `callers <Type::name>` / `impact <Type::name>
  --depth 4`. If the name is unknown (fixture ids such as `util::checksum`
  are module-style), it retries with `file::name`; if it is ambiguous, it
  retries with the ID of the candidate at the definition. Each retry counts
  as a call.

Scoring: every `file:line` a tool returns is credited to the function that
contains it (the nearest `@def` marker above it), then compared with the
`caller -> callee` edges in `expected.txt` (7 fixtures, 115 edges). This rule
credits a call inside a nested function defined before it to the nested
function. It costs every tool the same one edge (`run -> run::inner`).

For impact, grep and ast-grep go hop by hop: search the callee's name, map
the hits to functions, and search those names, up to 4 hops. Truth is the
reverse transitive closure of the expected edges.

| Who calls X? | Precision | Recall | Tool calls | Output |
|---|---|---|---|---|
| ripgrep | 0.35 | 0.96 | 84 | 19.4 KB |
| ast-grep | 0.74 | 0.80 | 238 | 6.0 KB |
| adaptive-codegraph | 0.99 | 0.97 | 98 | 8.1 KB |

| Impact, 4 hops | Precision | Recall | Tool calls | Output |
|---|---|---|---|---|
| ripgrep | 0.37 | 0.99 | 426 | 55.6 KB |
| ast-grep | 0.65 | 0.84 | 801 | 12.9 KB |
| adaptive-codegraph | 0.88* | 0.92 | 98 | 20.5 KB |

\* The default `impact` also follows dynamic dispatch (overriding methods,
interface callers), which the call-only ground truth does not count.

**Real repository** (ast-grep at `cab768cd`, copied to a temp folder; 25
non-test functions sampled with a fixed seed; no ground truth), average per
"who calls X?" question:

| | Lines | Bytes | ~Tokens | Time | Calls |
|---|---|---|---|---|---|
| ripgrep | 24.3 | 2135 | 534 | 47 ms | 1.0 |
| ast-grep | 7.2 | 662 | 165 | 367 ms | 3.0 |
| adaptive-codegraph | 1.1 | 208 | 52 | 255 ms | 1.1 |

Index build for codegraph: ~15 s once (with embeddings). Spot checks of the
sample: where ast-grep returns many more hits (`potential_kinds` 38,
`get_matcher` 38), they are calls to other methods with the same name, or
calls through a trait, which codegraph reports under `impact` (dispatch)
rather than as static callers of that implementation.

Building this comparison found one codegraph bug, now fixed:
`file.rs::method` did not find a method unless its type was named too
(`file.rs::Type::method`).
