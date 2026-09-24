//! Enforces that environment variables are only accessed at module/static scope or explicit configuration boundaries (`no-env-in-functions`).

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::rule::{CodeRule, RuleTarget};
use crate::core::{Config, FilterListDefaults, Rule, Tag};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
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

/// Returns true if a function named `name` is an entrypoint or configuration boundary allowed to
/// access env vars.
///
/// `from_env` / `from_environ` / `load_env` are idiomatic loader constructors and are exempt
/// anywhere, but `main` is only an entrypoint at file scope (`is_top_level`): a `main` nested in a
/// class, `impl`, or `mod` is ordinary business logic and stays subject to the rule.
///
/// The exemption is *inherited* by nested functions, closures, and lambdas (see
/// [`ast::enclosing_non_exempt_function_name`]): they are part of the boundary's implementation
/// and cannot be called or substituted from the outside, so the testability rationale behind this
/// rule does not apply to them.
///
/// Known limitation: a class declared inside a boundary whose methods escape (returned, registered)
/// also inherits the exemption.
// TODO: Should we make those hardcoded values constants/configurable?
fn is_exempt_boundary_function(name: &str, is_top_level: bool) -> bool {
    match name {
        "from_env" | "from_environ" | "load_env" => true,
        "main" => is_top_level,
        _ => false,
    }
}

impl CodeRule for NoEnvInFunctions {
    fn target(&self) -> RuleTarget {
        RuleTarget::SourceOnly
    }

    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic> {
        let lang = file.lang();
        let mut diagnostics = Vec::new();

        for call_match in self.find_configured_banned_calls(file, config, &DEFAULT_BANNED_CALLS) {
            if let Some(func_name) = ast::enclosing_non_exempt_function_name(
                &call_match.node,
                lang,
                is_exempt_boundary_function,
            ) {
                let expr = format!("{}()", call_match.callee);
                diagnostics.push(self.diagnostic_at_node(
                    path,
                    &call_match.node,
                    &[("call", &expr), ("expr", &expr), ("func_name", &func_name)],
                ));
            }
        }

        if lang == SupportLang::Python {
            for (subscript_node, label) in ast::python::collect_environ_subscripts(file) {
                if let Some(func_name) = ast::enclosing_non_exempt_function_name(
                    &subscript_node,
                    lang,
                    is_exempt_boundary_function,
                ) {
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
crate::test_utils::rule_test!(
    NoEnvInFunctions,
    {
        Python => {
            pass: [
                module_scope_call_allowed => r#"
                    import os
                    from os import getenv

                    MODULE_KEY = os.getenv("API_KEY")
                    MODULE_PORT = getenv("PORT", "8080")
                "#,
                module_scope_subscript_allowed => r#"
                    import os

                    MODULE_HOST = os.environ["HOST"]
                "#,
                class_scope_call_allowed => r#"
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
                from_environ_boundary_allowed => r#"
                    from os import environ

                    def from_environ() -> dict[str, str]:
                        return {"token": environ["TOKEN"]}
                "#,
                load_env_boundary_allowed => r#"
                    from os import environ

                    def load_env() -> dict[str, str]:
                        return {"url": environ["DATABASE_URL"]}
                "#,
                main_entrypoint_allowed => r#"
                    from os import getenv

                    def main() -> None:
                        _ = getenv("APP_ENV")
                "#,
                nested_fn_in_main_exempt => r#"
                    import os

                    def main() -> None:
                        def helper():
                            return os.environ.get("DB_URL")
                "#,
                lambda_in_main_exempt => r#"
                    from os import getenv

                    def main() -> None:
                        loader = lambda: getenv("API_KEY")
                "#,
                dict_subscript_allowed => r#"
                    def fetch_orders(config: dict[str, str]) -> str:
                        return config["HOST"]
                "#,
                non_os_environ_subscript_allowed => r#"
                    def fetch_orders(client) -> str:
                        return client.environ["PORT"]
                "#,
            ],
            fail: [
                env_call_in_function_flagged => r#"
                    from os import getenv

                    def fetch_orders() -> str:
                        return getenv("SECRET_KEY")
                "# => r#"getenv("SECRET_KEY")"#,
                environ_mutation_call_in_function_flagged => r#"
                    import os

                    def fetch_orders() -> None:
                        os.environ.update({"DEBUG": "1"})
                "# => r#"os.environ.update({"DEBUG": "1"})"#,
                environ_get_call_in_function_flagged => r#"
                    import os

                    def fetch_orders() -> str:
                        return os.environ.get("ENDPOINT")
                "# => r#"os.environ.get("ENDPOINT")"#,
                env_call_in_method_flagged => r#"
                    import os

                    class Settings:
                        def connect(self) -> str:
                            return os.getenv("API_TOKEN")
                "# => r#"os.getenv("API_TOKEN")"#,
                bare_environ_subscript_in_function_flagged => r#"
                    from os import environ

                    def fetch_orders() -> str:
                        return environ["FALLBACK_URL"]
                "# => r#"environ["FALLBACK_URL"]"#,
                environ_subscript_in_method_flagged => r#"
                    import os

                    class Settings:
                        def connect(self) -> str:
                            return os.environ["AWS_REGION"]
                "# => r#"os.environ["AWS_REGION"]"#,
                method_named_main_not_exempt => r#"
                    import os

                    class Worker:
                        def main(self) -> None:
                            self.token = os.getenv("TOKEN")
                "# => r#"os.getenv("TOKEN")"#,
                lambda_in_function_flagged => r#"
                    from os import getenv

                    def retrieve_data():
                        fetcher = lambda: getenv("API_KEY")
                "# => r#"getenv("API_KEY")"#,
                nested_fn_in_function_flagged => r#"
                    from os import getenv

                    def retrieve_data():
                        def helper():
                            return getenv("API_KEY")
                "# => r#"getenv("API_KEY")"#,
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
                from_environ_boundary_allowed => r#"
                    use std::env;

                    pub struct Config {
                        pub host: String,
                    }

                    impl Config {
                        pub fn from_environ() -> Self {
                            Self {
                                host: env::var("SERVICE_HOST").unwrap_or_default(),
                            }
                        }
                    }
                "#,
                load_env_boundary_allowed => r#"
                    use std::env;

                    fn load_env() -> Result<String, env::VarError> {
                        env::var("DATABASE_URL")
                    }
                "#,
                top_level_main_allowed => r#"
                    use std::env;

                    fn main() {
                        let _ = env::var("PORT");
                    }
                "#,
                nested_fn_in_main_exempt => r#"
                    use std::env;

                    fn main() {
                        fn read_log() -> Option<std::ffi::OsString> {
                            env::var_os("RUST_LOG")
                        }
                        let _ = read_log();
                    }
                "#,
                closure_in_main_exempt => r#"
                    use std::env;

                    fn main() {
                        let loader = || env::var("PORT").ok();
                        let _ = loader();
                    }
                "#,
            ],
            fail: [
                env_call_in_function_flagged => r#"
                    use std::env;

                    fn fetch_host() -> String {
                        env::var("SERVICE_HOST").unwrap_or_default()
                    }
                "# => r#"env::var("SERVICE_HOST")"#,
                method_env_call_flagged => r#"
                    pub struct Config {
                        pub host: String,
                    }

                    impl Config {
                        pub fn refresh(&mut self) {
                            self.host = std::env::var("SERVICE_HOST").unwrap_or_default();
                        }
                    }
                "# => r#"std::env::var("SERVICE_HOST")"#,
                closure_in_function_flagged => r#"
                    use std::env;

                    fn send_request() -> Option<String> {
                        let closure = || env::var("API_KEY").ok();
                        closure()
                    }
                "# => r#"env::var("API_KEY")"#,
                nested_fn_in_function_flagged => r#"
                    use std::env;

                    fn send_request() -> Option<String> {
                        fn helper() -> Option<String> {
                            env::var("API_KEY").ok()
                        }
                        helper()
                    }
                "# => r#"env::var("API_KEY")"#,
                method_named_main_not_exempt => r#"
                    use std::env;

                    pub struct Worker;

                    impl Worker {
                        fn main(&self) -> String {
                            env::var("TOKEN").unwrap_or_default()
                        }
                    }
                "# => r#"env::var("TOKEN")"#,
                mod_scoped_main_not_exempt => r#"
                    mod app {
                        use std::env;

                        fn main() {
                            let _ = env::var("APP_MODE");
                        }
                    }
                "# => r#"env::var("APP_MODE")"#,
            ],
        },
    }
);
