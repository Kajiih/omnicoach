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
    let mut raw_diagnostics = Vec::new();

    for rule in crate::rules::CODE_RULES {
        if rule.tags().contains(&crate::rules::Tag::Suppression) {
            continue;
        }

        if !config.is_rule_enabled_for_path(*rule, path) {
            continue;
        }

        let target = rule.target();
        if is_test && target == RuleTarget::SourceOnly {
            continue;
        }
        if !is_test && target == RuleTarget::TestsOnly && inline_test_ranges.is_empty() {
            continue;
        }

        if rule.supports_language(lang) {
            let rule_diagnostics = rule.check_file(path, &grep, config);
            if !is_test && !inline_test_ranges.is_empty() {
                match target {
                    RuleTarget::TestsOnly => {
                        raw_diagnostics.extend(rule_diagnostics.into_iter().filter(|diagnostic| {
                            let pos = diagnostic.location.span.start;
                            inline_test_ranges.iter().any(|range| range.contains(&pos))
                        }));
                    }
                    RuleTarget::SourceOnly => {
                        raw_diagnostics.extend(rule_diagnostics.into_iter().filter(|diagnostic| {
                            let pos = diagnostic.location.span.start;
                            !inline_test_ranges.iter().any(|range| range.contains(&pos))
                        }));
                    }
                    RuleTarget::All => {
                        raw_diagnostics.extend(rule_diagnostics);
                    }
                }
            } else {
                raw_diagnostics.extend(rule_diagnostics);
            }
        }
    }

    let mut diagnostics = tracker.filter_diagnostics(raw_diagnostics, content);
    let audit_diagnostics = tracker.audit(path, content, config);
    diagnostics.extend(audit_diagnostics);

    diagnostics
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
