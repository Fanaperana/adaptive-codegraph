# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Indexed text search** (`text_search` MCP tool, `grep` CLI command):
  regex or literal search over every text file (code, config, docs, any
  language), from a trigram index kept current on each call. Same matches as
  ripgrep (`.gitignore` decides what is searched); each match is grouped
  under the function or class that holds it. Files over 4 MB are scanned
  from disk instead of held in memory. CLI output: grouped (default),
  `--flat` (`file:line:col:text`), `--json`, `-l`; flags `-F`, `-i`, `-w`,
  `-g <prefix or glob>`.
- **Possible callers**: `callers` counts calls to the same name that the
  resolver could not link to one target (`+ N possible`); `possible=true` /
  `--possible` lists them. Index format v4 keeps these calls.
- **Global `--json` flag** on the CLI: structured output for `grep` and
  `callers`, `{"command", "text"}` for the other commands.
- **File watcher in the MCP server**: the freshness check walks the tree only
  after a file changed. Warm answers take well under 1 ms; a query right
  after a write still sees the write.
- `ACG_NO_EMBED=1` skips the embedding model (fast indexing of very large
  repositories; lexical search only).
- **Text search benchmark** (`bench/compare/text.py`) against ripgrep and
  git grep: latency, agreement with ripgrep, output size.
- **Whole-project resolver**: calls are resolved through scopes, imports
  (incl. aliases, globs, re-exports, workspace packages), `self`/`this`,
  local variable / parameter / field / loop-variable types, and supertypes.
  Every edge carries a confidence (certain / inferred / guess); ambiguous
  calls are left unlinked instead of guessed.
- **New edge kinds**: `inherits`, `implements`, `overrides`, `references`,
  `renders` (JSX), plus file-level `imports`.
- **Insight**: `impact` (what breaks, which tests), `repo_map` (PageRank),
  `context` (budgeted task bundle), `explain`, `source`, `outline`,
  `call_path`, `dead_code`.
- **Code-aware search**: split identifiers, docs, signatures and body words;
  weighted fields; lenient queries; hybrid lexical + embedding ranking.
- **MCP server rewrite**: 14 name-based tools with plain-text answers,
  server instructions, auto-built and auto-refreshed index, protocol fixes
  (no replies to notifications, `ping`, version negotiation).
- **Claude Code plugin** in `plugin/` and a status-line mod in
  `mods/codegraph-status`, both in the repo's marketplace manifest.
  The status line also shows each graph tool call live (tool, target,
  result size, time).
  Graph tool calls also get a `codegraph` badge on their transcript row, and
  the mod finds the CLI in `~/.cargo/bin` when the app's PATH lacks it.
- **Comparison benchmark** against ripgrep and ast-grep
  (`bench/compare/compare.py`): precision, recall, tool calls and output size.
- Fixed: `file.rs::method` now finds a method without naming its type.
- Safe to run in every project: a cross-process index lock, the index folder
  git-ignores itself, the session-start hook skips non-project folders, and
  `max_files` caps accidental huge indexes. `status` no longer builds an index.
- **TSX** language; Go package and C global scoping via language profiles;
  built-in language definitions used when no `languages/` folder exists.
- **`acg-bench`**: ground-truth call-graph fixtures (design + held-out),
  impact, search and scale benchmarks. Results in `bench/RESULTS.md`.

### Changed

- Tree-sitter queries compile on first use, not at startup: a CLI query on
  an up-to-date index starts in ~30 ms instead of ~190 ms. Errors in a custom
  language's queries now show when its first file is parsed.
- Freshness is checked before every answer (cheap stat-only check), not at
  most every 2 s, so an answer right after an edit includes the edit.
- The file walk runs in parallel.

### Fixed

- Calls were linked by name only (e.g. `iter().find()` → an enum variant
  `Find`); callers were attributed to enclosing modules/impl blocks.
- Duplicate symbol IDs (methods in impl blocks, decorated/exported
  definitions) and duplicate edges.
- Incremental reindex dropped call edges; it now matches a full index.
- Absolute `\\?\` paths in the index; 0-based line numbers in output.
- Search re-indexed duplicates on every full index; `::` in a query errored.
- `--no-default-features` did not remove the embedding model from the CLI,
  MCP server and daemon.
- The daemon could miss changes made during its debounce window.
- `install.sh` did not work on Windows.

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
