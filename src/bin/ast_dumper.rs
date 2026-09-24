//! AST Dumper
//! A utility to dump concrete syntax trees for Rust and Python constructs.

use ast_grep_core::AstGrep;
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_language::SupportLang;

type AstNode<'a> = ast_grep_core::Node<'a, StrDoc<SupportLang>>;

fn print_tree(node: &AstNode<'_>, depth: usize) {
    let indent = "  ".repeat(depth);
    println!(
        "{}{:?} ({}) [{:?}]",
        indent,
        node.kind(),
        node.text(),
        node.range()
    );

    for child in node.children() {
        print_tree(&child, depth + 1);
    }
}

fn main() {
    println!("--- RUST STRUCT DESTRUCTURING EXPLICIT AST ---");
    let rust_source = "let Point { x: f, y: _ } = p;";
    let grep_rust = AstGrep::new(rust_source, SupportLang::Rust);
    print_tree(&grep_rust.root(), 0);

    println!("\n--- PYTHON COMPREHENSIONS AST ---");
    let python_source = indoc::indoc! {r"
        [a for a in range(10)]
        {b: b for b in range(10)}
        {c for c in range(10)}
        (d for d in range(10))
    "};
    let grep_python = AstGrep::new(python_source, SupportLang::Python);
    print_tree(&grep_python.root(), 0);
}
