<div align="center">

# 🧬 adaptive-codegraph

**A code graph that gives AI assistants a real understanding of your codebase.**

Who calls this? What breaks if I change it? Where is the code that does X?<br/>
Answered from a resolved call graph in one tool call, instead of a dozen greps and file reads.

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MCP](https://img.shields.io/badge/MCP-2025--06--18-blueviolet)](https://modelcontextprotocol.io/)
[![Claude Code plugin](https://img.shields.io/badge/Claude%20Code-plugin-d97757)](#-claude-code-plugin)
[![tree-sitter](https://img.shields.io/badge/tree--sitter-0.25-green)](https://tree-sitter.github.io/)
[![Call-graph precision](https://img.shields.io/badge/call--graph%20precision-1.00-brightgreen)](bench/RESULTS.md)
[![Search MRR](https://img.shields.io/badge/search%20MRR-0.32%E2%86%920.78-brightgreen)](bench/RESULTS.md)

[What it saves](#-what-it-saves-you) · [Quick start](#-quick-start) · [Claude Code plugin](#-claude-code-plugin) · [Tools](#%EF%B8%8F-tools) · [How it works](#-how-it-works) · [Benchmarks](#-benchmarks) · [Add a language](#-adding-a-language)

</div>

---

## ✨ What it does

`adaptive-codegraph` parses your repository with [tree-sitter](https://tree-sitter.github.io/), resolves every call, import, and type reference across files, and serves the result to your AI assistant over [MCP](https://modelcontextprotocol.io/), or to you on the command line.

```text
> impact normalize_type

Changing normalize_type (crates/core/src/extract/treesitter.rs:727) affects 14 symbols in 4 files (4 tests), up to 3 hops.
Depth 1:
  method Resolver::field_type - crates/core/src/resolve.rs:813  [calls normalize_type]
  method TreeSitterExtractor::extract_symbols - crates/core/src/extract/treesitter.rs:120  [calls normalize_type]
  function element_type - crates/core/src/extract/treesitter.rs:648  [calls normalize_type]
Depth 2:
  method Resolver::type_of - crates/core/src/resolve.rs:720  [calls Resolver::field_type]
  ...
Depth 3:
  function update_index - crates/core/src/index.rs:75  [calls (via Extractor::extract) TreeSitterExtractor::extract]
Tests to run:
  crates/core/src/extract/treesitter.rs: normalize_type_cases, element_type_cases
```

- 🎯 **Accurate call graph.** Calls are resolved through scopes, imports, `self`/`this`, variable and field types, inheritance and interfaces, not by matching names. Every edge carries a confidence level, and ambiguous calls are left out rather than guessed.
- 💥 **Impact analysis.** See what depends on a function or type, including callers that reach it through an interface, and which tests to run.
- 🔎 **Search by meaning.** "parse the nth-child argument" finds `parse_nth_child_argument`. Hybrid lexical + embedding ranking.
- 📦 **Context in one call.** Describe a task and get the relevant source plus related signatures, packed to a token budget.
- 🗺️ **Repo map.** A PageRank-ranked overview of the code everything else depends on.
- 🔤 **Replaces grep.** Regex and literal search over every file, served from a trigram index: the same lines ripgrep returns, with each match labelled by the function that holds it.
- ♻️ **Always fresh.** Changed files are re-indexed automatically before each answer (a file watcher keeps this free). Incremental results are identical to a full rebuild.
- 🧑‍💻 **For people too.** Every tool is a CLI command with readable output, ripgrep-style flags, `--flat` for editors and `--json` for scripts.
- 🌐 **Language-agnostic.** Rust, Python, TypeScript/TSX, JavaScript/JSX, Go and C built in. Add a language with one `.toml` and two `.scm` files, no Rust code.

---

## 💰 What it saves you

Measured with the included benchmark ([`bench/RESULTS.md`](bench/RESULTS.md)) on real repositories.

| Task | Without a code graph | With adaptive-codegraph |
|---|---|---|
| **"Who calls X?"** (ground-truth fixtures) | `rg -w X`: finds 96% of callers, but **65% of what it returns is noise** (definitions, imports, same-named methods). ast-grep: 74% precise, 80% recall, 3 patterns per question | `callers X`: **99% precise, 97% recall**, one call |
| **"Who calls X?"** (25 functions in [ast-grep](https://github.com/ast-grep/ast-grep)'s own source) | `rg`: **24 lines / ~530 tokens** per question · ast-grep: **7 lines / ~165 tokens** | **1.2 lines / ~60 tokens**: the callers of *that* function, not of everything with the same name |
| **"Find this text"** (any string, error message, config key; 20 queries each on ast-grep and vscode) | `rg`: 45 ms / 360 ms per search | `text_search`: the **same lines in 40 / 40** queries, **0.3 ms / 8 ms** from the server, each match labelled with its function |
| **"Who calls X?"** (18 hard edge cases: aliases, shadowing, same-named methods, re-exports, macros, callbacks) | `rg`: **0 / 18** exactly right · ast-grep: **5 / 18** | **18 / 18** |
| **"Where's the code that does X?"** (24 natural-language questions) | No direct way: guess keywords, grep, open files | Correct function **#1 for 71%** and **top-5 for 88%** of questions (MRR **0.78**, vs 0.32 for name-only search) |
| **"Show me what I need to work on X"** | Find the file, then read all of it (avg **10 KB**, ~2,500 tokens), then chase callers | `context`: the target's source is in a **~1,200-token** bundle for **88%** of questions, plus its callers/callees' signatures |
| **"What breaks if I change X?"** (4 hops) | Search callers, then their callers, and so on: **426 rg searches** (37% precise) or **801 ast-grep searches** (65% precise) across the fixtures | `impact`: **98 calls in total**, one per question, with transitive dependents and tests to run. Static mode: **precision 1.00, recall 0.97** |
| **Exploring around a symbol** (vscode, 6.3k files) | – | Neighborhood answers average **9 KB** (the old version returned **500 KB**) |

**Why this matters for an AI assistant:** fewer tool calls, fewer tokens spent reading files that don't matter, and no confidently wrong answers built on a bad call graph. Edits come with the full list of callers to update and tests to run.

---

## 🚀 Quick start

```bash
git clone https://github.com/Fanaperana/adaptive-codegraph.git
cd adaptive-codegraph
./install.sh                      # builds and puts the binaries on your PATH
# ./install.sh --no-default-features   # smaller build, lexical search only (no embedding model)
```

Then, in any project:

```bash
adaptive-codegraph search "load the configuration file"
adaptive-codegraph explain Store::save
adaptive-codegraph impact Store::save
```

No setup step is needed. The index is built on first use (stored in `.adaptive-codegraph/`, which is git-ignored automatically) and kept up to date as files change.

---

## 🤖 Claude Code plugin

The plugin wires everything into [Claude Code](https://claude.com/claude-code), for every project:

- the **MCP server** with all the tools below
- a **session-start hook** that refreshes the index and gives Claude a short map of the most important code
- a **skill** that teaches Claude when to use the graph instead of grep
- **commands**: `/impact <symbol>`, `/codemap [path]`, `/explain <symbol>`
- an optional **status-line mod** (`codegraph-status`) showing index health, a `◆ codegraph` badge on every graph tool call in the transcript (yellow while running, blue when done, red on failure), and the live call in the status line, with `/codegraph-status` and `/codegraph-reindex`

```bash
./install.sh   # the plugin calls the installed binaries
```

```text
/plugin marketplace add Fanaperana/adaptive-codegraph
/plugin install adaptive-codegraph@adaptive-codegraph
/plugin install codegraph-status@adaptive-codegraph     # optional status line
```

The session-start hook stays silent outside projects (e.g. a session started in your home folder) and refuses to index more than `max_files` source files. Set `ACG_DISABLE=1` to turn it off.

<details>
<summary><b>Other MCP clients</b> (Claude Desktop, VS Code, Cursor, …)</summary>

```json
{
  "mcpServers": {
    "adaptive-codegraph": {
      "command": "adaptive-codegraph-mcp",
      "args": ["--base", "/path/to/your/project"]
    }
  }
}
```

VS Code (`.vscode/mcp.json`):

```json
{
  "servers": {
    "adaptive-codegraph": {
      "type": "stdio",
      "command": "adaptive-codegraph-mcp",
      "args": ["--base", "${workspaceFolder}"]
    }
  }
}
```

</details>

---

## 🛠️ Tools

Every tool answers in compact plain text sized for a model's context window, takes symbols by name (`save`, `Store::save`, `Store.save`, `src/store.rs::Store::save`) or ID, and lists the candidates when a name is ambiguous.

| MCP tool | CLI | What it answers |
|---|---|---|
| `search` | `search <query>` | Symbols by name or description (hybrid lexical + semantic) |
| `context` | `context <task>` | The most relevant code for a task, packed to a token budget |
| `explain` | `explain <symbol>` | Signature, doc, members, callers, callees, supertypes, implementations, types used |
| `source` | `source <symbol>` | One symbol's source with line numbers |
| `text_search` | `grep <pattern>` | Regex or literal search over every file (code, config, docs), each match grouped under the function that holds it. Replaces grep |
| `callers` | `callers <symbol> [--depth N] [--possible]` | Call sites, or transitive callers by distance; counts (or lists) calls the resolver could not link |
| `callees` | `callees <symbol>` | What a symbol calls, in order |
| `impact` | `impact <symbol>` | What may break if it changes, and the tests to run |
| `repo_map` | `map` | PageRank-ranked overview of the most depended-on code |
| `outline` | `outline <file>` | All symbols in a file, nested, with line ranges |
| `call_path` | `path <from> <to>` | Shortest dependency chain between two symbols |
| `dead_code` | `dead-code` | Functions and methods nothing uses |
| `neighborhood` | `neighborhood <symbol>` | Symbols and edges within N hops |
| `reindex`, `index_status` | `index`, `status` | Force a re-index / show index health and resolution stats |

The MCP server watches the project's files itself, so answers are always current and a warm query takes well under a millisecond. The daemon (`adaptive-codegraph-daemon --base .`) is optional: it re-indexes in the background, so even the first CLI query after a big change is fast.

### On the command line, with or without an AI

Every tool is also a CLI command, so you can use the graph directly in a terminal, in scripts or in your editor:

```bash
adaptive-codegraph grep "index format"                 # matches grouped by function
adaptive-codegraph grep -i -w todo -g '*.rs'           # ignore case, whole word, only Rust files
adaptive-codegraph grep -F 'max_file_bytes' --flat     # file:line:col:text, for editors and pipes
adaptive-codegraph grep 'fn\s+parse_\w+' -l            # only the names of matching files
adaptive-codegraph callers Store::save --possible      # also list calls it could not link
adaptive-codegraph impact Store::save --depth 3
adaptive-codegraph --json callers Store::save | jq '.callers[].caller.name'
```

`grep` takes ripgrep-style flags (`-F` literal, `-i`, `-w`, `-g` path prefix or glob, `-l`) and searches the same files ripgrep does (`.gitignore` decides). Default output groups matches by file and then by the function or class that holds each one:

```text
1 matching line in 1 file (7 of 142 files scanned):
crates/core/src/store.rs
  Store::deserialize (method, 324-347)
    328: "index format v{version} is outdated (need v{FORMAT_VERSION}); ..."
```

`--json` works on every command: `grep` and `callers` give structured results (file, line, column, text, and the enclosing symbol with its ID); the other commands give `{"command": ..., "text": ...}`.

---

## 🔬 How it works

```mermaid
flowchart LR
    A[Source files] -->|tree-sitter + .scm queries| B[Per-file facts<br/>symbols · call sites · imports · local types]
    B -->|whole-project resolver| C[Code graph<br/>calls · imports · inherits · implements<br/>overrides · references · renders]
    C --> D[Insight<br/>impact · context · repo map · paths]
    B --> E[Search index<br/>BM25 on names, docs, bodies<br/>+ embeddings]
    D --> F((MCP · CLI · plugin))
    E --> F
```

1. **Extract** (per file, in parallel). Tree-sitter queries pull out definitions, plus raw facts: each call site with its qualifier (`Type::f`) or receiver (`obj.f`), imports with aliases, and the declared or inferred types of parameters, locals, fields and loop variables.
2. **Resolve** (whole project). Each call is matched by:
   - enclosing scopes, then imports (incl. globs, re-exports and workspace packages from `Cargo.toml` / `go.mod` / `package.json`)
   - for method calls: `self`/`this`, the receiver's inferred type, and members inherited through supertypes and interfaces

   Edges get a confidence: **certain**, **inferred**, or **guess**. When several targets are plausible, the call is left unlinked.
3. **Derive.** Inheritance and interface links, method overrides, type references, file-level imports.
4. **Index for search.** Names split into words (`parseHTTPRequest` → `parse http request`), doc comments, signatures and body words, stemmed and weighted, plus BGE-small embeddings.
5. **Stay fresh.** Changed files are found by mtime/size (no git needed) and re-parsed, then resolution re-runs over the stored facts (about a second on vscode). That's why incremental results equal a full rebuild.

<details>
<summary><b>Key design decisions</b></summary>

| Decision | Why |
|---|---|
| Tree-sitter `.scm` queries with a small capture vocabulary | Adding a language is query files, not Rust code |
| Extraction and resolution as separate stages | Parsing is the expensive part and stays incremental; resolution is cheap and global, so results are always consistent |
| Confidence on every edge, ambiguity left unlinked | A missing edge is visible; a wrong edge misleads |
| String-based symbol and edge kinds | New kinds without recompiling |
| BLAKE3 symbol IDs over (lang, kind, qualified name, file) | Stable across edits that don't rename or move a symbol |
| Plain-text tool output with 1-based `file:line` | Cheap for models to read and act on |

</details>

---

## 📊 Benchmarks

`acg-bench` (in `crates/bench`) measures accuracy against hand-labelled ground truth. *Held-out* fixtures were written after the resolver, with patterns it wasn't built for, so they're the honest numbers.

| | Before | Now |
|---|---|---|
| Call edges, all fixtures (precision / recall) | 0.45 / 0.43 | **1.00 / 0.98** |
| Call edges, held-out fixtures | ~0.22 / ~0.24 | **1.00 / 0.94** |
| Incremental vs full re-index | lost 5–8 edges per fixture | **identical** |
| Impact analysis, static (precision / recall) | – | **1.00 / 0.97** |
| Search MRR, 24 questions on ast-grep | 0.32 | **0.56** lexical · **0.78** hybrid |
| Duplicate IDs / duplicate edges on vscode `src/` | 57k / 197k | **0 / 0** |
| Full index, vscode `src/` (6.3k files, lexical) | 6.7 s | 9.7 s (doing far more) |

### Compared with grep and ast-grep

The same questions, asked the way an agent would ask each tool: `rg -w name`; ast-grep call patterns (`name($$$)`, `$O.name($$$)`, `$P::name($$$)` / `new name($$$)`); codegraph `callers` / `impact`. Every `file:line` returned is credited to the function that contains it, then scored against the fixtures' hand-labelled call edges (7 projects, 4 languages, 115 edges).

| Who calls X? | Precision | Recall | Tool calls | Output |
|---|---|---|---|---|
| ripgrep | 0.35 | 0.96 | 84 | 19.4 KB |
| ast-grep | 0.74 | 0.80 | 238 | 6.0 KB |
| **adaptive-codegraph** | **0.99** | **0.97** | 98 | 8.5 KB |
| adaptive-codegraph, with possible callers listed | 0.94 | **0.99** | | |

| What does a change to X affect? (4 hops) | Precision | Recall | Tool calls | Output |
|---|---|---|---|---|
| ripgrep, hop by hop | 0.37 | 0.99 | 426 | 55.6 KB |
| ast-grep, hop by hop | 0.65 | 0.84 | 801 | 12.9 KB |
| **adaptive-codegraph** `impact` | **0.88*** | **0.92** | **98** | 20.5 KB |

On a real repository with no ground truth ([ast-grep](https://github.com/ast-grep/ast-grep) at `cab768cd`, 25 sampled non-test functions), the average answer to "who calls X?":

| | Lines returned | Tokens to read | Time | Tool calls |
|---|---|---|---|---|
| ripgrep | 24.3 | ~534 | 47 ms | 1.0 |
| ast-grep | 7.2 | ~165 | 134 ms | 3.0 |
| **adaptive-codegraph** | **1.2** | **~60** | 64 ms (CLI) · <1 ms (server) | 1.1 |

**How to read this:**
- grep finds nearly everything, but most of what it returns is not a call to *this* function: definitions, imports, comments, and other methods with the same name. The agent has to read and filter all of it.
- ast-grep removes the text noise, but it matches syntax, not meaning: `x.save()` matches every `save` method, and calls through `self.repo`, imports or aliases are either all matched or all missed. Each question needs several patterns.
- codegraph resolves which function a call goes to, so it answers for the exact symbol and gives transitive impact in one call. Its few misses are calls that need flow analysis (see limits below).
- \* `impact` also reports callers that reach X through an interface or base class. These are real effects that the call-only ground truth doesn't count; static mode scores 1.00 / 0.97.
- codegraph builds its index once (~15 s here with embeddings, under 1 s without) and keeps it fresh incrementally. Its CLI times include loading the index; the MCP server keeps it in memory and answers in under a millisecond.
- `callers` also counts calls it could not link to one target (`+ N possible`). Listing them (`possible=true`) raises recall to 0.99, above ripgrep's 0.96, at 0.94 precision.

Reproduce with [`bench/compare/compare.py`](bench/compare/compare.py) (needs `rg`, `ast-grep` and `adaptive-codegraph` on PATH):

```bash
python bench/compare/compare.py --ast-grep ast-grep --repo /path/to/ast-grep --json bench/results/compare.json
```

### Text search compared with ripgrep and git grep

20 queries per repository, generated from the repository itself with a fixed seed (10 identifiers, 5 string literals, TODO / fixme, and 3 regexes, two with no literal to filter on). "Server" is the MCP server an agent talks to; the CLI is a fresh process that loads the index each time.

| Repository | ripgrep | git grep | codegraph CLI | **codegraph server** | Same lines as ripgrep | Files scanned |
|---|---|---|---|---|---|---|
| ast-grep (325 files) | 45 ms | 25 ms | 46 ms | **0.3 ms** | **20 / 20 queries** | 2.0% |
| vscode (14,838 files) | 360 ms | 271 ms | 705 ms | **8 ms** | **20 / 20 queries** | 0.8% |

Median per query. The trigram index means a typical search reads under 2% of the files. The slowest server query on vscode (a regex with no literal, so every file is scanned) took 42 ms, against 367 ms for ripgrep. Where codegraph is slower is the cold CLI on a very large repository, which has to load the whole index first (index build: 0.4 s for ast-grep, 15 s for vscode, without embeddings). Codegraph also returns less text: each line is cut at 240 characters (ripgrep printed 16.5 MB for these vscode queries, codegraph 4.8 MB), though grouping by function makes single-name answers on small repositories about a third larger.

```bash
python bench/compare/text.py --repo /path/to/ast-grep --repo /path/to/vscode --json bench/results/text.json
```

### Edge cases

18 hard "who calls X?" cases in Python, TypeScript and Rust ([`bench/edge/`](bench/edge)), each scored on its own: a case counts only if the tool returns exactly the right callers.

| Edge case (examples) | ripgrep | ast-grep | adaptive-codegraph |
|---|---|---|---|
| Same method name on another type or class (`Store.save` vs `Cache.save`) | wrong extras | wrong extras | ✅ |
| Call through a typed field (`self.store.save`, `this.repo.save`) | wrong extras | wrong extras | ✅ |
| Aliased import (`checksum as cs`), aliased `use` inside a closure | misses the call | misses the call | ✅ |
| Name only in a comment or a string | counted as a caller | ✅ | ✅ |
| Local variable shadows the function; function passed as a callback | wrong caller | wrong caller, callback missed | ✅ |
| `super().load()`, method on `Child()` | wrong extras | wrong extras | ✅ |
| Call inside a macro argument (`println!`), optional chaining (`a?.get()`) | wrong extras | missed | ✅ |
| Default export imported under another name | missed | missed | ✅ |
| **Exact answers** | **0 / 18** | **5 / 18** | **18 / 18** |

Text search was checked the same way against ripgrep on 17 edge cases (Unicode and case folding, CRLF, UTF-8 byte-order mark, UTF-16, a 200 KB line, a file over 4 MB, binary, ignored and hidden files, regexes with no literal, `^$`, a last line without a newline, invalid UTF-8): **the same lines in 17 / 17**. Full tables, and the bugs these cases found (all fixed), are in [`bench/RESULTS.md`](bench/RESULTS.md#edge-cases). These cases were written for this benchmark, so they show what the resolver handles; the held-out fixtures are the unbiased accuracy numbers.

```bash
python bench/compare/edge.py --ast-grep ast-grep --json bench/results/edge.json
```

### Reproduce the accuracy benchmark

```bash
cargo run --release -p adaptive-codegraph-bench -- --label mine --verbose \
  --repo /path/to/some/repo \
  --search-repo /path/to/ast-grep --search-queries bench/search/ast-grep.txt
```

**Known limits:**
- No flow analysis: calls on untyped values that are passed around, or on closure parameters inside iterator chains, are missed.
- Callbacks passed as values and reflection don't produce edges, so "no callers" means "no static callers".
- For airtight answers in a single language, a language server is more precise. This tool trades a little precision for one fast, uniform graph across many languages.

---

## 🌐 Adding a language

Three files, no Rust changes. Put them in `.adaptive-codegraph/languages/` (created by `adaptive-codegraph init`) or the repo's `languages/`.

**`languages/ruby.toml`**

```toml
id = "ruby"
name = "Ruby"
extensions = ["rb"]
grammar = "builtin"
# module_mode = "directory"   # packages are directories (like Go)
# scope = "global"            # top-level names visible everywhere (like C)
```

**`languages/queries/ruby.scm`**: definitions

```scheme
(method name: (identifier) @symbol.name) @symbol.def
(class name: (constant) @symbol.name) @symbol.def
```

**`languages/queries/ruby_edges.scm`**: references, imports, local types

```scheme
(call method: (identifier) @call.name)                                   ; f()
(call receiver: (_) @call.receiver method: (identifier) @call.name)      ; obj.f()
((class superclass: (superclass (constant) @call.name)) (#set! edge "inherits"))
(assignment left: (identifier) @binding.name right: (call receiver: (constant) @binding.type))
```

<details>
<summary><b>Capture reference</b></summary>

| Capture | Meaning |
|---|---|
| `@symbol.name`, `@symbol.def` | A definition's name node and whole node |
| `@symbol.type` | Declared type or return type |
| `@symbol.container` | Owner type when it isn't a syntactic parent (Go receivers) |
| `(#set! kind "...")` | Override the inferred symbol kind |
| `@call.name` + `@call.qualifier` / `@call.receiver` | A reference: `f()`, `T::f()`, `x.f()` |
| `(#set! edge "inherits")`, `(#set! target "macro")` | Edge kind (default `calls`) / restrict target kind |
| `@import.path` + `@import.name` / `@import.alias` / `@import.default` | Import of an item |
| `@import.module` (+ `@import.alias`), `@import.glob` | Whole-module / glob import |
| `@binding.name` + `@binding.type` / `@binding.call` / `@binding.iter` | Typed local, parameter, `self.x` attribute, or loop variable |

`cargo run -p adaptive-codegraph-core --example dump_tree -- <lang> <file>` prints a file's syntax tree with field names, which is handy for writing queries.

</details>

### Built-in languages

| Language | Extensions | Notes |
|---|---|---|
| 🦀 Rust | `.rs` | traits, impls, macros (incl. calls inside macro arguments), workspace crates |
| 🐍 Python | `.py` | classes, inheritance, `super()`, `self.x` attribute types, relative imports |
| 🔷 TypeScript / TSX | `.ts` `.mts` `.cts` / `.tsx` | interfaces, `implements`, re-exports, JSX `renders` edges |
| 📜 JavaScript / JSX | `.js` `.jsx` `.mjs` `.cjs` | `require`, classes, JSX |
| 🐹 Go | `.go` | packages as directories, receivers, embedding |
| 🇨 C | `.c` `.h` | global scope, struct fields, includes |

---

## ⚙️ Configuration

Optional: `.adaptive-codegraph.toml` in the project root.

```toml
roots = ["src", "lib"]                    # what to index (default: everything)
exclude = ["**/generated/**", "**/*.min.js"]
index_dir = ".adaptive-codegraph"
max_file_bytes = 1000000                  # skip larger files
max_files = 60000                         # refuse to index more than this
```

`.gitignore` is always respected. Embeddings are cached per user (`ACG_MODEL_DIR` overrides the location).

| Environment variable | Effect |
|---|---|
| `ACG_NO_EMBED=1` | Skip the embedding model: much faster indexing of very large repositories, lexical search only |
| `ACG_DISABLE=1` | Turn off the session-start hook |
| `ACG_MODEL_DIR` | Where the embedding model is cached |
| `ACG_TIMINGS=1` | Print how long each indexing and loading step takes |

Text search (`grep` / `text_search`) ignores the graph's default excludes such as `build/` and `dist/`, so it finds what ripgrep finds; patterns you add to `exclude` apply to it too.

---

## 📁 Project layout

```text
crates/
  core/     extraction, resolution, store, search, embeddings, insight, workspace service
  cli/      adaptive-codegraph
  mcp/      adaptive-codegraph-mcp (MCP server)
  daemon/   adaptive-codegraph-daemon (file watcher)
  bench/    acg-bench (accuracy, search, impact and scale benchmarks)
languages/  language definitions (.toml + .scm), also compiled into the binary
plugin/     Claude Code plugin (MCP server, hook, skill, commands)
mods/       Claude Code status-line mod
bench/      fixtures, search queries, results
```

See [DOCS.md](DOCS.md) for more detail and [CHANGELOG.md](CHANGELOG.md) for what changed.

## 🤝 Contributing

Issues and PRs are welcome, especially new languages and new benchmark fixtures. See [CONTRIBUTING.md](CONTRIBUTING.md). Please run `cargo test --workspace` and the benchmark before submitting changes to the resolver.

## 📄 License

[MIT](LICENSE)
