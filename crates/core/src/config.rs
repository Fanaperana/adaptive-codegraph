//! # Project Configuration
//!
//! Auto-detects project language, structure, and roots. Loads optional
//! `.adaptive-codegraph.toml` for overrides.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A language detected in the project.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetectedLanguage {
    pub id: String,
    pub extensions: Vec<String>,
    pub query_file: Option<PathBuf>,
}

/// Project configuration — either auto-detected or from config file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Base directory for the project (set at load time, not serialized).
    #[serde(skip)]
    pub base: Option<PathBuf>,
    /// Root directories to index (relative to base).
    pub roots: Vec<PathBuf>,
    /// Glob patterns to exclude.
    pub exclude: Vec<String>,
    /// Where to store the index.
    pub index_dir: PathBuf,
    /// Detected or configured languages.
    pub languages: Vec<DetectedLanguage>,
}

/// Well-known language definition (public for CLI listing).
pub struct BuiltinLangInfo {
    pub id: &'static str,
    pub extensions: &'static [&'static str],
}

/// List all built-in language IDs and their extensions.
pub fn list_builtin_languages() -> Vec<BuiltinLangInfo> {
    builtin_languages()
        .into_iter()
        .map(|b| BuiltinLangInfo {
            id: b.id,
            extensions: b.extensions,
        })
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base: None,
            roots: vec![PathBuf::from(".")],
            exclude: vec![
                "**/.git/**".into(),
                "**/node_modules/**".into(),
                "**/target/**".into(),
                "**/build/**".into(),
                "**/__pycache__/**".into(),
                "**/.venv/**".into(),
                "**/*.min.js".into(),
                "**/vendor/**".into(),
                "**/dist/**".into(),
            ],
            index_dir: PathBuf::from(".adaptive-codegraph"),
            languages: Vec::new(),
        }
    }
}

/// Well-known language definitions (private).
fn builtin_languages() -> Vec<BuiltinLang> {
    vec![
        BuiltinLang {
            id: "c",
            extensions: &["c", "h"],
            markers: &["Makefile", "CMakeLists.txt", "*.c"],
        },
        BuiltinLang {
            id: "rust",
            extensions: &["rs"],
            markers: &["Cargo.toml"],
        },
        BuiltinLang {
            id: "python",
            extensions: &["py"],
            markers: &["pyproject.toml", "setup.py", "requirements.txt"],
        },
        BuiltinLang {
            id: "javascript",
            extensions: &["js", "mjs", "cjs"],
            markers: &["package.json"],
        },
        BuiltinLang {
            id: "typescript",
            extensions: &["ts", "tsx"],
            markers: &["tsconfig.json"],
        },
        BuiltinLang {
            id: "go",
            extensions: &["go"],
            markers: &["go.mod"],
        },
        BuiltinLang {
            id: "java",
            extensions: &["java"],
            markers: &["pom.xml", "build.gradle", "build.gradle.kts"],
        },
        BuiltinLang {
            id: "ruby",
            extensions: &["rb"],
            markers: &["Gemfile"],
        },
        BuiltinLang {
            id: "csharp",
            extensions: &["cs"],
            markers: &["*.csproj", "*.sln"],
        },
        BuiltinLang {
            id: "cpp",
            extensions: &["cpp", "cc", "cxx", "hpp", "hxx"],
            markers: &["CMakeLists.txt"],
        },
        BuiltinLang {
            id: "sql",
            extensions: &["sql"],
            markers: &[],
        },
        BuiltinLang {
            id: "html",
            extensions: &["html", "htm"],
            markers: &[],
        },
    ]
}

struct BuiltinLang {
    id: &'static str,
    extensions: &'static [&'static str],
    markers: &'static [&'static str],
}

impl Config {
    /// Load config from file, or auto-detect from the project root.
    pub fn load(base: &Path) -> anyhow::Result<Self> {
        let config_path = base.join(".adaptive-codegraph.toml");
        if config_path.exists() {
            let text = std::fs::read_to_string(&config_path)?;
            let mut cfg: Config = toml::from_str(&text)?;
            cfg.base = Some(base.to_path_buf());
            if cfg.languages.is_empty() {
                cfg.languages = detect_languages(base);
            }
            return Ok(cfg);
        }

        // Auto-detect everything
        let mut cfg = Config::default();
        cfg.base = Some(base.to_path_buf());
        cfg.languages = detect_languages(base);

        // Auto-detect roots: look for src/, lib/, app/ directories
        let candidate_dirs = ["src", "lib", "app", "pkg", "cmd", "internal"];
        let mut roots = Vec::new();
        for dir in &candidate_dirs {
            if base.join(dir).is_dir() {
                roots.push(PathBuf::from(*dir));
            }
        }
        if roots.is_empty() {
            roots.push(PathBuf::from("."));
        }
        cfg.roots = roots;

        Ok(cfg)
    }

    /// Get all file extensions this config cares about.
    pub fn extensions(&self) -> Vec<String> {
        self.languages
            .iter()
            .flat_map(|l| l.extensions.iter().cloned())
            .collect()
    }
}

/// Scan the project root for marker files to detect which languages are present.
fn detect_languages(base: &Path) -> Vec<DetectedLanguage> {
    let mut detected = Vec::new();

    for lang in builtin_languages() {
        let has_marker = lang.markers.iter().any(|marker| {
            if marker.contains('*') {
                // Glob pattern — check if any matching file exists in root
                std::fs::read_dir(base)
                    .ok()
                    .map(|entries| {
                        entries.filter_map(|e| e.ok()).any(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            let pattern = marker.replace('*', "");
                            name.ends_with(&pattern)
                        })
                    })
                    .unwrap_or(false)
            } else {
                base.join(marker).exists()
            }
        });

        let has_files = lang.extensions.iter().any(|ext| {
            has_files_with_extension(base, ext)
        });

        if has_marker || has_files {
            detected.push(DetectedLanguage {
                id: lang.id.to_string(),
                extensions: lang.extensions.iter().map(|s| s.to_string()).collect(),
                query_file: None, // Will be resolved by the language registry
            });
        }
    }

    detected
}

/// Quick check: does the directory tree contain files with this extension?
/// Only scans 2 levels deep to keep it fast.
fn has_files_with_extension(base: &Path, ext: &str) -> bool {
    let check = |dir: &Path| -> bool {
        std::fs::read_dir(dir)
            .ok()
            .map(|entries| {
                entries.filter_map(|e| e.ok()).any(|e| {
                    e.path()
                        .extension()
                        .map(|x| x == ext)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };

    if check(base) {
        return true;
    }

    // Check one level of subdirectories
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') && name != "node_modules" && name != "target" {
                    if check(&entry.path()) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_standard_excludes() {
        let cfg = Config::default();
        assert!(cfg.exclude.iter().any(|e| e.contains("node_modules")));
        assert!(cfg.exclude.iter().any(|e| e.contains("target")));
    }
}
