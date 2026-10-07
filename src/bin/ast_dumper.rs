//! AST Dumper
//! A utility to dump syntax trees for Rust and Python constructs.

omni::architecture_component!(Bin);

use ra_ap_syntax::{Edition, SourceFile, SyntaxElement, SyntaxNode};

fn print_rust_tree(node: &SyntaxNode, depth: usize) {
    let indent = "  ".repeat(depth);
    println!(
        "{}{:?} ({:?}) [{:?}]",
        indent,
        node.kind(),
        node.text(),
        node.text_range()
    );

    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Node(child_node) => print_rust_tree(&child_node, depth + 1),
            SyntaxElement::Token(token) => {
                let token_indent = "  ".repeat(depth + 1);
                println!(
                    "{}{:?} ({:?}) [{:?}]",
                    token_indent,
                    token.kind(),
                    token.text(),
                    token.text_range()
                );
            }
        }
    }
}

fn main() {
    println!("--- RUST STRUCT DESTRUCTURING EXPLICIT AST ---");
    let rust_source = "fn sample() { let Point { x: first, y: _ } = point; }";
    let parsed_rust = SourceFile::parse(rust_source, Edition::Edition2024);
    print_rust_tree(&parsed_rust.syntax_node(), 0);

    println!("\n--- PYTHON COMPREHENSIONS AST ---");
    let python_source = indoc::indoc! {r"
        [a for a in range(10)]
        {b: b for b in range(10)}
        {c for c in range(10)}
        (d for d in range(10))
    "};
    let parsed_python = ruff_python_parser::parse_module(python_source);
    println!("{parsed_python:#?}");
}
