# adaptive-codegraph

Language-agnostic code graph indexer, search engine, and MCP server.

> A rewrite of [mie-codegraph](../mie-codegraph/) that replaces hardcoded
> language extractors with **tree-sitter query files** — add a new language by
> dropping a `.toml` + `.scm` file into `languages/`, no Rust code changes needed.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     adaptive-codegraph                       │
├─────────┬─────────┬───────────┬────────────────────────────┤
│  CLI    │  MCP    │  Daemon   │  (future: LSP, web UI)     │
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
│  (Python, Rust, C, TypeScript, Go, Java, ...)               │
└─────────────────────────────────────────────────────────────┘
```

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **String-based SymbolKind/EdgeKind** | No enum changes when adding a language |
| **Tree-sitter `.scm` queries** | Add a language by writing query files, not Rust code |
| **Auto-detect languages** | Scan marker files + extensions to know what to index |
| **BLAKE3 symbol IDs** | Stable, deterministic, fast hashing |
| **Tantivy BM25** | Full-text search over names, fqnames, signatures, paths |
| **HNSW vectors** | Semantic search with fastembed (optional) or hash fallback |
| **Plugin system** | Custom edge patterns via regex (Django routes, WCGetLayout, etc.) |
| **Git-aware incremental** | Only re-extract changed files since last indexed HEAD |

## Workspace Layout

```
adaptive-codegraph/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── core/               # Core library (extract, store, search, embed, ...)
│   ├── cli/                # Command-line interface
│   ├── mcp/                # MCP server (stdio)
│   └── daemon/             # File-watching daemon
└── languages/
    ├── python.toml          # Language definitions
    ├── rust.toml
    ├── c.toml
    ├── typescript.toml
    ├── go.toml
    └── queries/
        ├── python.scm       # Symbol extraction queries
        ├── python_edges.scm  # Edge extraction queries
        ├── rust.scm
        ├── rust_edges.scm
        ├── c.scm
        ├── c_edges.scm
        ├── typescript.scm
        ├── typescript_edges.scm
        ├── go.scm
        └── go_edges.scm
```

## Building

```bash
# Standard build (BLAKE3 hash embeddings)
cargo build --release

# With transformer embeddings (BGE-small-en-v1.5)
cargo build --release --features fastembed
```

## Adding a New Language

1. Create `languages/<lang>.toml`:
   ```toml
   id = "ruby"
   name = "Ruby"
   extensions = ["rb"]
   grammar = "builtin"
   ```

2. Create `languages/queries/<lang>.scm` (symbol extraction):
   ```scheme
   (method name: (identifier) @symbol.name) @symbol.def
   (class name: (constant) @symbol.name) @symbol.def
   ```

3. Create `languages/queries/<lang>_edges.scm` (edge extraction):
   ```scheme
   (call method: (identifier) @call.name)
   (call receiver: (identifier) @import.path)
   ```

4. Add the tree-sitter grammar crate to `Cargo.toml` dependencies.

That's it — no Rust code changes needed for the extraction logic.

## Custom Edge Patterns (Plugins)

For domain-specific edges that tree-sitter queries can't capture (e.g.,
framework routing, layout rendering), use the plugin system:

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

## CLI Usage

```bash
# Full index
adaptive-codegraph --base /path/to/project index

# Search
adaptive-codegraph search "parse_config"

# Find callers
adaptive-codegraph callers "handle_request"

# BFS neighborhood
adaptive-codegraph neighborhood "main" --depth 3

# List detected languages
adaptive-codegraph languages
```

## MCP Tools (Planned)

| Tool | Description |
|------|-------------|
| `search` | BM25 text search over symbols |
| `semantic_search` | Vector similarity search (fastembed) |
| `find_symbol` | Look up symbol by name |
| `find_callers` | Functions that call a symbol |
| `find_callees` | Functions called by a symbol |
| `expand_neighborhood` | BFS subgraph around a symbol |
| `index` | Full rebuild |
| `reindex_changed` | Incremental git-aware reindex |
| `index_status` | Report index state |

## Status

🔨 **In development** — core library is structurally complete, CLI/MCP/daemon
are skeletons ready to be wired up. Tree-sitter grammar integration pending
(need to add specific `tree-sitter-*` crate dependencies).
