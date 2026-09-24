//! Architecture fitness tests enforcing the L1–L7 module layering.
//!
//! See `docs/dev/rule_design_guide.md` for the role of each layer.
//!
//! `rust_arkitect`'s own engine recursively walks the whole crate directory (including
//! `scratch/` benchmark corpora), so these tests drive its rules over `src/` only.

#![allow(clippy::expect_used)]

use rust_arkitect::rule::Rule;
use rust_arkitect::rules::must_not_depend_on::MustNotDependOnRule;
use rust_arkitect::rules::utils::IsChild;
use rust_arkitect::rust_file::RustFile;
use std::fmt;
use std::path::{Path, PathBuf};

const CRATE_NAME: &str = "omni";

/// Layered modules, lowest first: a module must not depend on any module of a higher layer.
/// Unlisted modules (the crate root, module roots, binaries) form L7 and are unrestricted.
const LAYERS: &[&[&str]] = &[
    // L1: primitives.
    &["diagnostic", "diff"],
    // L2: domain vocabulary and grammar encapsulation.
    &["core", "code_lint::ast", "command_lint::vcs"],
    // L3: semantic engines.
    &[
        "code_lint::bindings",
        "code_lint::calls",
        "code_lint::comments",
    ],
    // L4: rule contracts.
    &["code_lint::rule", "command_lint::rule", "test_utils"],
    // L5: rules.
    &[
        "code_lint::rules",
        "code_lint::suppression",
        "command_lint::rules",
    ],
    // L6: domain orchestration runners.
    &["code_lint::runner", "command_lint::runner"],
];

/// Modules whose submodules are independent rules that must not import each other.
const RULE_FAMILIES: &[&str] = &["code_lint::rules", "command_lint::rules"];

/// The only modules allowed to handle raw ast-grep types.
const AST_GREP_OWNERS: &[&str] = &["code_lint::ast", "command_lint::rule", "bin::ast_dumper"];

/// Returns the logical path `rust_arkitect` assigns to the crate-relative `module`.
fn logical_path(module: &str) -> String {
    format!("{CRATE_NAME}::{module}")
}

/// Returns every spelling under which `rust_arkitect` reports a dependency on `module`.
///
/// It resolves `crate::` to the crate name in `use` trees but keeps it verbatim in
/// expression and type paths, so both spellings must be forbidden.
fn dependency_spellings(module: &str) -> [String; 2] {
    [logical_path(module), format!("crate::{module}")]
}

fn must_not_depend_on<'a>(
    subject: &str,
    forbidden: impl IntoIterator<Item = &'a str>,
) -> MustNotDependOnRule {
    MustNotDependOnRule::new(
        logical_path(subject),
        forbidden
            .into_iter()
            .flat_map(dependency_spellings)
            .collect(),
    )
}

/// Restricts `inner` to files outside the `exempt` modules.
struct Except {
    inner: MustNotDependOnRule,
    exempt: Vec<String>,
}

impl fmt::Display for Except {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (except in {})",
            self.inner,
            self.exempt.join(", ")
        )
    }
}

impl Rule for Except {
    fn apply(&self, file: &RustFile) -> Result<(), String> {
        self.inner.apply(file)
    }

    fn is_applicable(&self, file: &RustFile) -> bool {
        self.inner.is_applicable(file)
            && !self
                .exempt
                .iter()
                .any(|module| file.logical_path.is_child_of(module))
    }
}

fn layering_rules() -> Vec<Box<dyn Rule>> {
    LAYERS
        .iter()
        .enumerate()
        .flat_map(|(level, modules)| {
            let higher_modules: Vec<&str> = LAYERS[level + 1..].concat();
            modules.iter().map(move |module| {
                Box::new(must_not_depend_on(module, higher_modules.iter().copied()))
                    as Box<dyn Rule>
            })
        })
        .collect()
}

fn rule_isolation_rules() -> Vec<Box<dyn Rule>> {
    let rule_modules: Vec<String> = RULE_FAMILIES
        .iter()
        .flat_map(|family| {
            let dir = src_dir().join(family.replace("::", "/"));
            rust_files(&dir).into_iter().map(move |path| {
                let stem = path
                    .file_stem()
                    .expect("rule files have a stem")
                    .to_string_lossy();
                format!("{family}::{stem}")
            })
        })
        .collect();
    rule_modules
        .iter()
        .map(|module| {
            let siblings = rule_modules
                .iter()
                .map(String::as_str)
                .filter(|other| other != module);
            Box::new(must_not_depend_on(module, siblings)) as Box<dyn Rule>
        })
        .collect()
}

fn ast_grep_encapsulation_rules() -> Vec<Box<dyn Rule>> {
    vec![Box::new(Except {
        inner: MustNotDependOnRule::new(CRATE_NAME.to_owned(), vec!["ast_grep_core".to_owned()]),
        exempt: AST_GREP_OWNERS
            .iter()
            .map(|module| logical_path(module))
            .collect(),
    })]
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively lists the `.rs` files under `dir`.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).expect("source directory should be readable");
    entries
        .map(|entry| entry.expect("directory entry should be readable").path())
        .flat_map(|path| {
            if path.is_dir() {
                rust_files(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                vec![path]
            } else {
                vec![]
            }
        })
        .collect()
}

fn violations_in(files: &[RustFile], rules: &[Box<dyn Rule>]) -> Vec<String> {
    files
        .iter()
        .flat_map(|file| {
            rules
                .iter()
                .filter(|rule| rule.is_applicable(file))
                .filter_map(|rule| {
                    rule.apply(file)
                        .err()
                        .map(|error| format!("{rule}: {error}"))
                })
        })
        .collect()
}

fn assert_src_complies_with(rules: &[Box<dyn Rule>]) {
    let files: Vec<RustFile> = rust_files(&src_dir())
        .iter()
        .map(|path| RustFile::from_file_system(path.to_str().expect("source paths are UTF-8")))
        .collect();
    let violations = violations_in(&files, rules);
    assert!(
        violations.is_empty(),
        "Architecture violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_layers_only_depend_downward() {
    assert_src_complies_with(&layering_rules());
}

#[test]
fn test_rules_do_not_import_each_other() {
    assert_src_complies_with(&rule_isolation_rules());
}

#[test]
fn test_ast_grep_is_encapsulated() {
    assert_src_complies_with(&ast_grep_encapsulation_rules());
}

/// Returns the lines of `source` that give an item a second path.
///
/// Every item keeps one canonical path: a visible `use` (re-export) would hide the defining
/// layer from the dependency rules above. The one exception is a `macro_rules!` macro
/// declaring its own path next to its definition, because such macros cannot carry a
/// visibility. `#[macro_export]` is banned because it moves a macro to the crate root.
fn second_path_declarations(source: &str) -> Vec<&str> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| {
            let is_visible_use = line.starts_with("pub use ")
                || (line.starts_with("pub(") && line.contains(") use "));
            *line == "#[macro_export]" || (is_visible_use && !declares_own_macro_path(line, source))
        })
        .collect()
}

/// Returns true if `line` is `pub(crate) use name;` for a `macro_rules! name` defined in `source`.
fn declares_own_macro_path(line: &str, source: &str) -> bool {
    line.strip_prefix("pub(crate) use ")
        .and_then(|rest| rest.strip_suffix(';'))
        .is_some_and(|name| source.contains(&format!("macro_rules! {name} {{")))
}

#[test]
fn test_items_have_a_single_path() {
    let violations: Vec<String> = rust_files(&src_dir())
        .iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).expect("source files should be readable");
            second_path_declarations(&source)
                .into_iter()
                .map(|line| format!("{}: {line}", path.display()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "Second paths found:\n{}",
        violations.join("\n")
    );
}

/// Enforces that relative imports (`use super::...`) are never used in production code,
/// maintaining unambiguous `crate::` canonical paths across the crate.
#[test]
fn test_no_relative_imports_in_production_code() {
    let violations: Vec<String> = rust_files(&src_dir())
        .iter()
        .filter(|path| !path.ends_with("test_utils.rs"))
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).expect("source files should be readable");
            let mut in_test_module = false;
            let mut file_violations = Vec::new();

            for (idx, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("mod tests") {
                    in_test_module = true;
                }
                if !in_test_module && trimmed.starts_with("use super::") {
                    file_violations.push(format!("{}:{}: {trimmed}", path.display(), idx + 1));
                }
            }
            file_violations
        })
        .collect();
    assert!(
        violations.is_empty(),
        "Relative imports found in production code:\n{}",
        violations.join("\n")
    );
}

/// Guards against vacuous passes: re-exports and exported macros are caught, while a macro
/// declaring its own path is not.
#[test]
fn test_second_path_declarations_are_detected() {
    let source = indoc::indoc! {"
        macro_rules! local {
            () => {};
        }
        pub(crate) use local;
        pub use crate::diagnostic::RuleName;
        pub(crate) use crate::core::Tag;
        #[macro_export]
        macro_rules! exported {
            () => {};
        }
    "};
    assert_eq!(
        second_path_declarations(source),
        vec![
            "pub use crate::diagnostic::RuleName;",
            "pub(crate) use crate::core::Tag;",
            "#[macro_export]",
        ]
    );
}

/// Guards against vacuous passes: an upward dependency must be caught in both the `use`
/// and the inline-path spelling.
#[test]
fn test_layering_rules_detect_upward_dependencies() {
    let offending_rule = RustFile::from_content(
        "src/code_lint/rules/offending.rs",
        &logical_path("code_lint::rules::offending"),
        "use crate::code_lint::runner::lint_file;\nfn f() { crate::code_lint::runner::lint_file(); }\n",
    );
    let violations = violations_in(&[offending_rule], &layering_rules());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("omni::code_lint::runner::lint_file"),
        "{violations:?}"
    );
    assert!(
        violations[0].contains("crate::code_lint::runner::lint_file"),
        "{violations:?}"
    );
}
