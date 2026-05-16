<div align="center">

# 🧬 adaptive-codegraph

**Language-agnostic code graph indexer, search engine, and MCP server**

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MCP](https://img.shields.io/badge/MCP-2024--11--05-blueviolet)](https://modelcontextprotocol.io/)
[![tree-sitter](https://img.shields.io/badge/tree--sitter-0.25-green)](https://tree-sitter.github.io/)

*Add a new language by dropping a `.toml` + `.scm` file — no Rust code changes needed.*

[How It Works](#-how-it-works-under-the-hood) · [Why Not grep?](#-why-not-just-grep) · [Quick Start](#-quick-start) · [Adding a Language](#-adding-a-new-language) · [MCP Tools](#%EF%B8%8F-mcp-tools) · [CLI](#-cli-usage) · [Architecture](#%EF%B8%8F-architecture) · [Full Docs](DOCS.md)

</div>

---

## 💡 Why?

A similar implementation to [mie-codegraph](https://github.com/mieweb/mie-codegraph) that replaces **hardcoded per-language extractors** with **tree-sitter query files**. The result:

| | mie-codegraph | adaptive-codegraph |
|---|---|---|
| Add a language | Write a Rust extractor (~200 LOC) | Drop 2 `.scm` files + 1 `.toml` |
| Extraction engine | Mixed tree-sitter + regex | Pure tree-sitter queries |
| Symbol/Edge kinds | Rust enums (recompile to add) | **Strings** (no recompile) |
| Language detection | Hardcoded | **Auto-detect** from marker files |
| Plugin edges | Hardcoded patterns | **Regex plugin system** |
| WebChart-specific | Yes | **Generic** — works on any codebase |

## 📊 Status

| Component | Status |
|-----------|--------|
| Core library (extract, store, search, embed) | ✅ Complete |
| CLI (index, search, callers, callees, neighborhood) | ✅ Complete |
| MCP server (JSON-RPC 2.0 over stdio, 8 tools) | ✅ Complete |
| Built-in grammars (C, JS, Rust, Python, TypeScript, Go) | ✅ Complete |
| Fastembed semantic search (optional) | 🔨 Feature flag |
| Daemon (file-watching incremental reindex) | 🔨 Skeleton |

## � How It Works Under the Hood

adaptive-codegraph builds a **structural understanding** of your codebase, not just a text index. Here's what happens when you run `adaptive-codegraph index`:

### 1. Parse — Tree-sitter turns source code into syntax trees

Every source file is parsed with [tree-sitter](https://tree-sitter.github.io/), producing a concrete syntax tree (CST). This is a full structural parse — it knows the difference between a function *definition*, a function *call*, a *variable*, and a *string literal*.

### 2. Extract — Query files pull out symbols and relationships

Tree-sitter S-expression queries (`.scm` files) are run against the syntax tree to extract:

- **Symbols** — function definitions, classes, structs, enums, modules, traits, interfaces, etc.
- **Edges** — function calls, imports, inheritance, trait implementations, etc.

Each symbol gets a **stable ID** computed via BLAKE3 hashing of its language, kind, fully-qualified name, and file path. This ID survives edits that don't rename or move the symbol.

### 3. Build — A directed graph of your entire codebase

All symbols become **nodes** and all relationships become **directed edges** in a [petgraph](https://docs.rs/petgraph) graph, with side-table indexes for O(1) lookup by ID, file path, and name. This gives you:

- **"Who calls this function?"** → walk incoming edges
- **"What does this function call?"** → walk outgoing edges
- **"Show me everything connected to this symbol within 3 hops"** → BFS traversal

### 4. Index — Full-text search with BM25 ranking

Every symbol is indexed into a [Tantivy](https://github.com/quickwit-oss/tantivy) search engine across multiple fields (name, fully-qualified name, file path, signature). Queries are ranked using BM25 — the same algorithm used by Elasticsearch and Lucene — so a function *named* `parse_config` ranks higher than one that merely *contains* those words in a long path.

### 5. Embed — Optional vector search for semantic matching

Symbol names are embedded into vector space for similarity search:

- **Default:** BLAKE3 hash-based embeddings (32-dim, fast, deterministic)
- **With `fastembed`:** BGE-small-en-v1.5 transformer (384-dim, semantic understanding)

With transformer embeddings, a search for `"authenticate user"` can match `login`, `verify_credentials`, and `check_password` even though the words don't overlap.

### 6. Persist — Everything is saved for instant reloads

The graph, search index, and vectors are serialized to `.adaptive-codegraph/` in the project root. Subsequent runs load the index in milliseconds. **Incremental reindexing** uses `git diff` to detect changed files and only re-processes those.

---

## 🆚 Why Not Just `grep`?

`grep` searches text. adaptive-codegraph understands **code structure**.

| | `grep` / `ripgrep` | adaptive-codegraph |
|---|---|---|
| **What it searches** | Raw text / regex patterns | Parsed symbols, relationships, graph structure |
| **"Find the function `parse`"** | Matches every string containing "parse" — comments, variables, imports, documentation | Returns only the **function definition** named `parse` |
| **"Who calls `handle_request`?"** | `grep handle_request` → hundreds of hits, including the definition itself, string literals, comments | `callers "handle_request"` → only the **actual call sites**, with the calling function name and file |
| **"What does `main` depend on?"** | Not possible with grep | `callees "main"` → every function called by `main`, then `neighborhood "main" --depth 3` for the full dependency subgraph |
| **Ranking** | No ranking — results are in file order | **BM25 relevance scoring** — best matches first |
| **Semantic search** | Not possible | `"process authentication"` matches `login()`, `verify_token()` (with fastembed) |
| **Understands language syntax** | No — treats code as plain text | Yes — knows that `def parse():` is a definition and `parse()` is a call |
| **Cross-file relationships** | Manual — you grep, read the result, then grep again | Built-in — the graph connects symbols across all files automatically |
| **Speed on repeated queries** | Re-scans files every time | Index once, query in milliseconds |

### When grep is still the right tool

- Searching for **arbitrary text** (log messages, string literals, comments)
- One-off searches where you don't need an index
- Searching non-code files (docs, configs, data files)

### When adaptive-codegraph is better

- Understanding **code structure** — what calls what, what depends on what
- Navigating **large codebases** where grep returns too many irrelevant results
- Powering **AI assistants** (via MCP) that need structural context, not just text matches
- Finding **all callers** of a function across the entire project
- Exploring the **dependency graph** around a symbol
- **Semantic search** — finding code by meaning, not exact text

---

> 📖 **[Full documentation →](DOCS.md)** — CLI reference, MCP tool schemas, config options, data model, plugin system, and more.

---

## �🚀 Quick Start

### Prerequisites

- **Rust 1.75+** (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

### Install Globally

```bash
# Clone & install to ~/.cargo/bin (available system-wide)
git clone https://github.com/Fanaperana/adaptive-codegraph.git
cd adaptive-codegraph
cargo install --path crates/cli
cargo install --path crates/mcp
cargo install --path crates/daemon

# Install language definitions (required for global usage)
cp -a languages/ "$(dirname $(which adaptive-codegraph))"/languages/
```

Now you can use `adaptive-codegraph`, `adaptive-codegraph-mcp`, and `adaptive-codegraph-daemon` from any project directory.

### Usage

```bash
# Go to any project
cd /path/to/your/project

# Index the project (creates .adaptive-codegraph/ in the project root)
adaptive-codegraph index

# Search
adaptive-codegraph search "handle_request"
```

A `.adaptive-codegraph/` folder will be created in the project root to store the index. You can optionally add a `.adaptive-codegraph.toml` config file to customize behavior.

> **Tip:** Add `.adaptive-codegraph/` to your global gitignore so it's ignored across all projects:
> ```bash
> echo ".adaptive-codegraph/" >> ~/.gitignore
> git config --global core.excludesFile ~/.gitignore
> ```

### With Fastembed (Recommended)

```bash
cargo install --path crates/cli --features fastembed
cargo install --path crates/mcp --features fastembed
```

Adds BGE-small-en-v1.5 (~33MB model, downloaded on first use) for **high-quality semantic search**. Without this, vector search falls back to BLAKE3 hash-based embeddings which only match similar *names*, not similar *meanings*. With fastembed, searching `"validate input"` can match `sanitize_params()`, `check_user_data()`, etc.

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     adaptive-codegraph                       │
├─────────┬─────────┬───────────┬────────────────────────────┤
│   CLI   │   MCP   │  Daemon   │  (future: LSP, web UI)     │
├─────────┴─────────┴───────────┴────────────────────────────┤
│                        Core Library                         │
│  ┌──────────┐ ┌──────────┐ ┌────────────┐ ┌────────────┐  │
│  │ Extract  │ │  Store   │ │   Search   │ │   Embed    │  │
│  │ (TS+SCM) │ │ (Graph)  │ │  (Tantivy) │ │(HNSW/Hash) │  │
│  └──────────┘ └──────────┘ └────────────┘ └────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌────────────┐ ┌────────────┐  │
│  │  Config  │ │  Index   │ │Incremental │ │   Query    │  │
│  │(auto-det)│ │(pipeline)│ │ (git-aware)│ │  (BFS etc) │  │
│  └──────────┘ └──────────┘ └────────────┘ └────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                    Language Definitions                      │
│  languages/*.toml + languages/queries/*.scm                 │
│  (C, JavaScript, Rust, Python, TypeScript, Go, ...)         │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **String-based SymbolKind/EdgeKind** | No enum changes when adding a language |
| **Tree-sitter `.scm` queries** | Add a language by writing query files, not Rust code |
| **Auto-detect languages** | Scan marker files + extensions to know what to index |
| **BLAKE3 symbol IDs** | Stable, deterministic, content-addressable hashing |
| **Tantivy BM25** | Full-text search over names, fqnames, signatures, paths |
| **HNSW vectors** | Semantic search with fastembed (optional) or hash fallback |
| **Plugin system** | Custom edge patterns via regex (Django routes, etc.) |
| **Git-aware incremental** | Only re-extract changed files since last indexed HEAD |

---

## 🌐 Adding a New Language

Three files. Zero Rust changes.

### 1. Language definition — `languages/<lang>.toml`

```toml
id = "ruby"
name = "Ruby"
extensions = ["rb"]
grammar = "builtin"
```

### 2. Symbol extraction — `languages/queries/<lang>.scm`

```scheme
;; Methods
(method name: (identifier) @symbol.name) @symbol.def

;; Classes
(class name: (constant) @symbol.name) @symbol.def
```

### 3. Edge extraction — `languages/queries/<lang>_edges.scm`

```scheme
;; Function calls
(call method: (identifier) @call.name)

;; Imports
(call method: (identifier) @import.path
  (#eq? @import.path "require"))
```

### Built-in Languages

| Language | Grammar | Extensions |
|----------|---------|------------|
| 🇨 C | tree-sitter-c | `.c`, `.h` |
| 📜 JavaScript | tree-sitter-javascript | `.js`, `.jsx`, `.mjs` |
| 🦀 Rust | tree-sitter-rust | `.rs` |
| 🐍 Python | tree-sitter-python | `.py` |
| 🔷 TypeScript | tree-sitter-typescript | `.ts`, `.tsx` |
| 🐹 Go | tree-sitter-go | `.go` |

---

## 🔌 Custom Edge Patterns (Plugins)

For domain-specific edges that tree-sitter queries can't capture (framework routing, layout rendering, etc.):

```rust
use adaptive_codegraph_core::extract::plugin::RegexEdgePattern;

let pattern = RegexEdgePattern {
    name: "django_url".to_string(),
    pattern: regex_lite::Regex::new(r#"path\("([^"]+)",\s*(\w+)"#).unwrap(),
    edge_kind: "endpoint".to_string(),
    from_group: 2,
    to_group: 1,
};
```

---

## 🛠️ MCP Tools

The MCP server exposes these tools over JSON-RPC 2.0 (stdio):

| Tool | Description |
|------|-------------|
| `search` | BM25 full-text search over symbols |
| `semantic_search` | Vector similarity search (fastembed) |
| `find_symbol` | Look up symbol by name substring |
| `get_symbol` | Get symbol details by ID |
| `find_callers` | Functions that call a given symbol |
| `find_callees` | Functions called by a given symbol |
| `expand_neighborhood` | BFS subgraph around a symbol |
| `index` | Full index rebuild |
| `incremental_index` | Re-process only changed files (git-aware) |
| `index_status` | Report index state and staleness |

### VS Code / Copilot Configuration

Add a `.vscode/mcp.json` to your project:

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

---

## 💻 CLI Usage

```bash
# cd into any project, then:

# Full index
adaptive-codegraph index

# Search symbols
adaptive-codegraph search "parse_config"

# Find callers of a function
adaptive-codegraph callers "handle_request"

# Find callees
adaptive-codegraph callees "main"

# BFS neighborhood (depth 3)
adaptive-codegraph neighborhood "main" --depth 3

# List detected languages
adaptive-codegraph languages

# Check index status
adaptive-codegraph status
```

You can also specify a different project with `--base`:

```bash
adaptive-codegraph --base /path/to/other/project index
```

---

## ⚙️ Configuration

Place `.adaptive-codegraph.toml` in your project root. If absent, languages are auto-detected.

### Minimal

```toml
roots = ["src"]
```

### Full

```toml
roots = ["src", "lib"]
index_dir = ".adaptive-codegraph"
exclude = [
  "**/.git/**",
  "**/node_modules/**",
  "**/target/**",
  "**/build/**",
  "**/*.min.js",
]

[[languages]]
id = "c"
extensions = ["c", "h"]

[[languages]]
id = "javascript"
extensions = ["js"]
```

### Config Reference

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `roots` | `string[]` | `["."]` | Directories to index |
| `index_dir` | `string` | `".adaptive-codegraph"` | Index storage location |
| `exclude` | `string[]` | *(common patterns)* | Glob patterns to skip |
| `languages` | `table[]` | *(auto-detected)* | Explicit language list |

### Language Auto-Detection

When `languages` is omitted, the tool scans for marker files:

| Language | Marker Files | Extensions |
|----------|-------------|------------|
| C | `Makefile`, `CMakeLists.txt` | `.c`, `.h` |
| Rust | `Cargo.toml` | `.rs` |
| Python | `pyproject.toml`, `setup.py`, `requirements.txt` | `.py` |
| JavaScript | `package.json` | `.js`, `.mjs`, `.cjs` |
| TypeScript | `tsconfig.json` | `.ts`, `.tsx` |
| Go | `go.mod` | `.go` |
| Java | `pom.xml`, `build.gradle` | `.java` |
| Ruby | `Gemfile` | `.rb` |
| C# | `*.csproj`, `*.sln` | `.cs` |
| C++ | `CMakeLists.txt` | `.cpp`, `.cc`, `.cxx`, `.hpp` |

---

## 📁 Project Layout

```
adaptive-codegraph/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── core/               # Core library (extract, store, search, embed)
│   ├── cli/                # Command-line interface (clap)
│   ├── mcp/                # MCP server (JSON-RPC 2.0 over stdio)
│   └── daemon/             # File-watching daemon (notify-rs)
└── languages/
    ├── c.toml              # Language definitions
    ├── javascript.toml
    ├── rust.toml
    ├── python.toml
    ├── typescript.toml
    ├── go.toml
    └── queries/
        ├── c.scm           # Symbol extraction queries
        ├── c_edges.scm     # Edge extraction queries
        ├── javascript.scm
        ├── javascript_edges.scm
        ├── rust.scm
        ├── rust_edges.scm
        └── ...
```

---

## 📄 License

[MIT](LICENSE)
