//! Python decorators: expressions applied to a definition, resolved through imports, with
//! arguments.

use super::names::written_name;
use crate::code_lint::ast::ParsedFile;
use ruff_python_ast::{Decorator, Expr};

/// Structured metadata for a Python decorator.
#[derive(Clone)]
pub(super) struct DecoratorInfo {
    /// Full path string as written (e.g. `"dataclasses.dataclass"`, `"pytest.mark.parametrize"`).
    path: String,
    /// Terminal identifier as written (e.g. `"dataclass"`, `"parametrize"`).
    terminal_name: String,
    /// Canonical path resolved through [`crate::code_lint::ast::resolve_name`], or `None` when the
    /// first segment is a module-level definition in the file.
    resolved_path: Option<String>,
    /// Keyword argument names if invoked as a call.
    keyword_args: Vec<String>,
}

impl DecoratorInfo {
    /// Returns true if a keyword argument with `key` was explicitly passed.
    #[must_use]
    pub(super) fn has_arg(&self, key: &str) -> bool {
        self.keyword_args.iter().any(|arg| arg == key)
    }

    /// Returns true if the import-resolved decorator path equals one of `candidates`,
    /// excluding decorators whose first segment is defined in the file.
    #[must_use]
    pub(super) fn matches_any(&self, candidates: &[&str]) -> bool {
        self.resolved_path
            .as_deref()
            .is_some_and(|path| candidates.contains(&path))
    }

    /// Returns true if this decorator is an attribute access (`@<receiver>.<attr>`) whose
    /// attribute name is in `attrs` (e.g. `@prop.setter`, `@func.register`).
    #[must_use]
    pub(super) fn is_attribute_named(&self, attrs: &[&str]) -> bool {
        self.path.contains('.') && attrs.contains(&self.terminal_name.as_str())
    }
}

/// Converts a slice of `ruff_python_ast::Decorator` nodes into `DecoratorInfo` values.
pub(super) fn extract_decorators_from_slice(
    decorators: &[Decorator],
    file: &ParsedFile,
) -> Vec<DecoratorInfo> {
    let mut out = Vec::with_capacity(decorators.len());
    for decorator in decorators {
        let (target_expr, keyword_args) = if let Expr::Call(call) = &decorator.expression {
            let kwargs = call
                .arguments
                .keywords
                .iter()
                .filter_map(|kw| Some(kw.arg.as_ref()?.id.to_string()))
                .collect();
            (call.func.as_ref(), kwargs)
        } else {
            (&decorator.expression, Vec::new())
        };

        let written = written_name(target_expr, &file.source);
        let resolved_path = written.clone().resolve(file).map(|name| name.path);
        out.push(DecoratorInfo {
            path: written.path,
            terminal_name: written.terminal,
            resolved_path,
            keyword_args,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use ruff_python_ast::{Stmt, StmtFunctionDef};

    /// Returns the first top-level function definition of `file`.
    fn first_function_def(file: &ParsedFile) -> &StmtFunctionDef {
        let parsed = file.py_module().expect("source should parse");
        let Some(Stmt::FunctionDef(func_def)) = parsed.syntax().body.first() else {
            unreachable!("source should start with a function definition");
        };
        func_def
    }

    /// Extracts the decorators of the first top-level function definition in `file`.
    fn first_function_decorators(file: &ParsedFile) -> Vec<DecoratorInfo> {
        extract_decorators_from_slice(&first_function_def(file).decorator_list, file)
    }

    #[test]
    fn test_extract_decorators_count() {
        let source = indoc::indoc! {r#"
            @dataclass(frozen=True, slots=False)
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);
        assert_eq!(decorators.len(), 3);
    }

    #[test]
    fn test_extract_decorator_keyword_args() {
        let source = indoc::indoc! {r"
            @dataclass(frozen=True, slots=False)
            def foo():
                pass
        "};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);

        let dec0 = &decorators[0];
        assert_eq!(dec0.terminal_name, "dataclass");
        assert!(dec0.has_arg("frozen"));
        assert!(dec0.has_arg("slots"));
        assert!(!dec0.has_arg("unknown"));
    }

    #[test]
    fn test_extract_decorators_metadata_and_path() {
        let source = indoc::indoc! {r#"
            @pytest.mark.parametrize("x", [1, 2])
            @custom
            def foo():
                pass
        "#};
        let file = ParsedFile::new(source, Language::Python);
        let decorators = first_function_decorators(&file);
        let dec0 = &decorators[0];
        let dec1 = &decorators[1];

        assert_eq!(dec0.terminal_name, "parametrize");
        assert_eq!(dec0.path, "pytest.mark.parametrize");
        assert_eq!(dec1.terminal_name, "custom");
        assert_eq!(dec1.path, "custom");
    }
}
