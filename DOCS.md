# 📖 adaptive-codegraph — Full Documentation

> Language-agnostic code graph indexer, search engine, and MCP server.

---

## Table of Contents

- [Installation](#installation)
- [CLI Reference](#cli-reference)
- [Configuration](#configuration)
- [MCP Server](#mcp-server)
- [Language Definitions](#language-definitions)
- [Plugin System](#plugin-system)
- [Data Model](#data-model)
- [Indexing Pipeline](#indexing-pipeline)
- [Search](#search)
- [Vector / Semantic Search](#vector--semantic-search)
- [Graph Queries](#graph-queries)
- [Incremental Indexing](#incremental-indexing)
- [Daemon](#daemon)
- [Feature Flags](#feature-flags)
- [Environment & Logging](#environment--logging)
- [Troubleshooting](#troubleshooting)

---

## Installation

### From Source (Global)

```bash
git clone https://github.com/Fanaperana/adaptive-codegraph.git
cd adaptive-codegraph

# Install CLI + MCP server globally (~/.cargo/bin)
cargo install --path crates/cli
cargo install --path crates/mcp

# With transformer embeddings (optional, adds ~33MB model)
cargo install --path crates/cli --features fastembed
```

### Verify

```bash
adaptive-codegraph --help
adaptive-codegraph-mcp --help
```

### Build Only (Development)

```bash
cargo build --release
# Binaries at:
#   target/release/adaptive-codegraph
#   target/release/adaptive-codegraph-mcp
```

---

## CLI Reference

```
adaptive-codegraph [OPTIONS] <COMMAND>
```

### Global Options

| Option | Default | Description |
|--------|---------|-------------|
| `--base <PATH>` | `.` (current directory) | Project root directory |

### Commands

#### `index` — Full Index Rebuild

```bash
adaptive-codegraph index
```

Walks all configured roots, extracts symbols and edges from every source file, and builds the graph, search index, and vector index. Results are stored in `.adaptive-codegraph/` inside the project root.

**Output:** Summary of indexed symbols and edges.

---

#### `search <QUERY>` — Full-Text Search

```bash
adaptive-codegraph search "parse_config"
adaptive-codegraph search "handle_request" --limit 10
```

BM25 full-text search across symbol names, fully-qualified names, file paths, and signatures.

| Option | Default | Description |
|--------|---------|-------------|
| `--limit <N>` | `20` | Maximum results to return |

**Output:** Ranked list of matching symbols with name, kind, language, file, and BM25 score.

---

#### `find <NAME>` — Symbol Lookup

```bash
adaptive-codegraph find "UserService"
adaptive-codegraph find "parse" --limit 50
```

Case-insensitive substring search for symbols by name.

| Option | Default | Description |
|--------|---------|-------------|
| `--limit <N>` | `20` | Maximum results to return |

**Output:** Matching symbols with name, kind, language, file, and line range.

---

#### `callers <NAME>` — Find Callers

```bash
adaptive-codegraph callers "handle_request"
```

Lists all symbols that call or reference the given symbol (incoming edges in the graph).

**Output:** Each caller with its name, kind, file, and the edge kind (e.g., `calls`, `imports`).

---

#### `callees <NAME>` — Find Callees

```bash
adaptive-codegraph callees "main"
```

Lists all symbols called or referenced by the given symbol (outgoing edges in the graph).

**Output:** Each callee with its name, kind, file, and the edge kind.

---

#### `neighborhood <NAME>` — BFS Subgraph

```bash
adaptive-codegraph neighborhood "main"
adaptive-codegraph neighborhood "process_request" --depth 3 --cap 100
```

Expands a BFS subgraph around the named symbol, following both incoming and outgoing edges.

| Option | Default | Description |
|--------|---------|-------------|
| `--depth <D>` | `2` | Maximum BFS hops from center |
| `--cap <C>` | `50` | Maximum nodes to visit |

**Output:** All nodes in the subgraph with their location and kind, plus all edges in `from --[kind]--> to` notation. The center symbol is marked `[CENTER]`.

---

#### `status` — Index Status

```bash
adaptive-codegraph status
```

Reports indexing metadata: git HEAD at last index time, timestamp, file count, symbol count, and edge count.

---

#### `languages` — List Detected Languages

```bash
adaptive-codegraph languages
```

Shows all languages detected in the project (or loaded from `languages/` directory), with their file extensions.

---

### Examples

```bash
# Index the current project
cd ~/projects/my-app
adaptive-codegraph index

# Search for anything related to "auth"
adaptive-codegraph search "auth"

# Who calls the login function?
adaptive-codegraph callers "login"

# What does the main function call?
adaptive-codegraph callees "main"

# Explore the graph around a function
adaptive-codegraph neighborhood "handle_request" --depth 3

# Index a different project
adaptive-codegraph --base ~/projects/other-app index
```

---

## Configuration

Place `.adaptive-codegraph.toml` in your project root. If absent, everything is auto-detected.

### Minimal Config

```toml
roots = ["src"]
```

### Full Config

```toml
roots = ["src", "lib", "app"]
index_dir = ".adaptive-codegraph"
exclude = [
  "**/.git/**",
  "**/node_modules/**",
  "**/target/**",
  "**/build/**",
  "**/__pycache__/**",
  "**/.venv/**",
  "**/*.min.js",
  "**/vendor/**",
  "**/dist/**",
]

[[languages]]
id = "python"
extensions = ["py"]

[[languages]]
id = "javascript"
extensions = ["js", "mjs", "cjs"]
```

### Config Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `roots` | `string[]` | Auto-detected (`src`, `lib`, `app`, `pkg`, `cmd`, `internal`) or `["."]` | Directories to index |
| `index_dir` | `string` | `".adaptive-codegraph"` | Where index data is stored |
| `exclude` | `string[]` | Common patterns (`.git`, `node_modules`, `target`, etc.) | Glob patterns to skip |
| `languages` | `table[]` | Auto-detected from marker files | Explicit language list |

### Language Auto-Detection

When `languages` is not specified, the tool scans the project root for marker files:

| Language | Marker Files | Extensions |
|----------|-------------|------------|
| C | `Makefile`, `CMakeLists.txt` | `.c`, `.h` |
| C++ | `CMakeLists.txt` | `.cpp`, `.cc`, `.cxx`, `.hpp` |
| C# | `*.csproj`, `*.sln` | `.cs` |
| Go | `go.mod` | `.go` |
| Java | `pom.xml`, `build.gradle`, `build.gradle.kts` | `.java` |
| JavaScript | `package.json` | `.js`, `.mjs`, `.cjs` |
| Python | `pyproject.toml`, `setup.py`, `requirements.txt` | `.py` |
| Ruby | `Gemfile` | `.rb` |
| Rust | `Cargo.toml` | `.rs` |
| TypeScript | `tsconfig.json` | `.ts`, `.tsx` |

### Root Auto-Detection

When `roots` is not specified, the tool checks for common source directories (`src`, `lib`, `app`, `pkg`, `cmd`, `internal`) and uses whichever exist. If none are found, it defaults to `"."` (the project root).

---

## MCP Server

The MCP server implements [Model Context Protocol](https://modelcontextprotocol.io/) version `2024-11-05` over JSON-RPC 2.0 on stdio.

### Setup in VS Code / Copilot

Add to `.vscode/settings.json`:

```json
{
  "mcp": {
    "servers": {
      "adaptive-codegraph": {
        "command": "adaptive-codegraph-mcp",
        "args": ["--base", "${workspaceFolder}"]
      }
    }
  }
}
```

### Tools

#### `search`

BM25 full-text search over symbols.

```json
{
  "query": "handle_request",
  "limit": 20
}
```

**Returns:** Array of `{ id, name, kind, lang, file, score }`

---

#### `semantic_search`

Vector similarity search using embeddings.

```json
{
  "query": "process user authentication",
  "limit": 10
}
```

**Returns:** Array of `{ id, name, kind, lang, file, score }` ranked by cosine similarity.

> Requires the `fastembed` feature for high-quality results. Falls back to BLAKE3 hash-based similarity otherwise.

---

#### `find_symbol`

Substring search with optional filters.

```json
{
  "needle": "parse",
  "kind": "function",
  "lang": "rust",
  "limit": 20
}
```

| Parameter | Required | Description |
|-----------|----------|-------------|
| `needle` | Yes | Substring to search for |
| `kind` | No | Filter by symbol kind (`function`, `class`, `struct`, etc.) |
| `lang` | No | Filter by language (`rust`, `python`, etc.) |
| `limit` | No | Max results (default: 20) |

**Returns:** Array of `{ id, name, kind, lang, file, start_line, end_line, signature }`

---

#### `get_symbol`

Lookup a single symbol by its stable hex ID.

```json
{
  "id": "a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8"
}
```

**Returns:** `{ id, name, kind, lang, file, fqname, start_line, end_line, signature, doc }`

---

#### `find_callers`

All symbols that call/reference a given symbol.

```json
{
  "id": "a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8"
}
```

**Returns:** Array of `{ id, name, kind, file, edge_kind }`

---

#### `find_callees`

All symbols called/referenced by a given symbol.

```json
{
  "id": "a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8"
}
```

**Returns:** Array of `{ id, name, kind, file, edge_kind }`

---

#### `expand_neighborhood`

BFS subgraph expansion.

```json
{
  "id": "a1b2c3d4e5f6a7b8a1b2c3d4e5f6a7b8",
  "depth": 2,
  "cap": 50
}
```

| Parameter | Required | Default | Description |
|-----------|----------|---------|-------------|
| `id` | Yes | — | Symbol ID (32-hex) |
| `depth` | No | `2` | Max BFS hops |
| `cap` | No | `50` | Max nodes |

**Returns:** `{ text, node_count, edge_count }` — formatted graph summary.

---

#### `index`

Trigger a full index rebuild.

```json
{}
```

**Returns:** `{ symbols, edges, status: "ok" }`

---

#### `index_status`

Check the current index state.

```json
{}
```

**Returns:** `{ status, git_head?, indexed_at?, file_count?, symbols?, edges? }`

Status is `"no_index"` if the project has never been indexed.

### Error Codes

| Code | Meaning |
|------|---------|
| `-32700` | Parse error (malformed JSON) |
| `-32600` | Invalid JSON-RPC request |
| `-32601` | Method not found |
| `-32602` | Invalid parameters |
| `-32603` | Internal server error |

---

## Language Definitions

Languages are defined by three files. No Rust code changes required.

### 1. Definition File — `languages/<lang>.toml`

```toml
id = "ruby"
name = "Ruby"
extensions = ["rb"]
grammar = "builtin"
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique identifier (used in queries, edges, output) |
| `name` | `string` | Human-readable name |
| `extensions` | `string[]` | File extensions (without dot) |
| `grammar` | `string` | `"builtin"` for compiled-in grammars |

### 2. Symbol Query — `languages/queries/<lang>.scm`

Tree-sitter S-expression query that extracts symbol definitions.

**Capture names:**

| Capture | Required | Purpose |
|---------|----------|---------|
| `@symbol.name` | Yes | The identifier node of the symbol |
| `@symbol.def` | Yes | The entire definition node (used for span) |

**Example (Python):**

```scheme
(function_definition name: (identifier) @symbol.name) @symbol.def
(class_definition name: (identifier) @symbol.name) @symbol.def
(decorated_definition
  definition: (function_definition name: (identifier) @symbol.name)) @symbol.def
```

### 3. Edge Query — `languages/queries/<lang>_edges.scm`

Tree-sitter query that extracts call sites and imports.

**Capture names:**

| Capture | Purpose |
|---------|---------|
| `@call.name` | A function/method being called |
| `@import.path` | An import or include path |

**Example (Python):**

```scheme
(call function: (identifier) @call.name)
(call function: (attribute attribute: (identifier) @call.name))
(import_from_statement module_name: (dotted_name) @import.path)
(import_statement name: (dotted_name) @import.path)
```

### Symbol Kind Inference

The kind is automatically inferred from the tree-sitter node type:

| Node Type Pattern | Inferred Kind |
|-------------------|---------------|
| `function_definition`, `function_declaration`, `method_definition` | `function` |
| `class_definition`, `class_declaration` | `class` |
| `struct_item`, `struct_specifier` | `struct` |
| `enum_item`, `enum_specifier` | `enum` |
| `trait_item` | `trait` |
| `impl_item` | `impl` |
| `module`, `mod_item` | `module` |
| `type_alias`, `type_item` | `type_alias` |
| `interface_declaration` | `interface` |
| *(anything else)* | `definition` |

### Built-in Grammars

These tree-sitter grammars are compiled into the binary:

| Grammar | Crate |
|---------|-------|
| C | `tree-sitter-c` 0.24 |
| JavaScript | `tree-sitter-javascript` 0.25 |
| Rust | `tree-sitter-rust` 0.24 |
| Python | `tree-sitter-python` 0.25 |
| Go | `tree-sitter-go` 0.25 |
| TypeScript / TSX | `tree-sitter-typescript` 0.23 |

---

## Plugin System

For domain-specific edges that tree-sitter queries alone can't capture (framework routing, template rendering, etc.), you can use regex-based edge patterns.

### RegexEdgePattern

```rust
use adaptive_codegraph_core::extract::plugin::{RegexEdgePattern, PluginRegistry};

let mut plugins = PluginRegistry::new();

plugins.register(RegexEdgePattern {
    name: "django_urls".to_string(),
    pattern: regex_lite::Regex::new(r#"path\("([^"]+)",\s*(\w+)"#).unwrap(),
    edge_kind: "endpoint".to_string(),
    from_group: 2,   // capture group index for the source symbol
    to_group: 1,     // capture group index for the target
});
```

### EdgePattern Trait

Implement custom edge extraction logic:

```rust
pub trait EdgePattern: Send + Sync {
    fn name(&self) -> &str;
    fn apply(
        &self,
        file_path: &str,
        content: &str,
        result: &mut ExtractionResult,
        symbol_index: &HashMap<String, SymbolId>,
    );
}
```

### Use Cases

| Pattern | Edge Kind | Example |
|---------|-----------|---------|
| Django URL routes | `endpoint` | `path("users/", UserListView)` |
| Flask routes | `endpoint` | `@app.route("/api/users")` |
| Template includes | `renders` | `{% include "header.html" %}` |
| Event handlers | `handles` | `@EventHandler("user.created")` |

---

## Data Model

### SymbolId

A stable 128-bit content-addressed identifier.

**Computation:**

$$\text{SymbolId} = \text{BLAKE3}(\texttt{lang} + \texttt{":"} + \texttt{kind} + \texttt{":"} + \texttt{fqname} + \texttt{":"} + \texttt{file})[0..16]$$

- Deterministic — same symbol always gets the same ID
- Stable across edits that don't rename or move the symbol
- Displayed as a 32-character hex string

### Symbol

| Field | Type | Description |
|-------|------|-------------|
| `id` | `SymbolId` | Stable 128-bit content-addressed hash |
| `lang` | `String` | Language identifier (`"rust"`, `"python"`, etc.) |
| `kind` | `String` | Symbol kind (`"function"`, `"class"`, `"struct"`, etc.) |
| `name` | `String` | Short display name (e.g., `"process_patient"`) |
| `fqname` | `String` | Fully-qualified name (e.g., `"src/patient.py::Patient::process"`) |
| `file` | `String` | Workspace-relative file path |
| `span` | `Span` | `{ start_byte, end_byte, start_line, end_line }` |
| `signature` | `Option<String>` | Function/method signature (first line of definition) |
| `doc` | `Option<String>` | Doc comment or docstring |

### Edge

| Field | Type | Description |
|-------|------|-------------|
| `from` | `SymbolId` | Source symbol |
| `to` | `SymbolId` | Target symbol |
| `kind` | `String` | Relationship type (`"calls"`, `"imports"`, `"inherits"`, etc.) |

### UnresolvedEdge

During extraction, some edges can't be immediately resolved to symbol IDs (the target symbol might be in another file). These are stored temporarily and resolved during the indexing pipeline.

| Field | Type | Description |
|-------|------|-------------|
| `from` | `SymbolId` | Source symbol (known) |
| `to_name` | `String` | Target symbol name (to be resolved) |
| `kind` | `String` | Relationship type |

---

## Indexing Pipeline

### Full Index (`adaptive-codegraph index`)

```
1. Load Config
   ├── Read .adaptive-codegraph.toml (or auto-detect)
   └── Resolve language definitions + tree-sitter grammars

2. File Collection
   ├── Walk configured roots (using `ignore` crate, respects .gitignore)
   ├── Filter by registered file extensions
   └── Apply exclude patterns

3. Parallel Extraction (Rayon)
   ├── For each file:
   │   ├── Select extractor by extension
   │   ├── Parse with tree-sitter
   │   ├── Run symbol query → extract Symbols
   │   ├── Run edge query → extract Edges + UnresolvedEdges
   │   └── Run plugin patterns → additional edges
   └── Collect all ExtractionResults

4. Graph Building
   ├── Insert all Symbols into Store (petgraph + side indexes)
   ├── Resolve UnresolvedEdges by name-lookup (best-effort)
   └── Insert all Edges

5. Search Index (Tantivy)
   ├── Create/open index at .adaptive-codegraph/tantivy/
   ├── Index every symbol (name, fqname, file, signature, kind, lang)
   └── Commit

6. Vector Index
   ├── Select embedder (Transformer or Hash)
   ├── Batch embed all symbol names
   └── Save to .adaptive-codegraph/vectors.bin

7. Persist
   ├── Save graph to .adaptive-codegraph/graph.bin (bincode)
   ├── Save vectors to .adaptive-codegraph/vectors.bin (bincode)
   └── Save state to .adaptive-codegraph/state.json
       (git HEAD, timestamp, file count)
```

### Index Files

The `.adaptive-codegraph/` directory contains:

| File | Format | Contents |
|------|--------|----------|
| `graph.bin` | bincode | Serialized graph (symbols + edges) |
| `vectors.bin` | bincode | Vector index (embeddings + IDs) |
| `state.json` | JSON | Index metadata (git HEAD, timestamp, counts) |
| `tantivy/` | Tantivy | BM25 full-text search index files |

---

## Search

### BM25 Full-Text Search

Powered by [Tantivy](https://github.com/quickwit-oss/tantivy).

**Indexed fields:**

| Field | Type | Description |
|-------|------|-------------|
| `name` | TEXT | Short symbol name |
| `fqname` | TEXT | Fully-qualified name |
| `file` | TEXT | File path |
| `signature` | TEXT | Function/method signature |
| `kind` | STRING | Symbol kind (exact match filter) |
| `lang` | STRING | Language (exact match filter) |

A search query is matched against `name`, `fqname`, `file`, and `signature` simultaneously. Results are ranked by BM25 relevance score.

### How BM25 Works

BM25 (Best Matching 25) is a probabilistic ranking function that scores documents based on:

- **Term frequency** — how often the query term appears in a field
- **Inverse document frequency** — rarer terms get higher weight
- **Field length normalization** — shorter fields (like `name`) get boosted

This means searching `"parse_config"` will rank a symbol *named* `parse_config` higher than one that merely *mentions* it in a long file path.

---

## Vector / Semantic Search

### Hash Embedder (Default)

Always available, zero dependencies. Uses BLAKE3 to hash symbol names into 32-dimensional vectors.

- Deterministic and fast
- Useful for fuzzy name matching
- Not truly "semantic" — similar *names* score higher, not similar *meanings*

### Transformer Embedder (Optional)

Enabled with `--features fastembed`. Uses BGE-small-en-v1.5 (~33MB) to produce 384-dimensional embeddings.

- True semantic search — `"authenticate user"` matches `"login"`, `"verify_credentials"`, etc.
- Model is downloaded on first use and cached
- Falls back to hash embedder if download fails

### Similarity

Results are ranked by cosine similarity:

$$\text{score} = \frac{\vec{q} \cdot \vec{s}}{|\vec{q}| \cdot |\vec{s}|}$$

where $\vec{q}$ is the query embedding and $\vec{s}$ is the symbol embedding.

---

## Graph Queries

### Callers / Callees

Direct graph traversal on the petgraph directed graph.

- **Callers** = incoming edges to a symbol
- **Callees** = outgoing edges from a symbol

Each edge carries a `kind` string (`"calls"`, `"imports"`, `"inherits"`, etc.).

### BFS Neighborhood

Expands outward from a center symbol, following both incoming and outgoing edges.

**Algorithm:**
1. Start with the center node in the queue
2. For each node in the queue, visit all neighbors (both directions)
3. Stop when `depth` hops are reached or `cap` nodes are visited
4. Return the set of visited nodes and edges

**Use case:** Understanding the local context around a function — what it calls, what calls it, and the broader dependency neighborhood.

---

## Incremental Indexing

### Git-Based (Primary)

When git is available:

1. Read `state.json` for the git HEAD at last index time
2. Run `git diff --name-status <prev_head> HEAD` to find changed/deleted files
3. Also check `git diff --name-only` for uncommitted working-tree changes
4. Filter by registered file extensions

### Mtime-Based (Fallback)

When git is unavailable:

1. Read `state.json` for the timestamp of the last index
2. Scan all files and compare modification times
3. Include files modified after the last index timestamp

### Update Process

For **deleted** files:
- Remove all symbols and edges from the graph
- Remove from the search index

For **changed** files:
- Remove old symbols, edges, and search entries
- Re-extract the file
- Insert new symbols, edges, and search entries
- Update vector index

Finally, save updated state, graph, and vectors.

---

## Daemon

**Status:** Skeleton — not yet wired up.

The daemon will:
- Watch the workspace for file changes using the `notify` crate
- Debounce rapid changes
- Trigger incremental reindexing automatically

```bash
adaptive-codegraph-daemon --base /path/to/project
```

---

## Feature Flags

| Flag | Crate | Effect |
|------|-------|--------|
| `fastembed` | `core`, `cli` | Enable transformer embeddings (BGE-small-en-v1.5, 384-dim vectors, ~33MB model download) |

### Build with Features

```bash
# CLI with fastembed
cargo install --path crates/cli --features fastembed

# Or build without install
cargo build --release --features fastembed
```

---

## Environment & Logging

adaptive-codegraph uses `tracing` with an env-filter subscriber. Control log verbosity with the `RUST_LOG` environment variable:

```bash
# Default (warnings only)
adaptive-codegraph index

# Debug logging
RUST_LOG=debug adaptive-codegraph index

# Trace logging (very verbose)
RUST_LOG=trace adaptive-codegraph index

# Filter to specific modules
RUST_LOG=adaptive_codegraph_core::extract=debug adaptive-codegraph index
```

---

## Troubleshooting

### "No languages detected"

The auto-detection didn't find any marker files. Either:
- Add a `.adaptive-codegraph.toml` with explicit `[[languages]]` entries
- Make sure you're running from the project root (or use `--base`)

### "No index found"

Run `adaptive-codegraph index` first. The tool needs an initial full index before search, callers, callees, etc. will work.

### Index is stale

Run `adaptive-codegraph index` again. Incremental indexing will only reprocess changed files.

### MCP server not responding

1. Verify the binary is on your PATH: `which adaptive-codegraph-mcp`
2. Test manually: `echo '{"jsonrpc":"2.0","method":"index_status","params":{},"id":1}' | adaptive-codegraph-mcp --base /path/to/project`
3. Check VS Code settings for correct `command` and `args`

### Large projects are slow to index

- Set specific `roots` in config instead of indexing everything
- Add patterns to `exclude` for generated code, vendor directories, etc.
- Consider using the `fastembed` feature only if you need semantic search — hash embeddings are much faster

### Binary size is large

The release profile already uses `lto = "thin"`, `codegen-units = 1`, and `strip = "symbols"`. The main contributors to size are the six compiled-in tree-sitter grammars.
