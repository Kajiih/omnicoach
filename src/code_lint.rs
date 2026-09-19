//! Static file structure analysis domain using ast-grep-core.

pub mod ast_python;
pub mod ast_rust;
pub mod calls;
pub mod rules;
pub mod suppression;

use crate::core::Config;
use crate::diagnostic::Diagnostic;
use ast_grep_core::AstGrep;
pub use ast_grep_language::SupportLang;
use std::path::Path;

/// Concrete document type used across code linting rules.
pub type SourceDoc = ast_grep_core::tree_sitter::StrDoc<SupportLang>;
/// Concrete AST node type used across code linting rules.
pub type AstNode<'a> = ast_grep_core::Node<'a, SourceDoc>;

/// Target execution scope for a code rule (source files vs test files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuleTarget {
    /// Rule runs on all matching files.
    #[default]
    All,
    /// Rule runs exclusively on test files.
    TestsOnly,
    /// Rule runs exclusively on production source files (skipped on test files).
    SourceOnly,
}

/// Common trait for static file code validation rules.
pub trait CodeRule: crate::core::Rule {
    /// Returns the target execution scope of this code rule (defaults to `RuleTarget::All`).
    #[must_use]
    fn target(&self) -> RuleTarget {
        RuleTarget::All
    }

    /// Returns true if this rule supports the given language.
    #[must_use]
    fn supports_language(&self, lang: SupportLang) -> bool {
        self.supported_languages().contains(&lang)
    }

    /// Renders this rule's violation template at the given AST node using the node's language.
    #[must_use]
    fn diagnostic_at_node(
        &self,
        path: &Path,
        node: &AstNode<'_>,
        params: &[(&str, &str)],
    ) -> Diagnostic {
        self.render_diagnostic_for_lang(
            *node.lang(),
            params,
            crate::diagnostic::SourceLocation::from_node(path, node),
        )
    }

    /// Evaluates the file against this static analysis rule.
    #[must_use]
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic>;
}

/// Helper to detect language from file extension.
#[must_use]
pub fn detect_language(path: &Path) -> Option<SupportLang> {
    path.extension().and_then(std::ffi::OsStr::to_str).and_then(|ext| match ext {
        "py" => Some(SupportLang::Python),
        "rs" => Some(SupportLang::Rust),
        _ => None,
    })
}

/// Returns true if `rule` should be evaluated on `path` given its language and test-file context.
fn should_evaluate_rule(
    rule: &dyn CodeRule,
    path: &Path,
    lang: SupportLang,
    is_test: bool,
    has_inline_tests: bool,
    config: &Config,
) -> bool {
    if rule.tags().contains(&crate::rules::Tag::Suppression) {
        return false;
    }
    if !config.is_rule_enabled_for_path(rule, path) || !rule.supports_language(lang) {
        return false;
    }
    match rule.target() {
        RuleTarget::SourceOnly => !is_test,
        RuleTarget::TestsOnly => is_test || has_inline_tests,
        RuleTarget::All => true,
    }
}

/// Filters rule diagnostics according to `RuleTarget` and inline `#[cfg(test)]` / `#[test]` byte ranges.
fn filter_diagnostics_by_target(
    rule_diagnostics: Vec<Diagnostic>,
    target: RuleTarget,
    is_test: bool,
    inline_test_ranges: &[std::ops::Range<usize>],
) -> Vec<Diagnostic> {
    if is_test || inline_test_ranges.is_empty() {
        return rule_diagnostics;
    }
    match target {
        RuleTarget::TestsOnly => rule_diagnostics
            .into_iter()
            .filter(|diagnostic| {
                let pos = diagnostic.location.span.start;
                inline_test_ranges.iter().any(|range| range.contains(&pos))
            })
            .collect(),
        RuleTarget::SourceOnly => rule_diagnostics
            .into_iter()
            .filter(|diagnostic| {
                let pos = diagnostic.location.span.start;
                !inline_test_ranges.iter().any(|range| range.contains(&pos))
            })
            .collect(),
        RuleTarget::All => rule_diagnostics,
    }
}

/// Analyzes the structure of a file and returns diagnostic alerts.
#[must_use]
pub fn lint_file(path: &Path, content: &str, config: &Config) -> Vec<Diagnostic> {
    let Some(lang) = detect_language(path) else {
        return Vec::new();
    };

    let grep = AstGrep::new(content, lang);
    let mut tracker = suppression::SuppressionTracker::from_ast(&grep, content);

    let is_test = config.is_test_path(path);
    let inline_test_ranges = if !is_test && lang == SupportLang::Rust {
        ast_rust::collect_inline_test_ranges(&grep.root())
    } else {
        Vec::new()
    };
    let has_inline_tests = !inline_test_ranges.is_empty();
    let mut raw_diagnostics = Vec::new();

    for rule in crate::rules::CODE_RULES {
        if !should_evaluate_rule(*rule, path, lang, is_test, has_inline_tests, config) {
            continue;
        }
        let rule_diagnostics = rule.check_file(path, &grep, config);
        raw_diagnostics.extend(filter_diagnostics_by_target(
            rule_diagnostics,
            rule.target(),
            is_test,
            &inline_test_ranges,
        ));
    }

    let mut diagnostics = tracker.filter_diagnostics(raw_diagnostics);
    diagnostics.extend(tracker.audit(path, config));
    diagnostics
}

/// Collects all outermost test function and test method nodes (`def test_*` in Python, `#[test]` / `fn test_*` in Rust).
#[must_use]
pub fn collect_test_functions<'a>(root: &AstNode<'a>, lang: SupportLang) -> Vec<AstNode<'a>> {
    let mut functions = Vec::new();
    collect_outer_test_functions(root, lang, &mut functions);
    functions
}

fn collect_outer_test_functions<'a>(
    node: &AstNode<'a>,
    lang: SupportLang,
    out: &mut Vec<AstNode<'a>>,
) {
    let target_kind = match lang {
        SupportLang::Rust => "function_item",
        _ => "function_definition",
    };
    if node.kind() == target_kind {
        let is_test = match lang {
            SupportLang::Rust => ast_rust::is_test_function(node),
            SupportLang::Python => ast_python::is_test_function(node),
            _ => false,
        };
        if is_test {
            out.push(node.clone());
        }
        return;
    }
    for child in node.children() {
        collect_outer_test_functions(&child, lang, out);
    }
}

/// Helper to collect binding definition nodes from a parsed AST grep document.
#[must_use]
pub fn collect_bindings(grep: &AstGrep<SourceDoc>) -> Vec<AstNode<'_>> {
    match grep.lang() {
        SupportLang::Rust => ast_rust::collect_bindings(&grep.root()),
        SupportLang::Python => ast_python::collect_bindings(&grep.root()),
        _ => Vec::new(),
    }
}

/// Returns true if the node represents an import binding.
#[must_use]
pub fn is_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => matches!(
            parent_kind.as_ref(),
            "use_declaration" | "use_list" | "use_as_clause" | "scoped_identifier"
        ),
        SupportLang::Python => matches!(
            parent_kind.as_ref(),
            "import_statement" | "import_from_statement" | "aliased_import" | "dotted_name"
        ),
        _ => false,
    }
}

/// Returns true if the node represents an unaliased import binding (an external symbol
/// imported directly without a local `as` alias).
#[must_use]
pub fn is_unaliased_import_binding(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => {
            matches!(parent_kind.as_ref(), "use_declaration" | "use_list" | "scoped_identifier")
        }
        SupportLang::Python => matches!(
            parent_kind.as_ref(),
            "import_statement" | "import_from_statement" | "dotted_name"
        ),
        _ => false,
    }
}

/// Returns true if the node represents a structural type, class, or function definition name.
#[must_use]
pub fn is_structural_definition(node: &AstNode<'_>, lang: SupportLang) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    match lang {
        SupportLang::Rust => matches!(
            parent_kind.as_ref(),
            "struct_item"
                | "enum_item"
                | "trait_item"
                | "type_item"
                | "associated_type"
                | "function_item"
        ),
        SupportLang::Python => {
            matches!(parent_kind.as_ref(), "class_definition" | "function_definition")
        }
        _ => false,
    }
}

/// Returns true if the node is the name of a member defined inside a trait implementation
/// (`impl Trait for Type`), i.e. a name mandated by the trait contract.
///
/// Such names cannot be changed without breaking the implementation, so naming rules that
/// suggest renaming do not meaningfully apply to them. Only the member name itself qualifies;
/// nested names the author does control, such as parameters and locals, do not.
#[must_use]
pub fn is_trait_impl_member(node: &AstNode<'_>, lang: SupportLang) -> bool {
    if lang != SupportLang::Rust {
        return false;
    }
    // The name must belong directly to an associated item ...
    let Some(item) = node.parent() else {
        return false;
    };
    if !matches!(
        item.kind().as_ref(),
        "function_item" | "type_item" | "associated_type" | "const_item"
    ) {
        return false;
    }
    // ... declared in the body of an `impl` block ...
    let Some(body) = item.parent() else {
        return false;
    };
    if body.kind().as_ref() != "declaration_list" {
        return false;
    }
    // ... that names a trait.
    let Some(impl_item) = body.parent() else {
        return false;
    };
    impl_item.kind().as_ref() == "impl_item" && impl_item.field("trait").is_some()
}
