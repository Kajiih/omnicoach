//! What a Python expression names: dotted path extraction and imported path references.

use crate::code_lint::ast::imports::{QualifiedName, ResolvedName, import_lines, resolve_name};
use crate::code_lint::ast::tree::{ParsedFile, span_from_ruff_range};
use ruff_python_ast::Expr;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr};
use ruff_text_size::Ranged as _;

/// Collects `(line, imported_path)` references for all imports and imported-name usages in `file`.
#[must_use]
pub(in crate::code_lint) fn collect_path_references(file: &ParsedFile) -> Vec<(usize, String)> {
    struct PathRefVisitor<'a> {
        file: &'a ParsedFile,
        out: Vec<(usize, String)>,
    }

    impl<'a> SourceOrderVisitor<'a> for PathRefVisitor<'a> {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if matches!(expr, Expr::Name(_) | Expr::Attribute(_))
                && let ResolvedName::Imported(imported) =
                    resolve_name(self.file, &written_name(expr, &self.file.source).path)
            {
                let line = self.file.line_index.line(expr.range().start().into());
                self.out.push((line, imported));
            }
            walk_expr(self, expr);
        }
    }

    let Some(parsed) = file.py_module() else {
        return Vec::new();
    };
    let mut visitor = PathRefVisitor {
        file,
        out: import_lines(file).to_vec(),
    };
    visitor.visit_body(&parsed.syntax().body);
    visitor.out
}

/// The dotted path and terminal identifier of `expr` as written in `source`.
pub(super) fn written_name(expr: &Expr, source: &str) -> QualifiedName {
    let span = span_from_ruff_range(expr.range());
    let path = source[span.start..span.end].to_string();
    match expr {
        Expr::Attribute(attr) => QualifiedName {
            path,
            terminal: attr.attr.to_string(),
        },
        _ => QualifiedName::from_path(path, "."),
    }
}
