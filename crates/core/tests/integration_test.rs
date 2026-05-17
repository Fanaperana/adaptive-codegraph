//! Integration tests: extract symbols from fixture files and verify counts/kinds.
//!
//! These tests exercise the full extraction pipeline against the edge case
//! fixture files in `tests/fixtures/`.

use adaptive_codegraph_core::extract::treesitter::{TreeSitterConfig, TreeSitterExtractor};
use adaptive_codegraph_core::extract::Extractor;
use adaptive_codegraph_core::model::ExtractionResult;
use std::path::Path;

/// Helper: build a TreeSitterExtractor for a given language.
fn make_extractor(lang: &str) -> TreeSitterExtractor {
    // Read query files from the workspace languages/ directory
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let languages_dir = Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("languages");

    let symbol_query =
        std::fs::read_to_string(languages_dir.join(format!("queries/{lang}.scm"))).unwrap();
    let edge_query =
        std::fs::read_to_string(languages_dir.join(format!("queries/{lang}_edges.scm"))).unwrap();

    let ts_language = match lang {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        _ => panic!("unknown language: {lang}"),
    };

    let config = TreeSitterConfig {
        lang_id: lang.to_string(),
        extensions: vec![],
        ts_language,
        symbol_query,
        edge_query,
    };

    TreeSitterExtractor::new(config).expect("failed to create extractor")
}

/// Helper: make a TSX extractor (uses typescript grammar in TSX mode).
fn make_tsx_extractor() -> TreeSitterExtractor {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let languages_dir = Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("languages");

    let symbol_query =
        std::fs::read_to_string(languages_dir.join("queries/typescript.scm")).unwrap();
    let edge_query =
        std::fs::read_to_string(languages_dir.join("queries/typescript_edges.scm")).unwrap();

    let config = TreeSitterConfig {
        lang_id: "typescript".to_string(),
        extensions: vec![],
        ts_language: tree_sitter_typescript::LANGUAGE_TSX.into(),
        symbol_query,
        edge_query,
    };

    TreeSitterExtractor::new(config).expect("failed to create tsx extractor")
}

/// Helper: extract from a fixture file.
fn extract_fixture(lang: &str, filename: &str) -> ExtractionResult {
    let extractor = make_extractor(lang);
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures");

    let file_path = fixtures_dir.join(filename);
    let content = std::fs::read(&file_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", file_path.display()));
    extractor
        .extract(&file_path, &content)
        .unwrap_or_else(|e| panic!("Extraction failed for {}: {e}", file_path.display()))
}

/// Helper: extract TSX fixture.
fn extract_tsx_fixture(filename: &str) -> ExtractionResult {
    let extractor = make_tsx_extractor();
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let file_path = Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures")
        .join(filename);
    let content = std::fs::read(&file_path).unwrap();
    extractor.extract(&file_path, &content).unwrap()
}

/// Count symbols of a specific kind.
fn count_kind(result: &ExtractionResult, kind: &str) -> usize {
    result.symbols.iter().filter(|s| s.kind == kind).count()
}

/// Check that a symbol with the given name exists.
fn has_symbol(result: &ExtractionResult, name: &str) -> bool {
    result.symbols.iter().any(|s| s.name == name)
}

/// Check that a symbol with name and kind exists.
fn has_symbol_of_kind(result: &ExtractionResult, name: &str, kind: &str) -> bool {
    result
        .symbols
        .iter()
        .any(|s| s.name == name && s.kind == kind)
}

// ─── Rust ───────────────────────────────────────────────────────────────

#[test]
fn rust_extracts_symbols() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(
        r.symbols.len() >= 20,
        "Expected ≥20 Rust symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn rust_functions() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(has_symbol_of_kind(&r, "public_function", "function"));
    assert!(has_symbol_of_kind(&r, "private_function", "function"));
}

#[test]
fn rust_structs_and_enums() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(count_kind(&r, "struct") >= 1, "Should have structs");
    assert!(count_kind(&r, "enum") >= 1, "Should have enums");
}

#[test]
fn rust_traits_and_impls() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(count_kind(&r, "trait") >= 1, "Should have traits");
    assert!(count_kind(&r, "impl") >= 1, "Should have impls");
}

#[test]
fn rust_macros() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(count_kind(&r, "macro") >= 1, "Should have macros");
}

#[test]
fn rust_fields_and_variants() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(count_kind(&r, "field") >= 1, "Should have fields");
    assert!(
        count_kind(&r, "enum_variant") >= 1,
        "Should have enum variants"
    );
}

#[test]
fn rust_has_edges() {
    let r = extract_fixture("rust", "rust/edge_cases.rs");
    assert!(
        !r.unresolved_edges.is_empty(),
        "Should have unresolved call edges"
    );
}

// ─── Python ─────────────────────────────────────────────────────────────

#[test]
fn python_extracts_symbols() {
    let r = extract_fixture("python", "python/edge_cases.py");
    assert!(
        r.symbols.len() >= 30,
        "Expected ≥30 Python symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn python_classes() {
    let r = extract_fixture("python", "python/edge_cases.py");
    assert!(has_symbol_of_kind(&r, "Color", "class"));
    assert!(has_symbol_of_kind(&r, "Registry", "class"));
    assert!(has_symbol_of_kind(&r, "SingletonMeta", "class"));
    assert!(has_symbol_of_kind(&r, "Database", "class"));
    assert!(has_symbol_of_kind(&r, "Temperature", "class"));
    assert!(has_symbol_of_kind(&r, "Outer", "class"));
}

#[test]
fn python_nested_classes() {
    let r = extract_fixture("python", "python/edge_cases.py");
    assert!(has_symbol_of_kind(&r, "Inner", "class"));
    assert!(has_symbol_of_kind(&r, "DeepInner", "class"));
}

#[test]
fn python_functions() {
    let r = extract_fixture("python", "python/edge_cases.py");
    assert!(has_symbol(&r, "__init__"));
    assert!(has_symbol(&r, "accumulator"));
    assert!(has_symbol(&r, "gather_with_limit"));
}

#[test]
fn python_decorated_functions() {
    let r = extract_fixture("python", "python/edge_cases.py");
    // Decorated definitions (like @property, @classmethod)
    assert!(has_symbol(&r, "celsius"));
    assert!(has_symbol(&r, "fahrenheit"));
}

// ─── TypeScript ─────────────────────────────────────────────────────────

#[test]
fn typescript_extracts_symbols() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(
        r.symbols.len() >= 25,
        "Expected ≥25 TypeScript symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn typescript_enums() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(has_symbol_of_kind(&r, "BitFlags", "enum"));
    assert!(has_symbol_of_kind(&r, "Direction", "enum"));
}

#[test]
fn typescript_classes() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(has_symbol_of_kind(&r, "StateManager", "class"));
}

#[test]
fn typescript_type_aliases() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(has_symbol_of_kind(&r, "DeepPartial", "type_alias"));
}

#[test]
fn typescript_interfaces() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(has_symbol_of_kind(&r, "Entity", "interface"));
    assert!(has_symbol_of_kind(&r, "StringMap", "interface"));
}

#[test]
fn typescript_functions() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(has_symbol(&r, "parse"));
    assert!(has_symbol(&r, "merge"));
    assert!(has_symbol(&r, "area"));
}

#[test]
fn typescript_namespace_members() {
    let r = extract_fixture("typescript", "typescript/edge_cases.ts");
    assert!(has_symbol(&r, "isEmail"));
    assert!(has_symbol(&r, "isUUID"));
}

// ─── TSX ────────────────────────────────────────────────────────────────

#[test]
fn tsx_extracts_symbols() {
    let r = extract_tsx_fixture("typescript/edge_cases.tsx");
    assert!(
        r.symbols.len() >= 10,
        "Expected ≥10 TSX symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn tsx_components() {
    let r = extract_tsx_fixture("typescript/edge_cases.tsx");
    assert!(has_symbol(&r, "GenericTable"));
    assert!(has_symbol(&r, "StatusWrapper"));
}

#[test]
fn tsx_hooks() {
    let r = extract_tsx_fixture("typescript/edge_cases.tsx");
    assert!(has_symbol(&r, "useToggle"));
    assert!(has_symbol(&r, "useInterval"));
    assert!(has_symbol(&r, "useIntersectionObserver"));
}

// ─── JavaScript ─────────────────────────────────────────────────────────

#[test]
fn javascript_extracts_symbols() {
    let r = extract_fixture("javascript", "javascript/edge_cases.js");
    assert!(
        r.symbols.len() >= 15,
        "Expected ≥15 JavaScript symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn javascript_classes() {
    let r = extract_fixture("javascript", "javascript/edge_cases.js");
    assert!(has_symbol_of_kind(&r, "CustomMap", "class"));
    assert!(has_symbol_of_kind(&r, "Cache", "class"));
}

#[test]
fn javascript_functions() {
    let r = extract_fixture("javascript", "javascript/edge_cases.js");
    assert!(has_symbol(&r, "fibonacci"));
    assert!(has_symbol(&r, "reactive"));
    assert!(has_symbol(&r, "resilientFetch"));
}

#[test]
fn javascript_prototype_functions() {
    let r = extract_fixture("javascript", "javascript/edge_cases.js");
    assert!(has_symbol(&r, "Animal"));
    assert!(has_symbol(&r, "Dog"));
}

#[test]
fn javascript_variables() {
    let r = extract_fixture("javascript", "javascript/edge_cases.js");
    assert!(has_symbol(&r, "pipeline"));
}

#[test]
fn javascript_has_edges() {
    let r = extract_fixture("javascript", "javascript/edge_cases.js");
    assert!(
        !r.unresolved_edges.is_empty(),
        "Should have call/import edges"
    );
}

// ─── JSX ────────────────────────────────────────────────────────────────

#[test]
fn jsx_extracts_symbols() {
    let r = extract_fixture("javascript", "javascript/edge_cases.jsx");
    assert!(
        r.symbols.len() >= 8,
        "Expected ≥8 JSX symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn jsx_components() {
    let r = extract_fixture("javascript", "javascript/edge_cases.jsx");
    assert!(has_symbol(&r, "AppProvider"));
    assert!(has_symbol(&r, "TodoApp"));
    assert!(has_symbol(&r, "MouseTracker"));
    assert!(has_symbol(&r, "VirtualList"));
}

// ─── Go ─────────────────────────────────────────────────────────────────

#[test]
fn go_extracts_symbols() {
    let r = extract_fixture("go", "go/edge_cases.go");
    assert!(
        r.symbols.len() >= 20,
        "Expected ≥20 Go symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn go_structs() {
    let r = extract_fixture("go", "go/edge_cases.go");
    assert!(has_symbol_of_kind(&r, "Base", "type"));
    assert!(has_symbol_of_kind(&r, "User", "type"));
    assert!(has_symbol_of_kind(&r, "Config", "type"));
    assert!(has_symbol_of_kind(&r, "Stack", "type"));
}

#[test]
fn go_interfaces() {
    let r = extract_fixture("go", "go/edge_cases.go");
    assert!(has_symbol_of_kind(&r, "Reader", "type"));
    assert!(has_symbol_of_kind(&r, "ReadWriter", "type"));
}

#[test]
fn go_functions_and_methods() {
    let r = extract_fixture("go", "go/edge_cases.go");
    assert!(has_symbol_of_kind(&r, "Max", "function"));
    assert!(has_symbol_of_kind(&r, "fanOut", "function"));
    assert!(has_symbol_of_kind(&r, "describe", "function"));
    assert!(has_symbol(&r, "Push"));
    assert!(has_symbol(&r, "Pop"));
}

#[test]
fn go_fields() {
    let r = extract_fixture("go", "go/edge_cases.go");
    assert!(count_kind(&r, "field") >= 3, "Should have struct fields");
}

#[test]
fn go_constants_and_variables() {
    let r = extract_fixture("go", "go/edge_cases.go");
    let constants = count_kind(&r, "constant");
    let variables = count_kind(&r, "variable");
    assert!(
        constants + variables >= 2,
        "Should have constants/variables, got {constants} + {variables}"
    );
}

// ─── C ──────────────────────────────────────────────────────────────────

#[test]
fn c_extracts_symbols() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(
        r.symbols.len() >= 15,
        "Expected ≥15 C symbols, got {}",
        r.symbols.len()
    );
}

#[test]
fn c_structs() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(
        count_kind(&r, "struct") >= 2,
        "Should have multiple structs"
    );
}

#[test]
fn c_functions() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(has_symbol(&r, "context_new"));
    assert!(has_symbol(&r, "context_free"));
    assert!(has_symbol(&r, "sum_ints"));
    assert!(has_symbol(&r, "node_create"));
    assert!(has_symbol(&r, "matrix_create"));
    assert!(has_symbol(&r, "matrix_free"));
}

#[test]
fn c_macros() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(has_symbol_of_kind(&r, "ARRAY_SIZE", "macro"));
    assert!(has_symbol_of_kind(&r, "MIN", "macro"));
    assert!(has_symbol_of_kind(&r, "MAX", "macro"));
    assert!(has_symbol_of_kind(&r, "LOG", "macro"));
}

#[test]
fn c_enums() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(
        count_kind(&r, "enum_variant") + count_kind(&r, "enum") >= 1,
        "Should have enums/enumerators"
    );
}

#[test]
fn c_typedefs() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(
        count_kind(&r, "type_alias") >= 2,
        "Should have typedef aliases"
    );
}

#[test]
fn c_has_edges() {
    let r = extract_fixture("c", "c/edge_cases.c");
    assert!(
        !r.unresolved_edges.is_empty(),
        "Should have call edges in C"
    );
}

// ─── Cross-cutting concerns ─────────────────────────────────────────────

#[test]
fn all_symbols_have_nonempty_names() {
    for (lang, file) in [
        ("rust", "rust/edge_cases.rs"),
        ("python", "python/edge_cases.py"),
        ("typescript", "typescript/edge_cases.ts"),
        ("javascript", "javascript/edge_cases.js"),
        ("go", "go/edge_cases.go"),
        ("c", "c/edge_cases.c"),
    ] {
        let r = extract_fixture(lang, file);
        for sym in &r.symbols {
            assert!(
                !sym.name.is_empty(),
                "Empty symbol name in {lang}/{file}: {:?}",
                sym
            );
        }
    }
}

#[test]
fn all_symbols_have_valid_spans() {
    for (lang, file) in [
        ("rust", "rust/edge_cases.rs"),
        ("python", "python/edge_cases.py"),
        ("typescript", "typescript/edge_cases.ts"),
        ("javascript", "javascript/edge_cases.js"),
        ("go", "go/edge_cases.go"),
        ("c", "c/edge_cases.c"),
    ] {
        let r = extract_fixture(lang, file);
        for sym in &r.symbols {
            assert!(
                sym.span.end_byte >= sym.span.start_byte,
                "Invalid span in {lang}/{file} for {}: end < start",
                sym.name
            );
            assert!(
                sym.span.end_line >= sym.span.start_line,
                "Invalid span lines in {lang}/{file} for {}: end_line < start_line",
                sym.name
            );
        }
    }
}

#[test]
fn all_symbols_have_correct_language() {
    for (lang, file) in [
        ("rust", "rust/edge_cases.rs"),
        ("python", "python/edge_cases.py"),
        ("typescript", "typescript/edge_cases.ts"),
        ("javascript", "javascript/edge_cases.js"),
        ("go", "go/edge_cases.go"),
        ("c", "c/edge_cases.c"),
    ] {
        let r = extract_fixture(lang, file);
        for sym in &r.symbols {
            assert_eq!(
                sym.lang, lang,
                "Symbol {} has wrong lang: expected {lang}, got {}",
                sym.name, sym.lang
            );
        }
    }
}

#[test]
fn fqnames_contain_file_path() {
    for (lang, file) in [
        ("rust", "rust/edge_cases.rs"),
        ("python", "python/edge_cases.py"),
        ("typescript", "typescript/edge_cases.ts"),
        ("javascript", "javascript/edge_cases.js"),
        ("go", "go/edge_cases.go"),
        ("c", "c/edge_cases.c"),
    ] {
        let r = extract_fixture(lang, file);
        for sym in &r.symbols {
            assert!(
                sym.fqname.contains("edge_cases"),
                "FQName for {} in {lang} should contain file path: {}",
                sym.name,
                sym.fqname
            );
        }
    }
}
