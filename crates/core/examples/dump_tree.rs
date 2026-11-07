//! Print the tree-sitter syntax tree of a file: `cargo run --example dump_tree -- <lang> <file>`
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lang: tree_sitter::Language = match args[1].as_str() {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        other => panic!("unknown lang {other}"),
    };
    let src = std::fs::read(&args[2]).unwrap();
    let mut p = tree_sitter::Parser::new();
    p.set_language(&lang).unwrap();
    let tree = p.parse(&src, None).unwrap();
    let mut out = String::new();
    fn walk(n: tree_sitter::Node, field: Option<&str>, depth: usize, out: &mut String) {
        if !n.is_named() { return; }
        out.push_str(&"  ".repeat(depth));
        if let Some(f) = field { out.push_str(f); out.push_str(": "); }
        out.push_str(n.kind());
        out.push('\n');
        let mut c = n.walk();
        for (i, ch) in n.children(&mut c).enumerate() {
            walk(ch, n.field_name_for_child(i as u32), depth + 1, out);
        }
    }
    walk(tree.root_node(), None, 0, &mut out);
    print!("{out}");
}
