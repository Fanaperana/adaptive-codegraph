# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-16

### Added

- **Core indexing pipeline**: tree-sitter based symbol extraction for 6 languages
  (Rust, Python, TypeScript, JavaScript, Go, C).
- **Symbol types**: functions, classes, structs, enums, enum variants, traits,
  impls, interfaces, type aliases, macros, constants, statics, fields, variables,
  modules, namespaces, unions, and decorated definitions.
- **Edge extraction**: call graphs, import relationships via `.scm` query files.
- **BM25 full-text search** via Tantivy for symbol names, fqnames, file paths,
  and signatures.
- **Vector similarity search** via fastembed (BGE-small-en-v1.5) with BLAKE3
  hash fallback for zero-dependency builds.
- **In-memory graph store** with petgraph, O(1) lookup by ID/file/name, and
  bincode serialization.
- **BFS neighborhood expansion** for graph exploration queries.
- **Incremental reindexing** using git diff or mtime fallback.
- **MCP server** (`adaptive-codegraph-mcp`) with JSON-RPC over stdio, exposing:
  `search`, `find_symbol`, `get_symbol`, `find_callers`, `find_callees`,
  `expand_neighborhood`, `semantic_search`, `index`, `incremental_index`,
  `index_status`.
- **CLI** (`adaptive-codegraph`) with subcommands: `init`, `index`, `search`,
  `find`, `semantic-search`, `add-language`.
- **File-watching daemon** (`adaptive-codegraph-daemon`) for automatic
  incremental reindexing on file changes.
- **Compile-time embedded languages**: all `.toml` and `.scm` files embedded
  via `include_str!`, no external `languages/` directory needed at runtime.
- **`init` subcommand**: creates `.adaptive-codegraph/` project directory with
  language configs and VS Code MCP integration (`mcp.json`).
- **Edge case test fixtures** for all 6 languages in `tests/fixtures/`.
- **CI pipeline** via GitHub Actions: check, test, fmt, clippy, release build.

### Language Query Coverage

- **Rust**: functions (async/unsafe/const/extern), structs, enums + variants,
  traits, impls, modules, macros, fields, constants, statics.
- **Python**: functions, classes (including nested), decorated definitions,
  assignments.
- **TypeScript**: functions, classes, interfaces, enums + variants, type aliases,
  namespaces/modules, abstract methods, public fields, exported declarations.
- **JavaScript**: functions (including generators), classes, field definitions,
  lexical declarations (const/let), exported defaults.
- **Go**: functions, methods, type declarations (structs/interfaces), fields,
  var/const declarations.
- **C**: functions, structs, enums + enumerators, unions, typedefs, macros
  (`#define`), global declarations, struct fields.

[Unreleased]: https://github.com/Fanaperana/adaptive-codegraph/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Fanaperana/adaptive-codegraph/releases/tag/v0.1.0
