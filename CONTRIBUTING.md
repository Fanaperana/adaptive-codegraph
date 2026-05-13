# Contributing

Thanks for your interest in improving adaptive-codegraph. This project aims to
make code graph indexing extensible through configuration and tree-sitter query
files, so small focused contributions are especially welcome.

## Ways to Contribute

- Add or improve language definitions in `languages/`.
- Improve tree-sitter symbol and edge queries.
- Fix bugs in indexing, search, MCP tools, or CLI behavior.
- Improve documentation and examples.
- Add focused tests for extractors and graph behavior.

## Development Setup

```bash
git clone https://github.com/Fanaperana/adaptive-codegraph.git
cd adaptive-codegraph
cargo build
cargo test
```

For release builds:

```bash
cargo build --release
```

For transformer-backed semantic search:

```bash
cargo build --release --features fastembed
```

## Adding a Language

1. Add `languages/<language>.toml`.
2. Add `languages/queries/<language>.scm` for symbols.
3. Add `languages/queries/<language>_edges.scm` for edges.
4. Add or update tests when practical.
5. Document any known extraction limitations.

## Pull Request Guidelines

- Keep changes focused and easy to review.
- Prefer small pull requests over large rewrites.
- Include tests for behavior changes when practical.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` before opening a pull request.
- Update the README or examples when user-facing behavior changes.

## Commit Messages

Use concise commit messages that describe the change, for example:

```text
feat: add ruby query definitions
fix: resolve unresolved call edges by exact symbol name
docs: clarify MCP setup
```

## Reporting Issues

When filing an issue, include:

- The command you ran.
- The expected behavior.
- The actual behavior.
- A small code sample or repository link if the issue involves extraction.
- Your operating system and Rust version.