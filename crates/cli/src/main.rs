use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Instant;

use adaptive_codegraph_core::{lang, service::Workspace};

#[derive(Parser)]
#[command(
    name = "adaptive-codegraph",
    version,
    about = "Language-agnostic code graph: index, search, and explain a codebase"
)]
struct Cli {
    /// Path to workspace root (default: current directory)
    #[arg(long, default_value = ".", global = true)]
    base: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize adaptive-codegraph in the current project (creates .adaptive-codegraph/)
    Init,
    /// Build or update the index
    Index {
        /// Only re-index files changed since the last index
        #[arg(long)]
        incremental: bool,
        /// Print nothing on success
        #[arg(long, short)]
        quiet: bool,
    },
    /// Search symbols by name or description
    Search {
        query: String,
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Only this symbol kind (function, method, class, ...)
        #[arg(long)]
        kind: Option<String>,
        /// Only symbols under this path prefix
        #[arg(long)]
        path: Option<String>,
    },
    /// Alias of `search` (kept for compatibility)
    #[command(hide = true)]
    SemanticSearch {
        query: String,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Alias of `search` (kept for compatibility)
    #[command(hide = true)]
    Find {
        name: String,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Everything known about a symbol
    Explain { symbol: String },
    /// Print a symbol's source with line numbers
    Source {
        symbol: String,
        #[arg(long, default_value = "150")]
        max_lines: usize,
    },
    /// Who calls a symbol (transitively with --depth > 1)
    Callers {
        symbol: String,
        #[arg(long, default_value = "1")]
        depth: usize,
    },
    /// What a symbol calls
    Callees { symbol: String },
    /// What may break if a symbol changes
    Impact {
        symbol: String,
        #[arg(long, default_value = "4")]
        depth: usize,
    },
    /// Most relevant code for a task, within a token budget
    Context {
        task: String,
        #[arg(long, default_value = "2000")]
        budget: usize,
    },
    /// Ranked overview of the most important symbols
    Map {
        #[arg(long, default_value = "1500")]
        budget: usize,
        #[arg(long)]
        path: Option<String>,
    },
    /// Symbols in a file
    Outline { file: String },
    /// Shortest dependency path between two symbols
    Path { from: String, to: String },
    /// Functions/methods nothing uses
    DeadCode {
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Symbols and edges around a symbol
    Neighborhood {
        symbol: String,
        #[arg(long, default_value = "2")]
        depth: usize,
        #[arg(long, default_value = "50")]
        cap: usize,
    },
    /// Show index status
    Status,
    /// Update the index and print a short orientation (for session-start hooks)
    SessionStart {
        #[arg(long, default_value = "800")]
        budget: usize,
    },
    /// List detected languages
    Languages,
    /// Add a custom language definition
    AddLanguage {
        /// Path to a .toml language definition file
        toml_file: PathBuf,
        /// Path to the symbol query .scm file
        #[arg(long)]
        symbol_query: Option<PathBuf>,
        /// Path to the edge query .scm file
        #[arg(long)]
        edge_query: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let base = PathBuf::from(&cli.base).canonicalize()?;

    match cli.command {
        Commands::Init => return cmd_init(&base),
        Commands::Languages => return cmd_languages(&base),
        Commands::AddLanguage {
            toml_file,
            symbol_query,
            edge_query,
        } => return cmd_add_language(&base, &toml_file, symbol_query.as_deref(), edge_query.as_deref()),
        _ => {}
    }

    let mut ws = Workspace::open(&base)?;
    let out = match cli.command {
        Commands::Index { incremental, quiet } => {
            let t0 = Instant::now();
            let note = ws.refresh(!incremental)?;
            if quiet {
                return Ok(());
            }
            let s = ws.store();
            eprintln!(
                "{}Indexed {} files, {} symbols, {} edges in {:.2}s",
                note.map(|n| format!("{n}\n")).unwrap_or_default(),
                s.file_count(),
                s.symbol_count(),
                s.edge_count(),
                t0.elapsed().as_secs_f64()
            );
            return Ok(());
        }
        Commands::Status => {
            // Report only: never build an index just to show its status.
            if !ws.load_existing() {
                println!("No index yet for {}. Any query (or `adaptive-codegraph index`) builds it.", base.display().to_string().trim_start_matches(r"\\?\"));
                return Ok(());
            }
            ws.status_text()
        }
        Commands::SessionStart { budget } => {
            // Stay silent outside projects (e.g. a session in the home folder)
            // and when disabled.
            if std::env::var_os("ACG_DISABLE").is_some() || !ws.is_project() {
                return Ok(());
            }
            // Never fail a session start: print what we can.
            match ws.ensure_fresh() {
                Ok(_) => {
                    let s = ws.store();
                    format!(
                        "Code graph ready ({} files, {} symbols). Use the adaptive-codegraph tools (search, context, explain, impact, callers, source, repo_map) instead of grep for code structure.\nMost depended-on code:\n{}",
                        s.file_count(),
                        s.symbol_count(),
                        ws.repo_map_text(budget, None)
                    )
                }
                Err(e) => format!("adaptive-codegraph: index unavailable ({e})"),
            }
        }
        cmd => {
            if let Some(note) = ws.ensure_fresh()? {
                eprintln!("{note}");
            }
            match cmd {
                Commands::Search { query, limit, kind, path } => {
                    ws.search_text(&query, limit, kind.as_deref(), path.as_deref())?
                }
                Commands::SemanticSearch { query, limit } | Commands::Find { name: query, limit } => {
                    ws.search_text(&query, limit, None, None)?
                }
                Commands::Explain { symbol } => ws.explain_text(&symbol),
                Commands::Source { symbol, max_lines } => ws.source_text(&symbol, max_lines),
                Commands::Callers { symbol, depth } => ws.callers_text(&symbol, depth),
                Commands::Callees { symbol } => ws.callees_text(&symbol),
                Commands::Impact { symbol, depth } => ws.impact_text(&symbol, depth),
                Commands::Context { task, budget } => ws.context_text(&task, budget)?,
                Commands::Map { budget, path } => ws.repo_map_text(budget, path.as_deref()),
                Commands::Outline { file } => ws.outline_text(&file),
                Commands::Path { from, to } => ws.path_text(&from, &to),
                Commands::DeadCode { path, limit } => ws.dead_code_text(path.as_deref(), limit),
                Commands::Neighborhood { symbol, depth, cap } => ws.neighborhood_text(&symbol, depth, cap),
                Commands::Status => ws.status_text(),
                _ => unreachable!(),
            }
        }
    };
    print!("{out}");
    Ok(())
}

fn cmd_languages(base: &Path) -> Result<()> {
    let registry = lang::build_registry(base)?;
    let langs = registry.languages();
    if langs.is_empty() {
        println!("No languages loaded. Run `adaptive-codegraph init` first.");
        let builtins = adaptive_codegraph_core::config::list_builtin_languages();
        println!("\nBuilt-in languages available:");
        for b in &builtins {
            println!("  {} - extensions: {}", b.id, b.extensions.join(", "));
        }
    } else {
        println!("Loaded languages:");
        for lang in &langs {
            println!("  {}", lang);
        }
    }
    Ok(())
}

fn cmd_init(base: &Path) -> Result<()> {
    let acg_dir = base.join(".adaptive-codegraph");

    if acg_dir.join("languages").exists() {
        eprintln!("Already initialized at {}", acg_dir.display());
        eprintln!("To re-initialize, delete .adaptive-codegraph/ and run init again.");
        return Ok(());
    }

    // Create .adaptive-codegraph/ and write embedded language files
    std::fs::create_dir_all(&acg_dir)?;
    lang::embedded::write_to(&acg_dir)?;

    // Add .vscode/mcp.json if .vscode/ exists or create it
    let vscode_dir = base.join(".vscode");
    let mcp_json = vscode_dir.join("mcp.json");
    if !mcp_json.exists() {
        std::fs::create_dir_all(&vscode_dir)?;
        std::fs::write(
            &mcp_json,
            r#"{
  "servers": {
    "adaptive-codegraph": {
      "type": "stdio",
      "command": "adaptive-codegraph-mcp",
      "args": ["--base", "${workspaceFolder}"]
    }
  }
}
"#,
        )?;
        eprintln!("Created {}", mcp_json.display());
    }

    // Add .adaptive-codegraph/ to .gitignore if git repo
    let gitignore = base.join(".gitignore");
    if base.join(".git").exists() {
        let content = std::fs::read_to_string(&gitignore).unwrap_or_default();
        if !content.contains(".adaptive-codegraph") {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&gitignore)?;
            use std::io::Write;
            writeln!(file, "\n# Adaptive Codegraph index")?;
            writeln!(file, ".adaptive-codegraph/")?;
            eprintln!("Added .adaptive-codegraph/ to .gitignore");
        }
    }

    let lang_count = lang::embedded::language_ids().len();
    eprintln!("Initialized .adaptive-codegraph/ with {} languages", lang_count);
    eprintln!("Languages: {}", lang::embedded::language_ids().join(", "));
    eprintln!("\nNext: run `adaptive-codegraph index` to build the index.");
    Ok(())
}

fn cmd_add_language(
    base: &Path,
    toml_file: &Path,
    symbol_query: Option<&Path>,
    edge_query: Option<&Path>,
) -> Result<()> {
    let acg_dir = base.join(".adaptive-codegraph");
    let lang_dir = acg_dir.join("languages");
    let queries_dir = lang_dir.join("queries");

    if !lang_dir.exists() {
        anyhow::bail!("Not initialized. Run `adaptive-codegraph init` first.");
    }

    let toml_content = std::fs::read_to_string(toml_file)?;
    let def: adaptive_codegraph_core::lang::LanguageDef = toml::from_str(&toml_content)?;

    let dest_toml = lang_dir.join(format!("{}.toml", def.id));
    std::fs::write(&dest_toml, &toml_content)?;
    eprintln!("Installed {}.toml", def.id);

    std::fs::create_dir_all(&queries_dir)?;
    if let Some(sq) = symbol_query {
        let dest = queries_dir.join(format!("{}.scm", def.id));
        std::fs::copy(sq, &dest)?;
        eprintln!("Installed queries/{}.scm", def.id);
    }
    if let Some(eq) = edge_query {
        let dest = queries_dir.join(format!("{}_edges.scm", def.id));
        std::fs::copy(eq, &dest)?;
        eprintln!("Installed queries/{}_edges.scm", def.id);
    }

    eprintln!(
        "\nLanguage '{}' added. Run `adaptive-codegraph index` to re-index.",
        def.id
    );
    Ok(())
}
