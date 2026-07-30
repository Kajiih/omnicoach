//! Static file structure analysis domain using ast-grep-core.

pub mod ast_python;
pub mod ast_rust;
pub mod rules;

use crate::core::Config;
use crate::diagnostic::Diagnostic;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Common trait for static file code validation rules.
pub trait CodeRule: crate::core::Rule {
    /// Evaluates the file against this static analysis rule.
    #[must_use]
    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<ast_grep_core::source::StrDoc<SupportLang>>,
        config: &Config,
    ) -> Vec<Diagnostic>;
}

/// Helper to detect language from file extension.
#[must_use]
pub fn detect_language(path: &Path) -> Option<SupportLang> {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|ext| match ext {
            "py" => Some(SupportLang::Python),
            "rs" => Some(SupportLang::Rust),
            _ => None,
        })
}

const fn lang_to_tag(lang: SupportLang) -> Option<crate::rules::Tag> {
    match lang {
        SupportLang::Python => Some(crate::rules::Tag::Python),
        SupportLang::Rust => Some(crate::rules::Tag::Rust),
        _ => None,
    }
}

fn rule_supports_language(rule: &dyn CodeRule, lang: SupportLang) -> bool {
    lang_to_tag(lang)
        .is_some_and(|tag| rule.tags().contains(&tag))
}

/// Analyzes the structure of a file and returns diagnostic alerts.
#[must_use]
pub fn lint_file(path: &Path, content: &str, config: &Config) -> Vec<Diagnostic> {
    let Some(lang) = detect_language(path) else {
        return Vec::new();
    };

    let grep = AstGrep::new(content, lang);
    let mut diagnostics = Vec::new();

    for rule in crate::rules::CODE_RULES {
        if config.is_rule_enabled(*rule) && rule_supports_language(*rule, lang) {
            diagnostics.extend(rule.check_file(path, &grep, config));
        }
    }
    diagnostics
}
