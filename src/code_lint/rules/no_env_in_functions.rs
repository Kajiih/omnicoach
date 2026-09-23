//! Enforces that environment variables are only accessed at module/static scope or explicit configuration boundaries (`no-env-in-functions`).

use crate::code_lint::{AstNode, CodeRule, RuleTarget, SourceDoc};
use crate::core::{Config, FilterListDefaults, Rule, RuleName};
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

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Environment variable access `{call}` inside function `{func_name}`.",
    rationale: "Reading ambient environment variables inside functions introduces hidden global state and causes cross-test pollution during parallel test execution.",
    suggestion: {
        base: "Load environment variables at composition boundaries (`main`, `from_env`) and pass a typed configuration parameter into `{func_name}`.",
        Python => "Load environment variables at composition boundaries (`main`, `from_env`, or module/class scope) and pass a typed configuration parameter into `{func_name}`.",
        Rust => "Load environment variables at composition boundaries (`main`, `from_env`, or a module-level `LazyLock`) and pass a typed configuration parameter into `{func_name}`.",
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
        let mut diagnostics = Vec::new();

        for call_match in self.find_configured_banned_calls(grep, config, &DEFAULT_BANNED_CALLS) {
            if let Some(func_name) = enclosing_non_exempt_function_name(&call_match.node, lang) {
                let expr = format!("{}()", call_match.callee);
                diagnostics.push(self.diagnostic_at_node(
                    path,
                    &call_match.node,
                    &[("call", &expr), ("expr", &expr), ("func_name", &func_name)],
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
                        &[("call", label), ("expr", label), ("func_name", &func_name)],
                    ));
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
crate::rule_test!(
    NoEnvInFunctions,
    {
        Python => {
            pass: [
                module_scope_allowed => r#"
                    import os
                    from os import environ, getenv

                    MODULE_KEY = os.getenv("API_KEY")
                    MODULE_HOST = os.environ["HOST"]
                    MODULE_PORT = environ.get("PORT", "8080")
                "#,
                class_scope_allowed => r#"
                    import os

                    class Settings:
                        DEFAULT_TIMEOUT = os.getenv("TIMEOUT", "30")
                "#,
                from_env_boundary_allowed => r#"
                    import os

                    class Settings:
                        @classmethod
                        def from_env(cls) -> "Settings":
                            return cls(os.getenv("API_KEY"), os.environ["HOST"])
                "#,
                main_entrypoint_allowed => r#"
                    from os import getenv

                    def main() -> None:
                        _ = getenv("APP_ENV")
                "#,
                load_env_boundary_allowed => r#"
                    from os import environ

                    def load_env() -> dict[str, str]:
                        return {"url": environ["DATABASE_URL"]}
                "#,
                nested_in_main_allowed => r#"
                    import os
                    from os import getenv

                    def main() -> None:
                        def helper():
                            return os.environ.get("DB_URL")

                        loader = lambda: getenv("API_KEY")
                "#,
            ],
            fail: [
                function_env_call => r#"
                    from os import getenv

                    def fetch_orders() -> str:
                        return getenv("SECRET_KEY")
                "# => [r#"getenv("SECRET_KEY")"#],
                method_env_call => r#"
                    import os

                    class Settings:
                        def connect(self) -> str:
                            return os.getenv("API_TOKEN")
                "# => [r#"os.getenv("API_TOKEN")"#],
                method_environ_subscript => r#"
                    import os

                    class Settings:
                        def connect(self) -> str:
                            return os.environ["AWS_REGION"]
                "# => [r#"os.environ["AWS_REGION"]"#],
                environ_get_method => r#"
                    import os

                    def fetch_orders() -> str:
                        return os.environ.get("ENDPOINT")
                "# => [r#"os.environ.get("ENDPOINT")"#],
                environ_subscript => r#"
                    from os import environ

                    def fetch_orders() -> str:
                        return environ["FALLBACK_URL"]
                "# => [r#"environ["FALLBACK_URL"]"#],
                method_named_main => r#"
                    import os

                    class Worker:
                        def main(self) -> None:
                            self.token = os.getenv("TOKEN")
                "# => [r#"os.getenv("TOKEN")"#],
                environ_mutator => r#"
                    import os

                    def configure_overrides() -> None:
                        os.environ.update({"DEBUG": "1"})
                "# => [r#"os.environ.update({"DEBUG": "1"})"#],
                lambda_in_regular_function => r#"
                    from os import getenv

                    def retrieve_data():
                        fetcher = lambda: getenv("API_KEY")
                "# => [r#"getenv("API_KEY")"#],
            ],
        },
        Rust => {
            pass: [
                static_lazy_lock_allowed => r#"
                    use std::sync::LazyLock;

                    static GLOBAL_TOKEN: LazyLock<String> = LazyLock::new(|| {
                        std::env::var("GLOBAL_TOKEN").unwrap_or_default()
                    });
                "#,
                compile_time_env_macro_allowed => r#"
                    fn build_version() -> &'static str {
                        env!("CARGO_PKG_VERSION")
                    }
                "#,
                from_env_boundary_allowed => r#"
                    use std::env;

                    pub struct Config {
                        pub host: String,
                    }

                    impl Config {
                        pub fn from_env() -> Self {
                            Self {
                                host: env::var("SERVICE_HOST").unwrap_or_default(),
                            }
                        }
                    }
                "#,
                top_level_main_allowed => r#"
                    use std::env;

                    fn main() {
                        let _ = env::var_os("RUST_LOG");
                    }
                "#,
            ],
            fail: [
                method_env_call => r#"
                    pub struct Config {
                        pub host: String,
                    }

                    impl Config {
                        pub fn refresh(&mut self) {
                            self.host = std::env::var("SERVICE_HOST").unwrap_or_default();
                        }
                    }
                "# => [r#"std::env::var("SERVICE_HOST")"#],
                closure_in_function => r#"
                    use std::env;

                    fn send_request() -> Option<String> {
                        let closure = || env::var("API_KEY").ok();
                        closure()
                    }
                "# => [r#"env::var("API_KEY")"#],
                method_named_main => r#"
                    use std::env;

                    pub struct Worker;

                    impl Worker {
                        fn main(&self) -> String {
                            env::var("TOKEN").unwrap_or_default()
                        }
                    }
                "# => [r#"env::var("TOKEN")"#],
                module_nested_main => r#"
                    mod app {
                        use std::env;

                        fn main() {
                            let _ = env::var("APP_MODE");
                        }
                    }
                "# => [r#"env::var("APP_MODE")"#],
            ],
        },
    }
);
