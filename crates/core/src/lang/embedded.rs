//! Embedded language definitions — compiled into the binary so no external
//! `languages/` directory is needed for the default set.

/// A single embedded language file (toml config or scm query).
pub struct EmbeddedFile {
    pub name: &'static str,
    pub content: &'static str,
}

/// All embedded language .toml files.
pub fn toml_files() -> &'static [EmbeddedFile] {
    &[
        EmbeddedFile {
            name: "c.toml",
            content: include_str!("../../../../languages/c.toml"),
        },
        EmbeddedFile {
            name: "go.toml",
            content: include_str!("../../../../languages/go.toml"),
        },
        EmbeddedFile {
            name: "javascript.toml",
            content: include_str!("../../../../languages/javascript.toml"),
        },
        EmbeddedFile {
            name: "python.toml",
            content: include_str!("../../../../languages/python.toml"),
        },
        EmbeddedFile {
            name: "rust.toml",
            content: include_str!("../../../../languages/rust.toml"),
        },
        EmbeddedFile {
            name: "typescript.toml",
            content: include_str!("../../../../languages/typescript.toml"),
        },
    ]
}

/// All embedded query .scm files.
pub fn query_files() -> &'static [EmbeddedFile] {
    &[
        EmbeddedFile {
            name: "c.scm",
            content: include_str!("../../../../languages/queries/c.scm"),
        },
        EmbeddedFile {
            name: "c_edges.scm",
            content: include_str!("../../../../languages/queries/c_edges.scm"),
        },
        EmbeddedFile {
            name: "go.scm",
            content: include_str!("../../../../languages/queries/go.scm"),
        },
        EmbeddedFile {
            name: "go_edges.scm",
            content: include_str!("../../../../languages/queries/go_edges.scm"),
        },
        EmbeddedFile {
            name: "javascript.scm",
            content: include_str!("../../../../languages/queries/javascript.scm"),
        },
        EmbeddedFile {
            name: "javascript_edges.scm",
            content: include_str!("../../../../languages/queries/javascript_edges.scm"),
        },
        EmbeddedFile {
            name: "python.scm",
            content: include_str!("../../../../languages/queries/python.scm"),
        },
        EmbeddedFile {
            name: "python_edges.scm",
            content: include_str!("../../../../languages/queries/python_edges.scm"),
        },
        EmbeddedFile {
            name: "rust.scm",
            content: include_str!("../../../../languages/queries/rust.scm"),
        },
        EmbeddedFile {
            name: "rust_edges.scm",
            content: include_str!("../../../../languages/queries/rust_edges.scm"),
        },
        EmbeddedFile {
            name: "typescript.scm",
            content: include_str!("../../../../languages/queries/typescript.scm"),
        },
        EmbeddedFile {
            name: "typescript_edges.scm",
            content: include_str!("../../../../languages/queries/typescript_edges.scm"),
        },
    ]
}

/// List the IDs of all embedded languages.
pub fn language_ids() -> Vec<&'static str> {
    toml_files()
        .iter()
        .map(|f| f.name.trim_end_matches(".toml"))
        .collect()
}

/// Write all embedded language files to the given directory.
///
/// Creates `<dir>/languages/*.toml` and `<dir>/languages/queries/*.scm`.
pub fn write_to(dir: &std::path::Path) -> std::io::Result<()> {
    let lang_dir = dir.join("languages");
    let queries_dir = lang_dir.join("queries");
    std::fs::create_dir_all(&queries_dir)?;

    for f in toml_files() {
        std::fs::write(lang_dir.join(f.name), f.content)?;
    }
    for f in query_files() {
        std::fs::write(queries_dir.join(f.name), f.content)?;
    }

    Ok(())
}
