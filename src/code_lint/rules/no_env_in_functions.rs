//! Enforces that environment variables are only accessed at module/static scope or explicit configuration boundaries (`no-env-in-functions`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, DenyListConfig, DynamicRuleConfig, FilterListDefaults, Rule, RuleName};
use crate::diagnostic::{Diagnostic, ViolationTemplate, violation_template};
use crate::rules::Tag;
use ast_grep_core::AstGrep;
use ast_grep_language::SupportLang;
use std::path::Path;

/// Default banned environment variable access callees across Python and Rust.
const DEFAULT_BANNED_CALLS: FilterListDefaults = FilterListDefaults {
    base: &[],
    extend: &[
        (
            SupportLang::Python,
            &[
                "os.getenv",
                "getenv",
                "os.environ.get",
                "environ.get",
                "os.environ.pop",
                "environ.pop",
                "os.environ.setdefault",
                "environ.setdefault",
                "os.environ.update",
                "environ.update",
                "os.environ.clear",
                "environ.clear",
                "os.putenv",
                "os.unsetenv",
            ],
        ),
        (
            SupportLang::Rust,
            &[
                "std::env::var",
                "env::var",
                "std::env::var_os",
                "env::var_os",
                "std::env::vars",
                "env::vars",
                "std::env::vars_os",
                "env::vars_os",
                "std::env::set_var",
                "env::set_var",
                "std::env::remove_var",
                "env::remove_var",
            ],
        ),
    ],
    exempt: &[],
};

/// Configuration for the `NoEnvInFunctions` rule.
pub type NoEnvInFunctionsConfig = DynamicRuleConfig<DenyListConfig>;

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Environment variable access `{expr}` inside function `{func_name}`.",
    rationale: "Accessing environment variables deep inside functions introduces hidden global state, breaks dependency injection, and makes unit testing brittle.",
    suggestion: {
        base: "Load environment variables at startup or configuration boundaries (`main`, `from_env`, or module scope) and pass them as explicit arguments or typed config objects.",
        Python => "Load environment variables at module/class scope or inside `main()` / `from_env()` and pass them as explicit arguments or a typed config object.",
        Rust => "Load environment variables inside `main()`, `from_env()`, or a module-level `LazyLock` and pass them as explicit arguments or a `Config` struct.",
    },
};

/// Rule that flags direct environment variable reads/writes inside regular functions and methods.
pub struct NoEnvInFunctions;

impl Rule for NoEnvInFunctions {
    fn name(&self) -> RuleName {
        RuleName("no-env-in-functions")
    }

    fn tags(&self) -> &'static [Tag] {
        &[Tag::SideEffects, Tag::Opinionated]
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Returns true if `func_node` is declared directly at file scope, rather than nested inside a
/// `class`, `impl`, `trait`, `mod`, another function, or a conditional block.
fn is_top_level_function(func_node: &AstNode<'_>) -> bool {
    func_node
        .parent()
        .is_some_and(|parent| matches!(parent.kind().as_ref(), "source_file" | "module"))
}

/// Returns true if `func_node` is an entrypoint or configuration boundary allowed to access env vars.
///
/// `from_env` / `from_environ` / `load_env` are idiomatic loader constructors and are exempt
/// anywhere, but `main` is only an entrypoint at file scope: a `main` nested in a class, `impl`,
/// or `mod` is ordinary business logic and stays subject to the rule.
// TODO: Should we make those hardcoded values constants/configurable?
fn is_exempt_boundary_function(func_node: &AstNode<'_>, name: &str) -> bool {
    match name {
        "from_env" | "from_environ" | "load_env" => true,
        "main" => is_top_level_function(func_node),
        _ => false,
    }
}

/// Returns the nearest enclosing function name if `node` is inside a function and no enclosing
/// function is an exempt configuration boundary (`main`, `from_env`, `from_environ`, `load_env`).
///
/// The exemption is *inherited*: a nested function, closure, or lambda declared inside a boundary
/// is part of that boundary's implementation and cannot be called or substituted from the outside,
/// so the testability rationale behind this rule does not apply to it. The reported name is
/// deliberately the *nearest* enclosing function, so the diagnostic points at the innermost context
/// even though the exemption considers every ancestor.
///
/// Known limitation: a class declared inside a boundary whose methods escape (returned, registered)
/// also inherits the exemption.
fn enclosing_non_exempt_function_name(node: &AstNode<'_>, lang: SupportLang) -> Option<String> {
    let func_kind = match lang {
        SupportLang::Rust => "function_item",
        _ => "function_definition",
    };

    let mut nearest_function_name: Option<String> = None;

    for ancestor in node.ancestors() {
        if ancestor.kind() == func_kind
            && let Some(name_node) = ancestor.field("name")
        {
            let func_name = name_node.text().to_string();
            if is_exempt_boundary_function(&ancestor, &func_name) {
                return None;
            }
            if nearest_function_name.is_none() {
                nearest_function_name = Some(func_name);
            }
        }
    }

    nearest_function_name
}

/// Returns the formatted subscript label (`"os.environ[...]"` or `"environ[...]"`) if `node`
/// indexes into Python's `os.environ` or `environ` mapping.
fn python_environ_subscript_label(node: &AstNode<'_>) -> Option<&'static str> {
    if node.kind() != "subscript" {
        return None;
    }
    let value = node.field("value")?;
    match value.kind().as_ref() {
        "identifier" if value.text() == "environ" => Some("environ[...]"),
        "attribute" => {
            let obj = value.field("object")?;
            let attr = value.field("attribute")?;
            (obj.text() == "os" && attr.text() == "environ").then_some("os.environ[...]")
        }
        _ => None,
    }
}

impl CodeRule for NoEnvInFunctions {
    fn target(&self) -> RuleTarget {
        RuleTarget::SourceOnly
    }

    fn check_file(
        &self,
        path: &Path,
        grep: &AstGrep<SourceDoc>,
        config: &Config,
    ) -> Vec<Diagnostic> {
        let lang = *grep.lang();
        let rule_config: NoEnvInFunctionsConfig = config.get_rule_config(self.name().0);
        let effective_banned = rule_config.effective_banned_for_lang(lang, &DEFAULT_BANNED_CALLS);

        let mut diagnostics = Vec::new();

        for call_match in crate::code_lint::calls::find_banned_calls(grep, &effective_banned) {
            if let Some(func_name) = enclosing_non_exempt_function_name(&call_match.node, lang) {
                let expr = format!("{}()", call_match.callee);
                diagnostics.push(self.diagnostic_at_node(
                    path,
                    &call_match.node,
                    &[("expr", &expr), ("func_name", &func_name)],
                ));
            }
        }

        if lang == SupportLang::Python {
            for subscript_node in grep.root().dfs().filter(|node| node.kind() == "subscript") {
                if let Some(label) = python_environ_subscript_label(&subscript_node)
                    && let Some(func_name) =
                        enclosing_non_exempt_function_name(&subscript_node, lang)
                {
                    diagnostics.push(self.diagnostic_at_node(
                        path,
                        &subscript_node,
                        &[("expr", label), ("func_name", &func_name)],
                    ));
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_code_rule_snapshot;
    use indoc::indoc;

    #[test]
    fn test_python_env_in_functions_flagged_and_boundaries_allowed() {
        let source = indoc! {r#"
            import os
            from os import environ, getenv

            # Allowed at module scope
            MODULE_KEY = os.getenv("API_KEY")
            MODULE_HOST = os.environ["HOST"]
            MODULE_PORT = environ.get("PORT", "8080")

            class Settings:
                # Allowed at class body scope
                DEFAULT_TIMEOUT = os.getenv("TIMEOUT", "30")

                @classmethod
                def from_env(cls) -> "Settings":
                    # Allowed inside explicit boundary constructor
                    return cls(os.getenv("API_KEY"), os.environ["HOST"])

                def connect(self) -> str:
                    # Flagged inside regular method
                    token = os.getenv("API_TOKEN")
                    region = os.environ["AWS_REGION"]
                    return f"{token}:{region}"

            def main() -> None:
                # Allowed inside entrypoint
                _ = getenv("APP_ENV")

            def load_env() -> dict[str, str]:
                # Allowed inside explicit env loader
                return {"url": environ["DATABASE_URL"]}

            def fetch_orders() -> str:
                # Flagged inside regular function
                key = getenv("SECRET_KEY")
                endpoint = os.environ.get("ENDPOINT")
                fallback = environ["FALLBACK_URL"]
                return f"{key}:{endpoint}:{fallback}"
        "#};

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoEnvInFunctions, source, "src/service.py"),
            @"
        [no-env-in-functions] Line 20, Col 17: Environment variable access `os.getenv()` inside function `connect`.
        [no-env-in-functions] Line 21, Col 18: Environment variable access `os.environ[...]` inside function `connect`.
        [no-env-in-functions] Line 34, Col 11: Environment variable access `getenv()` inside function `fetch_orders`.
        [no-env-in-functions] Line 35, Col 16: Environment variable access `os.environ.get()` inside function `fetch_orders`.
        [no-env-in-functions] Line 36, Col 16: Environment variable access `environ[...]` inside function `fetch_orders`.
        "
        );
    }

    #[test]
    fn test_python_mutators_methods_and_nested_scopes() {
        let source = indoc! {r#"
            import os
            from os import environ, getenv

            class Worker:
                def main(self) -> None:
                    # Flagged: a method named `main` is not the application entrypoint
                    self.token = os.getenv("TOKEN")

            def configure_overrides() -> None:
                # Flagged: mutating the process environment is a hidden side effect
                os.environ.update({"DEBUG": "1"})
                environ.clear()
                os.environ["FORCE"] = "1"

            def main() -> None:
                # Allowed: nested scopes inherit the entrypoint exemption
                def helper():
                    return os.environ.get("DB_URL")

                loader = lambda: getenv("API_KEY")

            def retrieve_data():
                # Flagged: lambda inside a regular function is still hidden env access
                fetcher = lambda: getenv("API_KEY")
        "#};

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoEnvInFunctions, source, "src/worker.py"),
            @"
        [no-env-in-functions] Line 7, Col 22: Environment variable access `os.getenv()` inside function `main`.
        [no-env-in-functions] Line 11, Col 5: Environment variable access `os.environ.update()` inside function `configure_overrides`.
        [no-env-in-functions] Line 12, Col 5: Environment variable access `environ.clear()` inside function `configure_overrides`.
        [no-env-in-functions] Line 13, Col 5: Environment variable access `os.environ[...]` inside function `configure_overrides`.
        [no-env-in-functions] Line 24, Col 23: Environment variable access `getenv()` inside function `retrieve_data`.
        "
        );
    }

    #[test]
    fn test_rust_non_top_level_main_is_not_an_entrypoint() {
        let source = indoc! {r#"
            use std::env;

            pub struct Worker;

            impl Worker {
                fn main(&self) -> String {
                    env::var("TOKEN").unwrap_or_default()
                }
            }

            mod app {
                use std::env;

                fn main() {
                    let _ = env::var("APP_MODE");
                }
            }

            fn main() {
                let _ = env::var("TOKEN");
            }
        "#};

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoEnvInFunctions, source, "src/worker.rs"),
            @"
        [no-env-in-functions] Line 7, Col 9: Environment variable access `env::var()` inside function `main`.
        [no-env-in-functions] Line 15, Col 17: Environment variable access `env::var()` inside function `main`.
        "
        );
    }

    #[test]
    fn test_rust_env_in_functions_flagged_and_boundaries_allowed() {
        let source = indoc! {r#"
            use std::env;
            use std::sync::LazyLock;

            // Allowed in module-level static initializer
            static GLOBAL_TOKEN: LazyLock<String> = LazyLock::new(|| {
                std::env::var("GLOBAL_TOKEN").unwrap_or_default()
            });

            // Allowed compile-time macro
            fn build_version() -> &'static str {
                env!("CARGO_PKG_VERSION")
            }

            pub struct Config {
                pub host: String,
            }

            impl Config {
                pub fn from_env() -> Self {
                    Self {
                        host: env::var("SERVICE_HOST").unwrap_or_default(),
                    }
                }

                pub fn refresh(&mut self) {
                    self.host = std::env::var("SERVICE_HOST").unwrap_or_default();
                }
            }

            fn main() {
                let _ = env::var_os("RUST_LOG");
            }

            fn send_request() -> Option<String> {
                let closure = || env::var("API_KEY").ok();
                closure()
            }
        "#};

        insta::assert_snapshot!(
            assert_code_rule_snapshot(&NoEnvInFunctions, source, "src/config.rs"),
            @"
        [no-env-in-functions] Line 26, Col 21: Environment variable access `std::env::var()` inside function `refresh`.
        [no-env-in-functions] Line 35, Col 22: Environment variable access `env::var()` inside function `send_request`.
        "
        );
    }

    #[test]
    fn test_config_extend_and_test_scope_exemption() {
        let config_toml = r#"
            [rules.no-env-in-functions]
            extend_banned = ["dotenv.get_key"]
            allowed = ["getenv"]
        "#;
        let config: Config = toml::from_str(config_toml).unwrap();

        let source = indoc! {r#"
            from os import getenv
            import dotenv

            def load_secret() -> str:
                allowed_call = getenv("ALLOWED")
                custom_banned = dotenv.get_key(".env", "SECRET")
                return f"{allowed_call}:{custom_banned}"
        "#};

        insta::assert_snapshot!(
            crate::test_utils::assert_code_rule_snapshot_with_config(
                &NoEnvInFunctions,
                source,
                "src/secret.py",
                &config
            ),
            @"[no-env-in-functions] Line 6, Col 21: Environment variable access `dotenv.get_key()` inside function `load_secret`."
        );

        // Test files are automatically skipped via RuleTarget::SourceOnly in lint_file
        let test_diags = crate::code_lint::lint_file(
            Path::new("tests/test_secret.py"),
            source,
            &Config::default(),
        );
        assert!(test_diags.is_empty());
    }
}
