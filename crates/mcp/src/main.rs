//! # MCP Server for adaptive-codegraph
//!
//! JSON-RPC 2.0 over stdio. Every tool answers in compact plain text sized
//! for a model's context window, takes symbols by name (`save`,
//! `Store::save`, `Store.save`) or ID, and works on a fresh index: the
//! index is built on first use and changed files are re-indexed before
//! answering.

use adaptive_codegraph_core::service::Workspace;
use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "adaptive-codegraph-mcp", version, about = "MCP server for adaptive-codegraph")]
struct Args {
    /// Path to workspace root (default: current directory)
    #[arg(long, default_value = ".")]
    base: String,
}

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
adaptive-codegraph indexes this repository into a code graph (symbols, calls, \
imports, inheritance, type usage) and keeps it up to date as files change. \
Prefer it over grep/glob for questions about code structure:
- Don't know where something is? `search` (natural language or names) or `context` (task description → relevant code in one call).
- Before editing a function or type: `impact` (what depends on it, which tests to run) and `explain` (callers, callees, subtypes, members).
- Reading code: `source` for one symbol, `outline` for a file's structure — cheaper than reading whole files.
- New to the repo: `repo_map` (the most depended-on code, ranked).
- How does A reach B: `call_path`.
- Text that is not a symbol (strings, error messages, comments, config keys, docs, any file type): `text_search` instead of grep. Matches come grouped by the function that contains them.
Symbols can be given as `name`, `Type::method`, `Type.method`, or an ID from earlier output. \
Edges marked (inferred) or ? are lower confidence. Dynamic calls (callbacks, reflection) may be missing.";

#[derive(Serialize)]
struct Resp<'a> {
    jsonrpc: &'a str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(io::stderr)
        .with_target(false)
        .init();

    let args = Args::parse();
    let base = PathBuf::from(&args.base).canonicalize()?;
    let mut ws = Workspace::open(&base)?;
    // Re-check files only after they change (falls back to checking on
    // every call when the platform cannot watch).
    if let Err(e) = ws.watch() {
        tracing::warn!("file watching unavailable, checking files on every call: {e}");
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write(&mut stdout, Value::Null, None, Some(rpc_error(-32700, &format!("parse error: {e}"))))?;
                continue;
            }
        };
        // Notifications (no id) never get a response.
        let Some(id) = msg.get("id").cloned() else { continue };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match dispatch(&mut ws, method, &params) {
            Ok(v) => write(&mut stdout, id, Some(v), None)?,
            Err((code, m)) => write(&mut stdout, id, None, Some(rpc_error(code, &m)))?,
        }
    }
    Ok(())
}

fn rpc_error(code: i64, message: &str) -> Value {
    json!({ "code": code, "message": message })
}

fn write<W: Write>(w: &mut W, id: Value, result: Option<Value>, error: Option<Value>) -> Result<()> {
    let s = serde_json::to_string(&Resp {
        jsonrpc: "2.0",
        id,
        result,
        error,
    })?;
    writeln!(w, "{s}")?;
    w.flush()?;
    Ok(())
}

fn dispatch(ws: &mut Workspace, method: &str, params: &Value) -> std::result::Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
            let version = if PROTOCOL_VERSIONS.contains(&requested) { requested } else { PROTOCOL_VERSIONS[0] };
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "adaptive-codegraph", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools_list() })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or((-32602, "missing tool name".to_string()))?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (text, is_error) = match call_tool(ws, name, &args) {
                Ok(t) => (t, false),
                Err(e) => (format!("Error: {e}"), true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        _ => Err((-32601, format!("unknown method {method}"))),
    }
}

fn tool(name: &str, title: &str, desc: &str, props: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": desc,
        "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": { "title": title, "readOnlyHint": read_only, "openWorldHint": false },
    })
}

fn tools_list() -> Value {
    let symbol = json!({"type": "string", "description": "Symbol name: `save`, `Store::save`, `Store.save`, `path/file.rs::Store::save`, or an ID"});
    json!([
        tool("search", "Search code",
            "Find functions, classes and other symbols by name or by describing what they do. Ranked by relevance and how widely the symbol is used.",
            json!({
                "query": {"type": "string", "description": "Names or a natural-language description"},
                "limit": {"type": "integer", "default": 15},
                "kind": {"type": "string", "description": "Only this kind: function, method, class, struct, interface, trait, enum, type, field, ..."},
                "path": {"type": "string", "description": "Only symbols under this path prefix"}
            }), &["query"], true),
        tool("context", "Code context for a task",
            "Given a task or question, return the most relevant code in one bundle: source of the best-matching symbols plus signatures of what they call and what calls them, within a token budget. Use this first when starting work on an unfamiliar area.",
            json!({
                "task": {"type": "string", "description": "What you are trying to do or understand, or a symbol name"},
                "budget_tokens": {"type": "integer", "default": 2000}
            }), &["task"], true),
        tool("text_search", "Search text",
            "Regex or literal search over every file in the repository (code, config, docs, any language), served from an index that is kept current. Each match is grouped under the function or class that contains it. Use it instead of grep for strings, error messages, comments, config keys and TODOs.",
            json!({
                "pattern": {"type": "string", "description": "Regex (Rust syntax) or, with fixed=true, a literal string"},
                "fixed": {"type": "boolean", "default": false, "description": "Treat the pattern as a literal string"},
                "ignore_case": {"type": "boolean", "default": false},
                "word": {"type": "boolean", "default": false, "description": "Match whole words only"},
                "path": {"type": "string", "description": "Only files under this path prefix, or matching this glob (e.g. *.toml)"},
                "limit": {"type": "integer", "default": 60, "description": "Most matching lines to show"}
            }), &["pattern"], true),
        tool("explain", "Explain a symbol",
            "Everything known about one symbol: signature, doc, location, members, callers, callees, supertypes, subtypes/implementations, overrides, and types it uses.",
            json!({"symbol": symbol}), &["symbol"], true),
        tool("source", "Read a symbol's source",
            "Source code of one symbol with line numbers (long bodies are trimmed in the middle). Cheaper than reading the whole file.",
            json!({"symbol": symbol, "max_lines": {"type": "integer", "default": 150}}), &["symbol"], true),
        tool("callers", "Who calls this",
            "Call sites of a function/method (file:line). Also lists possible callers: calls to the same name the resolver could not link to one target. With depth > 1, transitive callers grouped by distance.",
            json!({"symbol": symbol, "depth": {"type": "integer", "default": 1}}), &["symbol"], true),
        tool("callees", "What this calls",
            "Functions/methods called by a symbol, in order, with their locations.",
            json!({"symbol": symbol}), &["symbol"], true),
        tool("impact", "Change impact",
            "What may break if this symbol changes: transitive callers, users of a type, subclasses/implementations, and callers that reach it through an interface. Lists affected tests.",
            json!({"symbol": symbol, "depth": {"type": "integer", "default": 4}}), &["symbol"], true),
        tool("repo_map", "Repository map",
            "Ranked overview of the most important (most depended-on) symbols grouped by file, with signatures. Good first call in a new codebase.",
            json!({
                "budget_tokens": {"type": "integer", "default": 1500},
                "path": {"type": "string", "description": "Only files under this path prefix"}
            }), &[], true),
        tool("outline", "File outline",
            "All symbols in a file, nested, with kinds, signatures and line ranges.",
            json!({"file": {"type": "string", "description": "Workspace-relative path (a unique suffix is enough)"}}), &["file"], true),
        tool("call_path", "Dependency path",
            "Shortest chain of calls/references from one symbol to another.",
            json!({"from": symbol.clone(), "to": symbol.clone()}), &["from", "to"], true),
        tool("dead_code", "Unused code",
            "Functions and methods nothing in the project calls or references (candidates: entry points and callbacks can look unused).",
            json!({"path": {"type": "string"}, "limit": {"type": "integer", "default": 50}}), &[], true),
        tool("neighborhood", "Graph neighborhood",
            "Symbols and edges within N hops of a symbol.",
            json!({"symbol": symbol, "depth": {"type": "integer", "default": 1}, "cap": {"type": "integer", "default": 40}}), &["symbol"], true),
        tool("reindex", "Re-index",
            "Re-index changed files now (normally automatic), or rebuild everything with full=true.",
            json!({"full": {"type": "boolean", "default": false}}), &[], false),
        tool("index_status", "Index status",
            "What is indexed: files, symbols, edges, languages, resolution quality.",
            json!({}), &[], true),
    ])
}

fn s<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|v| !v.trim().is_empty())
}

fn n(args: &Value, key: &str, default: usize) -> usize {
    args.get(key).and_then(|v| v.as_u64()).map(|v| v as usize).unwrap_or(default)
}

fn need<'a>(args: &'a Value, keys: &[&str]) -> anyhow::Result<&'a str> {
    keys.iter()
        .find_map(|k| s(args, k))
        .ok_or_else(|| anyhow::anyhow!("missing argument `{}`", keys[0]))
}

fn call_tool(ws: &mut Workspace, name: &str, args: &Value) -> anyhow::Result<String> {
    if name == "reindex" || name == "index" || name == "incremental_index" {
        let full = args.get("full").and_then(|v| v.as_bool()).unwrap_or(name == "index");
        let note = ws.refresh(full)?.unwrap_or_else(|| "Index is up to date.".into());
        ws.refresh_text()?;
        return Ok(format!("{note}\n{}", ws.status_text()));
    }
    let note = ws.ensure_fresh()?;
    let sym_keys = ["symbol", "id", "name", "needle"];
    let out = match name {
        "search" | "semantic_search" | "find_symbol" => ws.search_text(
            need(args, &["query", "needle"])?,
            n(args, "limit", 15),
            s(args, "kind"),
            s(args, "path"),
        )?,
        "context" => ws.context_text(need(args, &["task", "query"])?, n(args, "budget_tokens", 2000))?,
        "text_search" | "grep" => {
            let b = |k: &str| args.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
            let q = adaptive_codegraph_core::text::TextQuery {
                pattern: need(args, &["pattern", "query"])?.to_string(),
                fixed: b("fixed"),
                ignore_case: b("ignore_case"),
                word: b("word"),
                path: s(args, "path").map(str::to_string),
            };
            let r = ws.grep(&q)?;
            adaptive_codegraph_core::service::format_grep(&r, n(args, "limit", 60))
        }
        "explain" | "get_symbol" => ws.explain_text(need(args, &sym_keys)?),
        "source" => ws.source_text(need(args, &sym_keys)?, n(args, "max_lines", 150)),
        "callers" | "find_callers" => ws.callers_text(need(args, &sym_keys)?, n(args, "depth", 1)),
        "callees" | "find_callees" => ws.callees_text(need(args, &sym_keys)?),
        "impact" => ws.impact_text(need(args, &sym_keys)?, n(args, "depth", 4)),
        "repo_map" => ws.repo_map_text(n(args, "budget_tokens", 1500), s(args, "path")),
        "outline" => ws.outline_text(need(args, &["file", "path"])?),
        "call_path" => ws.path_text(need(args, &["from"])?, need(args, &["to"])?),
        "dead_code" => ws.dead_code_text(s(args, "path"), n(args, "limit", 50)),
        "neighborhood" | "expand_neighborhood" => {
            ws.neighborhood_text(need(args, &sym_keys)?, n(args, "depth", 1), n(args, "cap", 40))
        }
        "index_status" => ws.status_text(),
        _ => anyhow::bail!("unknown tool {name}"),
    };
    Ok(match note {
        Some(n) => format!("{n}\n{out}"),
        None => out,
    })
}
