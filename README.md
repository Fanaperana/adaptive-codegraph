<div align="center">

# 🧬 adaptive-codegraph

**Language-agnostic code graph indexer, search engine, and MCP server**

[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MCP](https://img.shields.io/badge/MCP-2024--11--05-blueviolet)](https://modelcontextprotocol.io/)
[![tree-sitter](https://img.shields.io/badge/tree--sitter-0.25-green)](https://tree-sitter.github.io/)

*Add a new language by dropping a `.toml` + `.scm` file — no Rust code changes needed.*

[Quick Start](#-quick-start) · [Adding a Language](#-adding-a-new-language) · [MCP Tools](#%EF%B8%8F-mcp-tools) · [CLI](#-cli-usage) · [How It Works](#%EF%B8%8F-architecture)

</div>

---

## 💡 Why?

A rewrite of [mie-codegraph](https://github.com/mieweb/mie-codegraph) that replaces **hardcoded per-language extractors** with **tree-sitter query files**. The result:

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

## 🚀 Quick Start

### Prerequisites

- **Rust 1.75+** (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

### Build & Index

```bash
# Clone
git clone https://github.com/Fanaperana/adaptive-codegraph.git
cd adaptive-codegraph

# Build
cargo build --release

# Index a project
./target/release/adaptive-codegraph --base /path/to/project index

# Search
./target/release/adaptive-codegraph --base /path/to/project search "handle_request"
```

### With Fastembed (Transformer Embeddings)

```bash
cargo build --release --features fastembed
```

Adds BGE-small-en-v1.5 (~33MB model) for high-quality semantic search.

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
| `index_status` | Report index state and staleness |

### VS Code / Copilot Configuration

Add to `.vscode/settings.json`:

```json
{
  "mcp": {
    "servers": {
      "adaptive-codegraph": {
        "command": "/path/to/adaptive-codegraph-mcp",
        "args": ["--base", "${workspaceFolder}"]
      }
    }
  }
}
```

---

## 💻 CLI Usage

```bash
# Full index
adaptive-codegraph --base /path/to/project index

# Search symbols
adaptive-codegraph --base . search "parse_config"

# Find callers of a function
adaptive-codegraph --base . callers "handle_request"

# Find callees
adaptive-codegraph --base . callees "main"

# BFS neighborhood (depth 3)
adaptive-codegraph --base . neighborhood "main" --depth 3

# List detected languages
adaptive-codegraph --base . languages

# Check index status
adaptive-codegraph --base . status
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
