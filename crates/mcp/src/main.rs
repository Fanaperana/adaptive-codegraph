//! # MCP Server for adaptive-codegraph
//!
//! JSON-RPC 2.0 over stdio with MCP tool surface.
//! Mirrors mie-codegraph's hand-rolled approach for compatibility.

use adaptive_codegraph_core::{
    config::Config,
    embed,
    extract::plugin::PluginRegistry,
    index::{self, IndexState},
    lang,
    query,
    search::SearchIndex,
    store::Store,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Req {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Resp<'a> {
    jsonrpc: &'a str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcErr>,
}

#[derive(Serialize)]
struct RpcErr {
    code: i64,
    message: String,
}

struct Ctx {
    base: PathBuf,
    config: Config,
    store: RefCell<Store>,
    index_dir: PathBuf,
    bm25: RefCell<Option<SearchIndex>>,
}

impl Ctx {
    fn bm25(&self) -> Result<std::cell::Ref<'_, SearchIndex>, String> {
        if self.bm25.borrow().is_none() {
            let idx = SearchIndex::open(&self.index_dir)
                .map_err(|e| format!("open bm25: {e}"))?;
            *self.bm25.borrow_mut() = Some(idx);
        }
        Ok(std::cell::Ref::map(self.bm25.borrow(), |o| {
            o.as_ref().unwrap()
        }))
    }

    fn reload_after_reindex(&self) -> Result<(), String> {
        match Store::load(&self.index_dir.join("graph.bin")) {
            Ok(s) => *self.store.borrow_mut() = s,
            Err(e) => return Err(format!("reload store: {e}")),
        }
        *self.bm25.borrow_mut() = None;
        Ok(())
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .with_target(false)
        .init();

    let base = std::env::current_dir()?;
    let config = Config::load(&base)?;
    let index_dir = base.join(&config.index_dir);

    let store = Store::load(&index_dir.join("graph.bin")).unwrap_or_else(|e| {
        eprintln!("warn: load store failed: {e}; serving empty store");
        Store::new()
    });

    let ctx = Ctx {
        base,
        config,
        store: RefCell::new(store),
        index_dir,
        bm25: RefCell::new(None),
    };

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Req = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_err(&mut stdout, Value::Null, -32700, &format!("parse: {e}"))?;
                continue;
            }
        };
        let id = req.id.clone().unwrap_or(Value::Null);
        match dispatch(&ctx, &req) {
            Ok(v) => write_ok(&mut stdout, id, v)?,
            Err((code, msg)) => write_err(&mut stdout, id, code, &msg)?,
        }
    }
    Ok(())
}

fn dispatch(ctx: &Ctx, req: &Req) -> Result<Value, (i64, String)> {
    if req.jsonrpc != "2.0" {
        return Err((-32600, "expected jsonrpc 2.0".into()));
    }
    match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "adaptive-codegraph-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "tools/list" => Ok(json!({ "tools": tools_list() })),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or((-32602, "missing name".into()))?;
            let args = req.params.get("arguments").cloned().unwrap_or(Value::Null);
            let out = call_tool(ctx, name, &args)?;
            Ok(json!({ "content": [{ "type": "text", "text": out.to_string() }] }))
        }
        _ => Err((-32601, format!("unknown method {}", req.method))),
    }
}

fn tools_list() -> Value {
    json!([
        tool("search",
            "BM25 full-text search over symbol names, fqnames, signatures, and file paths.",
            json!({"type":"object","properties":{
                "query":{"type":"string"},
                "limit":{"type":"integer","default":20}
            },"required":["query"]})),
        tool("find_symbol",
            "Substring search for a symbol by name. Returns matches with file/line.",
            json!({"type":"object","properties":{
                "needle":{"type":"string"},
                "kind":{"type":"string"},
                "lang":{"type":"string"},
                "limit":{"type":"integer","default":20}
            },"required":["needle"]})),
        tool("get_symbol",
            "Get a symbol by id (32-hex).",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("find_callers",
            "Functions that call this symbol.",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("find_callees",
            "Functions called by this symbol.",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]})),
        tool("expand_neighborhood",
            "BFS subgraph around a symbol (default depth=2, cap=50).",
            json!({"type":"object","properties":{
                "id":{"type":"string"},
                "depth":{"type":"integer","default":2},
                "cap":{"type":"integer","default":50}
            },"required":["id"]})),
        tool("index",
            "Full rebuild of the codebase index.",
            json!({"type":"object","properties":{}})),
        tool("index_status",
            "Report what's indexed: git HEAD, file count, symbol count.",
            json!({"type":"object","properties":{}})),
    ])
}

fn tool(name: &str, desc: &str, schema: Value) -> Value {
    json!({ "name": name, "description": desc, "inputSchema": schema })
}

fn call_tool(ctx: &Ctx, name: &str, args: &Value) -> Result<Value, (i64, String)> {
    match name {
        "index" => {
            let registry = lang::build_registry(&ctx.base)
                .map_err(|e| (-32603, format!("build_registry: {e}")))?;
            let plugins = PluginRegistry::new();
            let (store, _search, _vectors) =
                index::full_index(&ctx.config, &registry, &plugins)
                    .map_err(|e| (-32603, format!("full_index: {e}")))?;
            let syms = store.symbol_count();
            let edges = store.edge_count();
            ctx.reload_after_reindex().map_err(|e| (-32603, e))?;
            return Ok(json!({
                "symbols": syms,
                "edges": edges,
                "status": "ok"
            }));
        }
        "index_status" => {
            let state_path = ctx.index_dir.join("state.json");
            if !state_path.exists() {
                return Ok(json!({"status": "no_index"}));
            }
            let state = IndexState::load(&state_path)
                .map_err(|e| (-32603, format!("load state: {e}")))?;
            let store = ctx.store.borrow();
            return Ok(json!({
                "git_head": state.git_head,
                "indexed_at": state.indexed_at,
                "file_count": state.file_count,
                "symbols": store.symbol_count(),
                "edges": store.edge_count(),
            }));
        }
        _ => {}
    }

    let store = ctx.store.borrow();
    match name {
        "find_symbol" => {
            let needle = args.get("needle").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let kind = args.get("kind").and_then(|v| v.as_str());
            let lang = args.get("lang").and_then(|v| v.as_str());
            let results = store.find_filtered(needle, kind, lang);
            let hits: Vec<Value> = results
                .iter()
                .take(limit)
                .map(|s| {
                    json!({
                        "id": s.id.to_hex(),
                        "name": s.name,
                        "kind": s.kind,
                        "lang": s.lang,
                        "file": s.file,
                        "start_line": s.span.start_line,
                        "end_line": s.span.end_line,
                        "signature": s.signature,
                    })
                })
                .collect();
            Ok(json!(hits))
        }
        "get_symbol" => {
            let id_hex = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let id = crate::parse_symbol_id(id_hex)?;
            match store.get(&id) {
                Some(s) => Ok(json!({
                    "id": s.id.to_hex(),
                    "name": s.name,
                    "kind": s.kind,
                    "lang": s.lang,
                    "file": s.file,
                    "fqname": s.fqname,
                    "start_line": s.span.start_line,
                    "end_line": s.span.end_line,
                    "signature": s.signature,
                    "doc": s.doc,
                })),
                None => Err((-32602, format!("symbol {id_hex} not found"))),
            }
        }
        "find_callers" => {
            let id = parse_id_arg(args)?;
            let callers = store.callers(&id);
            let hits: Vec<Value> = callers
                .iter()
                .map(|(s, kind)| {
                    json!({
                        "id": s.id.to_hex(),
                        "name": s.name,
                        "kind": s.kind,
                        "file": s.file,
                        "edge_kind": kind,
                    })
                })
                .collect();
            Ok(json!(hits))
        }
        "find_callees" => {
            let id = parse_id_arg(args)?;
            let callees = store.callees(&id);
            let hits: Vec<Value> = callees
                .iter()
                .map(|(s, kind)| {
                    json!({
                        "id": s.id.to_hex(),
                        "name": s.name,
                        "kind": s.kind,
                        "file": s.file,
                        "edge_kind": kind,
                    })
                })
                .collect();
            Ok(json!(hits))
        }
        "expand_neighborhood" => {
            let id = parse_id_arg(args)?;
            let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
            let cap = args.get("cap").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
            let n = query::expand_neighborhood(&store, id, depth, cap);
            Ok(json!({
                "text": query::format_neighborhood(&store, &n),
                "node_count": n.nodes.len(),
                "edge_count": n.edges.len(),
            }))
        }
        "search" => {
            let q = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let bm25 = ctx.bm25().map_err(|e| (-32603, e))?;
            let hits = bm25.search(q, limit).map_err(|e| (-32603, e.to_string()))?;
            let results: Vec<Value> = hits
                .iter()
                .map(|h| {
                    json!({
                        "id": h.id.to_hex(),
                        "name": h.name,
                        "kind": h.kind,
                        "lang": h.lang,
                        "file": h.file,
                        "score": h.score,
                    })
                })
                .collect();
            Ok(json!(results))
        }
        _ => Err((-32601, format!("unknown tool {name}"))),
    }
}

fn parse_id_arg(args: &Value) -> Result<adaptive_codegraph_core::model::SymbolId, (i64, String)> {
    let id_hex = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
    parse_symbol_id(id_hex)
}

fn parse_symbol_id(
    hex: &str,
) -> Result<adaptive_codegraph_core::model::SymbolId, (i64, String)> {
    adaptive_codegraph_core::model::SymbolId::from_hex(hex)
        .map_err(|e| (-32602, format!("invalid id '{hex}': {e}")))
}

fn write_ok<W: Write>(w: &mut W, id: Value, result: Value) -> Result<()> {
    let r = Resp {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    };
    let s = serde_json::to_string(&r)?;
    writeln!(w, "{s}")?;
    w.flush()?;
    Ok(())
}

fn write_err<W: Write>(w: &mut W, id: Value, code: i64, message: &str) -> Result<()> {
    let r = Resp {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcErr {
            code,
            message: message.to_string(),
        }),
    };
    let s = serde_json::to_string(&r)?;
    writeln!(w, "{s}")?;
    w.flush()?;
    Ok(())
}
