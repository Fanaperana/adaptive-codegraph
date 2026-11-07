---
name: codegraph
description: Use the adaptive-codegraph tools to understand code structure before reading or changing code. Use when locating where something is implemented, finding who calls or depends on a function or type, checking what a change could break, reading one function without opening the whole file, getting oriented in an unfamiliar repository, or searching any text (strings, error messages, comments, config, docs). Prefer these over grep/glob: `text_search` replaces grep and groups matches by the function that holds them.
---

# Working with the code graph

The `adaptive-codegraph` MCP server keeps a graph of this repository: symbols
(functions, methods, classes, types, fields), and the relationships between
them (calls, imports, inheritance, interface implementations, overrides, type
usage). It re-indexes changed files automatically before answering.

## Pick the tool by the question

| Question | Tool |
|---|---|
| "Where is X done?" / "Find the code that …" | `search` (names or plain-English description) |
| "I need to work on …" (start of a task) | `context` — best-matching code plus its callers/callees, in one call |
| "What is this symbol and how is it used?" | `explain` |
| "Show me this function" | `source` (one symbol, with line numbers — cheaper than reading the file) |
| "What's in this file?" | `outline` |
| "Who calls this?" | `callers` (`depth` > 1 for transitive; `possible=true` also lists calls the resolver could not link) |
| "What does this call?" | `callees` |
| "What breaks if I change this?" / "Which tests should I run?" | `impact` |
| "How does A end up calling B?" | `call_path` |
| "What's important in this repo?" | `repo_map` |
| "Is this still used?" | `explain` (no callers / used by) or `dead_code` |
| "Where does this string / error message / config key / TODO appear?" | `text_search` (regex or `fixed=true`, `ignore_case`, `word`, `path` prefix or glob) |

Symbols can be passed as `name`, `Type::method`, `Type.method`, a
file-qualified name, or an ID from earlier output. If a name is ambiguous the
tool lists the candidates — pick one and call again.

## Workflow that saves round trips

1. Unfamiliar area: `context` with the task description. Often this is all
   the reading you need.
2. Before editing a function or type: `impact` on it. Update every caller it
   lists; run the tests it lists.
3. Read with `source`/`outline` instead of whole files when you only need one
   symbol.
4. For text the graph does not model (string literals, log messages,
   comments, configuration, non-code files), use `text_search`, not grep:
   same matches as ripgrep, served from an index, and each match names the
   function it is in, so you rarely need to open the file afterwards.

## Limits

- Edges marked `(inferred)` or `?` are lower confidence. Dynamic calls
  (callbacks passed as values, reflection, string-based dispatch) can be
  missing, so "no callers" means "no static callers". When `callers` reports
  `+ N possible`, ask again with `possible=true`, or `text_search` the name.
- `impact` includes callers that reach a method through an interface or base
  class (dynamic dispatch); treat those as "may be affected".
