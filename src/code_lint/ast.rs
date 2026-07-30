//! AST helper predicates for structural traversal.

use ast_grep_language::SupportLang;

/// Checks if a tree-sitter node represents a variable binding definition in Rust.
#[must_use]
pub fn is_rust_variable_binding(node: &ast_grep_core::Node<'_, ast_grep_core::source::StrDoc<SupportLang>>) -> bool {
    let kind = node.kind();
    
    if kind == "shorthand_field_identifier" {
        return true;
    }
    
    if kind == "identifier" {
        // Exclude uppercase names (variants/constants like None, Ok, MAX)
        if let Some(first_char) = node.text().chars().next() {
            if first_char.is_ascii_uppercase() {
                return false;
            }
        }
        if node.text() == "_" {
            return false;
        }

        let mut current = node.clone();
        while let Some(parent) = current.parent() {
            let p_kind = parent.kind();
            
            // Closure parameter names are always bindings
            if p_kind == "closure_parameters" {
                return true;
            }
            
            // Intercept match pattern guards (condition is not a pattern context)
            if p_kind == "match_pattern" {
                if let Some(cond) = parent.field("condition") {
                    if cond.range() == current.range() {
                        return false;
                    }
                }
            }
            
            // If the parent exposes a "pattern" field, check if we are in it
            if let Some(pattern) = parent.field("pattern") {
                if pattern.range() == current.range() {
                    return true;
                }
            }
            
            // Special handling for tuple struct patterns: exclude type constructor name
            if p_kind == "tuple_struct_pattern" {
                if let Some(type_node) = parent.field("type") {
                    if type_node.range() == current.range() {
                        return false;
                    }
                }
                return true;
            }
            
            // Stop traversal if we hit a boundary node, but weren't in its pattern field
            if p_kind == "let_declaration"
                || p_kind == "let_condition"
                || p_kind == "for_expression"
                || p_kind == "match_arm"
                || p_kind == "parameter"
                || p_kind == "field_pattern"
                || p_kind == "struct_pattern"
                || p_kind == "generic_pattern"
                || p_kind == "range_pattern"
                || p_kind == "const_item"
                || p_kind == "static_item"
            {
                return false;
            }
            
            current = parent;
        }
    }
    false
}

/// Checks if a tree-sitter node represents a variable binding definition in Python.
#[must_use]
pub fn is_python_variable_binding(node: &ast_grep_core::Node<'_, ast_grep_core::source::StrDoc<SupportLang>>) -> bool {
    let kind = node.kind();
    if kind != "identifier" {
        return false;
    }
    if node.text() == "_" {
        return false;
    }
    
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        let p_kind = parent.kind();
        
        // Simple parameters & lambda parameters
        if p_kind == "parameters" || p_kind == "lambda_parameters" {
            return true;
        }
        
        // Complex parameters: def foo(b: int = 1):
        if p_kind == "typed_parameter" || p_kind == "default_parameter" || p_kind == "typed_default_parameter" {
            if let Some(name_node) = parent.field("name") {
                return name_node.range() == current.range();
            }
            return false;
        }
        
        // Walrus operator variables: (x := 1)
        if p_kind == "named_expression" {
            if let Some(name_node) = parent.field("name") {
                return name_node.range() == current.range();
            }
            return false;
        }
        
        // Left-hand side of assignments, loops, and comprehensions
        if let Some(left) = parent.field("left") {
            if left.range() == current.range() {
                return true;
            }
        }
        
        // Aliases in with statements and except clauses
        if let Some(alias) = parent.field("alias") {
            if alias.range() == current.range() {
                return true;
            }
        }
        
        // Pattern Matching: case clauses
        if p_kind == "case_clause" {
            return true;
        }
        if p_kind == "dotted_name" {
            if parent.text().contains('.') {
                return false; // Multi-part constant lookup, not a binding
            }
        }
        if p_kind == "keyword_pattern" {
            if let Some(first_child) = parent.child(0) {
                if first_child.range() == current.range() {
                    return false; // Keyword parameter name, not a binding
                }
            }
        }
        if p_kind == "class_pattern" {
            if let Some(first_child) = parent.child(0) {
                if first_child.range() == current.range() {
                    return false; // Match class type name, not a binding
                }
            }
        }
        
        // Stop walking up if we hit a boundary node but weren't in its binding fields
        if p_kind == "assignment"
            || p_kind == "for_statement"
            || p_kind == "list_comprehension"
            || p_kind == "dictionary_comprehension"
            || p_kind == "set_comprehension"
            || p_kind == "generator_expression"
            || p_kind == "with_item"
            || p_kind == "except_clause"
            || p_kind == "function_definition"
            || p_kind == "class_definition"
            || p_kind == "lambda"
            || p_kind == "match_statement"
        {
            return false;
        }
        
        current = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_core::AstGrep;

    fn get_nodes_by_text<'a>(
        grep: &'a AstGrep<ast_grep_core::source::StrDoc<SupportLang>>,
        text: &str,
    ) -> Vec<ast_grep_core::Node<'a, ast_grep_core::source::StrDoc<SupportLang>>> {
        grep.root()
            .dfs()
            .filter(|n| n.text() == text && n.children().len() == 0)
            .collect()
    }

    #[test]
    fn test_rust_is_variable_binding() {
        // let assignment
        let grep = AstGrep::new("fn main() { let a = b; }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "a")[0]));
        assert!(!is_rust_variable_binding(&get_nodes_by_text(&grep, "b")[0]));

        // mut binding
        let grep = AstGrep::new("fn main() { let mut c = 1; }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "c")[0]));

        // tuple destructuring
        let grep = AstGrep::new("fn main() { let (d, e) = (1, 2); }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "d")[0]));
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "e")[0]));

        // struct destructuring
        let grep = AstGrep::new("fn main() { let Point { x: f, y: _ } = p; }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "f")[0]));
        assert!(!is_rust_variable_binding(&get_nodes_by_text(&grep, "x")[0]));
        assert!(!is_rust_variable_binding(&get_nodes_by_text(&grep, "_")[0]));

        // struct shorthand destructuring
        let grep = AstGrep::new("fn main() { let Point { g, h } = p; }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "g")[0]));
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "h")[0]));

        // loop variables
        let grep = AstGrep::new("fn main() { for i in 0..10 {} }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "i")[0]));

        // closure parameter
        let grep = AstGrep::new("fn main() { let f = |x: i32| x + 1; }", SupportLang::Rust);
        // get closure param "x", not the reference "x" inside body
        let x_nodes = get_nodes_by_text(&grep, "x");
        assert!(is_rust_variable_binding(&x_nodes[0])); // first is definition
        assert!(!is_rust_variable_binding(&x_nodes[1])); // second is reference

        // function parameters
        let grep = AstGrep::new("fn test(j: i32) {}", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "j")[0]));
        assert!(!is_rust_variable_binding(&get_nodes_by_text(&grep, "test")[0]));

        // match variants (ident is variant or constructor -> ignore if uppercase)
        let grep = AstGrep::new("fn main() { match val { Some(k) => {}, None => {} } }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "k")[0]));
        assert!(!is_rust_variable_binding(&get_nodes_by_text(&grep, "Some")[0]));
        assert!(!is_rust_variable_binding(&get_nodes_by_text(&grep, "None")[0]));

        // if-let and while-let
        let grep = AstGrep::new("fn main() { if let Some(y) = val {} while let Some(z) = val {} }", SupportLang::Rust);
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "y")[0]));
        assert!(is_rust_variable_binding(&get_nodes_by_text(&grep, "z")[0]));

        // match guard variable reference must be ignored
        let grep = AstGrep::new("fn main() { match val { Some(w) if w > 0 => {} } }", SupportLang::Rust);
        let w_nodes = get_nodes_by_text(&grep, "w");
        assert!(is_rust_variable_binding(&w_nodes[0])); // inside pattern is binding
        assert!(!is_rust_variable_binding(&w_nodes[1])); // inside guard is reference
    }

    #[test]
    fn test_python_is_variable_binding() {
        // assignments
        let grep = AstGrep::new("c = 2", SupportLang::Python);
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "c")[0]));

        // multi-assignments
        let grep = AstGrep::new("d, e = 3, 4", SupportLang::Python);
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "d")[0]));
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "e")[0]));

        // loop targets
        let grep = AstGrep::new("for x in range(10): pass", SupportLang::Python);
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "x")[0]));

        // comprehensions
        let grep = AstGrep::new("[y for y in range(10)]", SupportLang::Python);
        let y_nodes = get_nodes_by_text(&grep, "y");
        assert!(!is_python_variable_binding(&y_nodes[0])); // reference
        assert!(is_python_variable_binding(&y_nodes[1]));  // binding

        // typed parameter
        let grep = AstGrep::new("def foo(b: int = 1): pass", SupportLang::Python);
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "b")[0]));
        assert!(!is_python_variable_binding(&get_nodes_by_text(&grep, "int")[0]));
        assert!(!is_python_variable_binding(&get_nodes_by_text(&grep, "foo")[0]));

        // exception alias
        let grep = AstGrep::new("try:\n    pass\nexcept Exception as g:\n    pass", SupportLang::Python);
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "g")[0]));
        assert!(!is_python_variable_binding(&get_nodes_by_text(&grep, "Exception")[0]));

        // walrus expressions
        let grep = AstGrep::new("(v := 1)", SupportLang::Python);
        assert!(is_python_variable_binding(&get_nodes_by_text(&grep, "v")[0]));
    }
}
