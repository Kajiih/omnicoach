//! Enforces that environment variables are only accessed at module/static scope or explicit configuration boundaries.

use crate::code_lint::ast::{self, ParsedFile};
use crate::code_lint::contract::{CodeRule, RuleTarget};
use crate::code_lint::semantic::calls;
use crate::diagnostic::{Diagnostic, Language, RuleName, ViolationTemplate, violation_template};
use crate::rule_declaration::{
    Classification, Consensus, Declaration, Example, FilterListDefaults, ImpactedQuality, ListKind,
    ListOption, Precision, Reference, RuleDoc, RuleOptions, Topic,
};
use std::collections::HashSet;
use std::path::Path;

const BANNED: ListOption = ListOption {
    kind: ListKind::Deny,
    doc: "Environment variable accesses flagged inside functions.",
    default: FilterListDefaults {
        base: &[],
        extend: &[
            (
                Language::Python,
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
                Language::Rust,
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
        remove: &[],
    },
};

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: "Function `{function}` accesses an environment variable through `{expression}`.",
    rationale: "An environment variable accessed inside a function is hidden global state: the function's behavior depends on the process, and parallel tests that set the variable pollute each other.",
    suggestion: {
        base: "Load environment variables at the composition root (`main`, `from_env`) and pass a typed configuration value into `{function}`.",
        Python => "Load environment variables at the composition root (`main`, `from_env`, or module scope) and pass a typed configuration value into `{function}`.",
        Rust => "Load environment variables at the composition root (`main`, `from_env`, or a `LazyLock`) and pass a typed configuration value into `{function}`.",
    },
};

/// The rule's declaration.
pub const RULE: CodeRule<ListOption> = CodeRule {
    declaration: Declaration {
        name: RuleName("environment-variable-in-function"),
        template: &TEMPLATE,
        languages: &[Language::Python, Language::Rust],
        options: RuleOptions::code_rule(BANNED),
        classification: Classification {
            topics: &[Topic::GLOBAL_STATE],
            precision: Precision::Heuristic,
            consensus: Consensus::Opinionated,
            impacted_quality: ImpactedQuality::Reliability,
        },
        doc: RuleDoc {
            summary: "Flags environment variable access inside functions.",
            what_it_does: indoc::indoc! {r"
                Flags reads and writes of environment variables inside a function or method in
                source files; test files are not checked. In Python this covers `os.getenv`,
                `os.putenv`, `os.unsetenv`, the `os.environ` methods `get`, `pop`, `setdefault`,
                `update` and `clear`, and `os.environ[...]` subscripts, also when written as
                `getenv` or `environ` after a `from os import`. In Rust it covers `std::env::var`,
                `var_os`, `vars`, `vars_os`, `set_var` and `remove_var`, written with an
                `std::env::` or `env::` prefix. The deny list configures the calls; Python
                `environ[...]` subscripts are always checked. Access at module, class or `static`
                scope is allowed, and so is the compile-time `env!` macro. Functions named
                `from_env`, `from_environ` or `load_env`, and a `main` at the top level of the file,
                are configuration boundaries: access inside them, including inside functions,
                closures and lambdas nested in them, is not flagged. A method or `mod`-level
                function named `main` is not a boundary."},
            why_is_this_bad: indoc::indoc! {r"
                A function that reads the environment depends on hidden global state: its signature
                does not say what configuration it needs, and its behaviour changes with the process
                that runs it. Tests must then set and restore environment variables, which leaks
                between tests and races when tests run in parallel. Writing the environment from
                several threads is undefined behaviour on some platforms.

                Read the environment once, at startup or in a `from_env` constructor, into a typed
                configuration object, and pass that object, or the values it holds, to the code that
                needs them."},
            references: &[Reference {
                title: "Rust docs: std::env::set_var (safety)",
                url: "https://doc.rust-lang.org/std/env/fn.set_var.html",
            }],
            examples: &[
                Example {
                    language: Language::Python,
                    flagged: indoc::indoc! {r#"
                        def connect_database() -> Connection:
                            return psycopg.connect(os.environ["DATABASE_URL"])
                    "#},
                    flagged_span: r#"os.environ["DATABASE_URL"]"#,
                    fixed: indoc::indoc! {r"
                        def connect_database(database_url: str) -> Connection:
                            return psycopg.connect(database_url)
                    "},
                },
                Example {
                    language: Language::Rust,
                    flagged: indoc::indoc! {r#"
                        fn connect_database() -> Result<Connection, Error> {
                            let database_url = std::env::var("DATABASE_URL")?;
                            Connection::open(&database_url)
                        }
                    "#},
                    flagged_span: r#"std::env::var("DATABASE_URL")"#,
                    fixed: indoc::indoc! {r"
                        fn connect_database(database_url: &str) -> Result<Connection, Error> {
                            Connection::open(database_url)
                        }
                    "},
                },
            ],
        },
    },
    target: RuleTarget::SourceOnly,
    check: check_file,
};

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

/// Template placeholder for the environment read.
const EXPRESSION: &str = "expression";
/// Template placeholder for the enclosing function.
const FUNCTION: &str = "function";

fn check_file(
    rule: &CodeRule<ListOption>,
    path: &Path,
    file: &ParsedFile,
    banned_calls: &HashSet<String>,
) -> Vec<Diagnostic> {
    let lang = file.lang();
    let mut diagnostics = Vec::new();

    for call_match in calls::find_banned_calls(file, banned_calls) {
        if let Some(func_name) = ast::enclosing_non_exempt_function_name(
            &call_match.node,
            lang,
            is_exempt_boundary_function,
        ) {
            let expression = format!("{}()", call_match.callee);
            diagnostics.push(rule.diagnostic_at_node(
                path,
                &call_match.node,
                &[(EXPRESSION, &expression), (FUNCTION, &func_name)],
            ));
        }
    }

    if lang == Language::Python {
        for subscript in ast::python::collect_environ_subscripts(file) {
            if let Some(func_name) = ast::enclosing_non_exempt_function_name(
                &subscript.node,
                lang,
                is_exempt_boundary_function,
            ) {
                let expression = format!("{}[...]", subscript.mapping.text());
                diagnostics.push(rule.diagnostic_at_node(
                    path,
                    &subscript.node,
                    &[(EXPRESSION, &expression), (FUNCTION, &func_name)],
                ));
            }
        }
    }

    diagnostics
}

#[cfg(test)]
crate::test_utils::rule_test!(
    RULE,
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
