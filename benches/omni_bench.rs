//! End-to-end, semantic-stage, and marginal rule performance & allocation benchmarks for Omni.
//!
//! Run the full suite:
//! ```bash
//! cargo bench --bench omni_bench
//! ```
//!
//! Run a specific benchmark group (e.g., `parser`, `semantic`, `linter`, or `command_lint`):
//! ```bash
//! cargo bench --bench omni_bench -- parser
//! cargo bench --bench omni_bench -- semantic
//! cargo bench --bench omni_bench -- linter
//! cargo bench --bench omni_bench -- command_lint
//! ```

use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use divan::counter::BytesCount;
use divan::{AllocProfiler, Bencher, black_box};

use omni::code_lint::ast::{self, ParsedFile};
use omni::code_lint::contract::AnyCodeRule;
use omni::code_lint::rules::{self, CODE_RULES};
use omni::code_lint::runner::lint_file;
use omni::code_lint::suppression::SuppressionTracker;
use omni::command_lint::command::InterceptedCommand;
use omni::command_lint::runner::run_command_lint;
use omni::config::Config;
use omni::diagnostic::Language;
use omni::rule_selection::parse_config;

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    divan::main();
}

const PY_REAL_SMALL: &str = include_str!("fixtures/py_real_small.py.fixture");
const PY_REAL_TYPED_LIB: &str = include_str!("fixtures/py_real_typed_lib.py.fixture");
const PY_REAL_TEST_SUITE: &str = include_str!("fixtures/py_real_test_suite.py.fixture");
const PY_KITCHEN_SINK: &str = include_str!("fixtures/py_kitchen_sink.py.fixture");

const RS_REAL_SMALL: &str = include_str!("fixtures/rs_real_small.rs.fixture");
const RS_REAL_AST_MODULE: &str = include_str!("fixtures/rs_real_ast_module.rs.fixture");
const RS_REAL_TEST_SUITE: &str = include_str!("fixtures/rs_real_test_suite.rs.fixture");
const RS_KITCHEN_SINK: &str = include_str!("fixtures/rs_kitchen_sink.rs.fixture");

/// A pinned benchmark input file paired with its language and virtual file path.
///
/// The virtual path determines whether `RuleTarget::SourceOnly` or `RuleTarget::TestOnly` rules
/// apply to the file during end-to-end and marginal rule benchmarks.
#[derive(Clone, Copy)]
struct TestFile {
    label: &'static str,
    virtual_path: &'static str,
    language: Language,
    code: &'static str,
}

impl fmt::Display for TestFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size_kib = self.code.len() / 1024;
        write!(formatter, "{} ({size_kib} KiB)", self.label)
    }
}

const ALL_TEST_FILES: &[TestFile] = &[
    TestFile {
        label: "py_real_small",
        virtual_path: "src/numpy_globals.py",
        language: Language::Python,
        code: PY_REAL_SMALL,
    },
    TestFile {
        label: "py_real_typed_lib",
        virtual_path: "src/pydantic_types.py",
        language: Language::Python,
        code: PY_REAL_TYPED_LIB,
    },
    TestFile {
        label: "py_real_test_suite",
        virtual_path: "tests/test_dataset.py",
        language: Language::Python,
        code: PY_REAL_TEST_SUITE,
    },
    TestFile {
        label: "py_kitchen_sink",
        virtual_path: "src/kitchen_sink.py",
        language: Language::Python,
        code: PY_KITCHEN_SINK,
    },
    TestFile {
        label: "rs_real_small",
        virtual_path: "src/config.rs",
        language: Language::Rust,
        code: RS_REAL_SMALL,
    },
    TestFile {
        label: "rs_real_ast_module",
        virtual_path: "src/annotations.rs",
        language: Language::Rust,
        code: RS_REAL_AST_MODULE,
    },
    TestFile {
        label: "rs_real_test_suite",
        virtual_path: "tests/cli_test.rs",
        language: Language::Rust,
        code: RS_REAL_TEST_SUITE,
    },
    TestFile {
        label: "rs_kitchen_sink",
        virtual_path: "src/kitchen_sink.rs",
        language: Language::Rust,
        code: RS_KITCHEN_SINK,
    },
];

const PYTHON_TEST_FILES: &[TestFile] = &[
    ALL_TEST_FILES[0],
    ALL_TEST_FILES[1],
    ALL_TEST_FILES[2],
    ALL_TEST_FILES[3],
];

const RUST_TEST_FILES: &[TestFile] = &[
    ALL_TEST_FILES[4],
    ALL_TEST_FILES[5],
    ALL_TEST_FILES[6],
    ALL_TEST_FILES[7],
];

static DEFAULT_CONFIG: LazyLock<Config> = LazyLock::new(Config::default);

static NO_RULES_CONFIG: LazyLock<Config> =
    LazyLock::new(|| parse_config("select = []\n").unwrap_or_default());

/// Pre-warms all `OnceLock` caches on `file` so subsequent rule benchmarks measure only the
/// rule's own marginal work rather than first-caller shared index construction.
fn warm_shared_indexes(file: &ParsedFile) {
    assert!(
        !file.has_syntax_error(),
        "benchmark fixture must parse without syntax errors"
    );
    let _ = ast::collect_comment_nodes(file);
    let _ = ast::collect_bindings(file);
    let _ = ast::collect_call_candidates(file);
    let _ = ast::resolve_name(file, "probe");
    if file.lang() == Language::Rust {
        let _ = ast::rust::collect_inline_test_ranges(file);
    } else {
        let _ = ast::python::collect_locally_mutated_return_functions(file);
    }
}

/// Stage 1 (`parser`): Dedicated AST/CST parser construction (`ruff_python_parser` and
/// `ra_ap_syntax`) plus `LineIndex` build.
#[divan::bench_group]
mod parser {
    use super::{ALL_TEST_FILES, Bencher, BytesCount, ParsedFile, TestFile, black_box};

    #[divan::bench(args = ALL_TEST_FILES)]
    fn parse_file(bencher: Bencher<'_, '_>, test_file: &TestFile) {
        bencher
            .counter(BytesCount::of_str(test_file.code))
            .bench_local(|| ParsedFile::new(black_box(test_file.code), test_file.language));
    }
}

/// Stage 2 (`semantic`): Shared `OnceLock` semantic indexes and unmemoized AST extractors,
/// measured on a pre-parsed `ParsedFile` so parser cost is excluded.
#[divan::bench_group]
mod semantic {
    /// Shared extractors backed by `OnceLock` on `ParsedFile` (measured on a fresh pre-parsed
    /// `ParsedFile` per sample to capture cold index construction cost paid once per file).
    #[divan::bench_group]
    mod memoized_oncelock {
        use super::super::{
            ALL_TEST_FILES, Bencher, BytesCount, PYTHON_TEST_FILES, ParsedFile, RUST_TEST_FILES,
            SuppressionTracker, TestFile, ast, black_box,
        };

        #[divan::bench(args = ALL_TEST_FILES)]
        fn call_candidates(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .with_inputs(|| ParsedFile::new(test_file.code, test_file.language))
                .bench_local_refs(|file| ast::collect_call_candidates(black_box(file)).len());
        }

        #[divan::bench(args = ALL_TEST_FILES)]
        fn bindings(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .with_inputs(|| ParsedFile::new(test_file.code, test_file.language))
                .bench_local_refs(|file| ast::collect_bindings(black_box(file)).len());
        }

        #[divan::bench(args = ALL_TEST_FILES)]
        fn comment_nodes(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .with_inputs(|| ParsedFile::new(test_file.code, test_file.language))
                .bench_local_refs(|file| ast::collect_comment_nodes(black_box(file)).len());
        }

        #[divan::bench(args = ALL_TEST_FILES)]
        fn suppression_scan(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .with_inputs(|| ParsedFile::new(test_file.code, test_file.language))
                .bench_local_refs(|file| {
                    SuppressionTracker::from_file(black_box(file), black_box(test_file.code))
                });
        }

        #[divan::bench(args = RUST_TEST_FILES)]
        fn rust_inline_test_ranges(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .with_inputs(|| ParsedFile::new(test_file.code, test_file.language))
                .bench_local_refs(|file| {
                    ast::rust::collect_inline_test_ranges(black_box(file)).len()
                });
        }

        #[divan::bench(args = PYTHON_TEST_FILES)]
        fn py_symbol_and_mutation_index(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .with_inputs(|| ParsedFile::new(test_file.code, test_file.language))
                .bench_local_refs(|file| {
                    let _ = black_box(ast::resolve_name(black_box(file), "Sequence"));
                    ast::python::collect_locally_mutated_return_functions(black_box(file)).len()
                });
        }
    }

    /// Shared AST extractors that are NOT memoized in `OnceLock` and are re-executed by each rule
    /// that calls them.
    #[divan::bench_group]
    mod unmemoized_extractors {
        use super::super::{
            ALL_TEST_FILES, Bencher, BytesCount, PYTHON_TEST_FILES, ParsedFile, TestFile, ast,
            black_box, warm_shared_indexes,
        };

        #[divan::bench(args = PYTHON_TEST_FILES)]
        fn py_function_signatures(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            let file = ParsedFile::new(test_file.code, test_file.language);
            warm_shared_indexes(&file);
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .bench_local(|| ast::python::extract_function_signatures(black_box(&file)).len());
        }

        #[divan::bench(args = PYTHON_TEST_FILES)]
        fn py_parameter_usages(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            let file = ParsedFile::new(test_file.code, test_file.language);
            warm_shared_indexes(&file);
            let signatures = ast::python::extract_function_signatures(&file);
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .bench_local(|| {
                    signatures
                        .iter()
                        .map(|signature| {
                            ast::python::summarize_parameter_usages(black_box(signature)).len()
                        })
                        .sum::<usize>()
                });
        }

        #[divan::bench(args = PYTHON_TEST_FILES)]
        fn py_classes(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            let file = ParsedFile::new(test_file.code, test_file.language);
            warm_shared_indexes(&file);
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .bench_local(|| ast::python::extract_classes(black_box(&file)).len());
        }

        #[divan::bench(args = PYTHON_TEST_FILES)]
        fn py_class_attributes(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            let file = ParsedFile::new(test_file.code, test_file.language);
            warm_shared_indexes(&file);
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .bench_local(|| ast::python::collect_class_attributes(black_box(&file)).len());
        }

        #[divan::bench(args = ALL_TEST_FILES)]
        fn literal_occurrences(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            let file = ParsedFile::new(test_file.code, test_file.language);
            warm_shared_indexes(&file);
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .bench_local(|| ast::collect_literal_occurrences(black_box(&file)).len());
        }

        #[divan::bench(args = ALL_TEST_FILES)]
        fn positional_reads(bencher: Bencher<'_, '_>, test_file: &TestFile) {
            let file = ParsedFile::new(test_file.code, test_file.language);
            warm_shared_indexes(&file);
            bencher
                .counter(BytesCount::of_str(test_file.code))
                .bench_local(|| ast::collect_positional_reads(black_box(&file)).len());
        }
    }
}

/// Named group of rules sharing an underlying AST/semantic extractor.
#[derive(Clone, Copy)]
struct RuleFamily {
    label: &'static str,
    language: Language,
    rules: &'static [&'static dyn AnyCodeRule],
}

impl fmt::Display for RuleFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let active_count = self
            .rules
            .iter()
            .filter(|rule| rule.supports_language(self.language))
            .count();
        write!(formatter, "{} ({active_count} rules)", self.label)
    }
}

const GROUP_A_CALL_RULES: &[&dyn AnyCodeRule] = &[
    &rules::unstructured_task::RULE,
    &rules::sleep_in_tests::SLEEP_IN_TESTS,
    &rules::sleep_in_tests::ZERO_SLEEP_IN_TESTS,
    &rules::mock_in_tests::RULE,
    &rules::mock_call_assertion::RULE,
    &rules::error_log_in_except::RULE,
    &rules::suppressed_exception::RULE,
    &rules::type_cast::RULE,
    &rules::dynamic_attribute_access::RULE,
    &rules::environment_variable_in_function::RULE,
];

const GROUP_B_BINDING_RULES: &[&dyn AnyCodeRule] = &[
    &rules::single_letter_name::RULE,
    &rules::abbreviated_name::RULE,
    &rules::primitive_duration::RULE,
    &rules::type_suffixed_name::RULE,
];

const GROUP_C_PY_SIGNATURE_RULES: &[&dyn AnyCodeRule] = &[
    &rules::identical_positional_types::RULE,
    &rules::concrete_collection_parameter::RULE,
    &rules::concrete_collection_return::RULE,
    &rules::mutable_collection_parameter::RULE,
    &rules::mutable_collection_return::RULE,
    &rules::specific_collection_parameter::RULE,
];

const GROUP_D_PY_CLASS_RULES: &[&dyn AnyCodeRule] = &[
    &rules::fake_without_protocol::RULE,
    &rules::mutable_dataclass::RULE,
    &rules::unslotted_dataclass::RULE,
    &rules::concrete_collection_attribute::RULE,
    &rules::mutable_collection_attribute::RULE,
    &rules::inline_public_attribute_annotation::RULE,
];

const GROUP_E_DEDICATED_RULES: &[&dyn AnyCodeRule] = &[
    &rules::too_many_assertions::RULE,
    &rules::packed_assertion::RULE,
    &rules::nested_function::RULE,
    &rules::bare_multiline_string::RULE,
    &rules::repeated_index_access::RULE,
    &rules::repeated_literal::RULE,
    &rules::mutable_module_constant::RULE,
    &rules::nullable_collection_return::RULE,
    &rules::unmatched_logger_placeholder::RULE,
    &rules::quote_wrapped_placeholder::RULE,
];

const PYTHON_RULE_FAMILIES: &[RuleFamily] = &[
    RuleFamily {
        label: "A_call_candidates_oncelock",
        language: Language::Python,
        rules: GROUP_A_CALL_RULES,
    },
    RuleFamily {
        label: "B_bindings_oncelock",
        language: Language::Python,
        rules: GROUP_B_BINDING_RULES,
    },
    RuleFamily {
        label: "C_py_signatures_unmemoized",
        language: Language::Python,
        rules: GROUP_C_PY_SIGNATURE_RULES,
    },
    RuleFamily {
        label: "D_py_classes_unmemoized",
        language: Language::Python,
        rules: GROUP_D_PY_CLASS_RULES,
    },
    RuleFamily {
        label: "E_dedicated_extractors",
        language: Language::Python,
        rules: GROUP_E_DEDICATED_RULES,
    },
];

const RUST_RULE_FAMILIES: &[RuleFamily] = &[
    RuleFamily {
        label: "A_call_candidates_oncelock",
        language: Language::Rust,
        rules: GROUP_A_CALL_RULES,
    },
    RuleFamily {
        label: "B_bindings_oncelock",
        language: Language::Rust,
        rules: GROUP_B_BINDING_RULES,
    },
    RuleFamily {
        label: "E_dedicated_extractors",
        language: Language::Rust,
        rules: GROUP_E_DEDICATED_RULES,
    },
];

/// Wrapper around `&'static dyn AnyCodeRule` implementing `Display` for `divan` table rows.
#[derive(Clone, Copy)]
struct BenchRule(&'static dyn AnyCodeRule);

impl fmt::Display for BenchRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.name())
    }
}

fn all_bench_rules_for_language(language: Language) -> Vec<BenchRule> {
    CODE_RULES
        .iter()
        .copied()
        .filter(|rule| rule.supports_language(language))
        .map(BenchRule)
        .collect()
}

/// Stage 3 (`linter`): End-to-end single-file linting plus marginal cost of rule families and
/// individual rules when `ParsedFile` and its `OnceLock` shared indexes are already warm.
#[divan::bench_group]
mod linter {
    use super::{
        ALL_TEST_FILES, BenchRule, Bencher, BytesCount, DEFAULT_CONFIG, Language, NO_RULES_CONFIG,
        PYTHON_RULE_FAMILIES, PYTHON_TEST_FILES, ParsedFile, Path, RUST_RULE_FAMILIES,
        RUST_TEST_FILES, RuleFamily, TestFile, all_bench_rules_for_language, black_box, lint_file,
        warm_shared_indexes,
    };

    #[divan::bench(args = ALL_TEST_FILES)]
    fn end_to_end_all_rules(bencher: Bencher<'_, '_>, test_file: &TestFile) {
        let path = Path::new(test_file.virtual_path);
        bencher
            .counter(BytesCount::of_str(test_file.code))
            .bench_local(|| lint_file(black_box(path), black_box(test_file.code), &DEFAULT_CONFIG));
    }

    #[divan::bench(args = ALL_TEST_FILES)]
    fn end_to_end_no_rules(bencher: Bencher<'_, '_>, test_file: &TestFile) {
        let path = Path::new(test_file.virtual_path);
        bencher
            .counter(BytesCount::of_str(test_file.code))
            .bench_local(|| {
                lint_file(black_box(path), black_box(test_file.code), &NO_RULES_CONFIG)
            });
    }

    #[divan::bench(args = PYTHON_RULE_FAMILIES)]
    fn family_python(bencher: Bencher<'_, '_>, family: &RuleFamily) {
        let prepared: Vec<(&'static str, ParsedFile)> = PYTHON_TEST_FILES
            .iter()
            .map(|test_file| {
                let file = ParsedFile::new(test_file.code, test_file.language);
                warm_shared_indexes(&file);
                (test_file.virtual_path, file)
            })
            .collect();
        let total_bytes: usize = prepared
            .iter()
            .map(|(_, file)| file.source_text().len())
            .sum();

        bencher
            .counter(BytesCount::new(total_bytes))
            .bench_local(|| {
                let mut total_diagnostics = 0_usize;
                for (virtual_path, file) in &prepared {
                    let path = Path::new(virtual_path);
                    for &rule in family.rules {
                        if rule.supports_language(Language::Python) {
                            total_diagnostics += rule
                                .check_file(black_box(path), black_box(file), None)
                                .len();
                        }
                    }
                }
                total_diagnostics
            });
    }

    #[divan::bench(args = RUST_RULE_FAMILIES)]
    fn family_rust(bencher: Bencher<'_, '_>, family: &RuleFamily) {
        let prepared: Vec<(&'static str, ParsedFile)> = RUST_TEST_FILES
            .iter()
            .map(|test_file| {
                let file = ParsedFile::new(test_file.code, test_file.language);
                warm_shared_indexes(&file);
                (test_file.virtual_path, file)
            })
            .collect();
        let total_bytes: usize = prepared
            .iter()
            .map(|(_, file)| file.source_text().len())
            .sum();

        bencher
            .counter(BytesCount::new(total_bytes))
            .bench_local(|| {
                let mut total_diagnostics = 0_usize;
                for (virtual_path, file) in &prepared {
                    let path = Path::new(virtual_path);
                    for &rule in family.rules {
                        if rule.supports_language(Language::Rust) {
                            total_diagnostics += rule
                                .check_file(black_box(path), black_box(file), None)
                                .len();
                        }
                    }
                }
                total_diagnostics
            });
    }

    #[divan::bench(args = all_bench_rules_for_language(Language::Python))]
    fn rule_python(bencher: Bencher<'_, '_>, bench_rule: BenchRule) {
        let prepared: Vec<(&'static str, ParsedFile)> = PYTHON_TEST_FILES
            .iter()
            .map(|test_file| {
                let file = ParsedFile::new(test_file.code, test_file.language);
                warm_shared_indexes(&file);
                (test_file.virtual_path, file)
            })
            .collect();
        let total_bytes: usize = prepared
            .iter()
            .map(|(_, file)| file.source_text().len())
            .sum();

        bencher
            .counter(BytesCount::new(total_bytes))
            .bench_local(|| {
                prepared
                    .iter()
                    .map(|(virtual_path, file)| {
                        bench_rule
                            .0
                            .check_file(black_box(Path::new(virtual_path)), black_box(file), None)
                            .len()
                    })
                    .sum::<usize>()
            });
    }

    #[divan::bench(args = all_bench_rules_for_language(Language::Rust))]
    fn rule_rust(bencher: Bencher<'_, '_>, bench_rule: BenchRule) {
        let prepared: Vec<(&'static str, ParsedFile)> = RUST_TEST_FILES
            .iter()
            .map(|test_file| {
                let file = ParsedFile::new(test_file.code, test_file.language);
                warm_shared_indexes(&file);
                (test_file.virtual_path, file)
            })
            .collect();
        let total_bytes: usize = prepared
            .iter()
            .map(|(_, file)| file.source_text().len())
            .sum();

        bencher
            .counter(BytesCount::new(total_bytes))
            .bench_local(|| {
                prepared
                    .iter()
                    .map(|(virtual_path, file)| {
                        bench_rule
                            .0
                            .check_file(black_box(Path::new(virtual_path)), black_box(file), None)
                            .len()
                    })
                    .sum::<usize>()
            });
    }
}

/// Shell command workload for `command_lint`.
#[derive(Clone, Copy)]
struct ShellWorkload {
    label: &'static str,
    command_line: &'static str,
}

impl fmt::Display for ShellWorkload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.label)
    }
}

const SHELL_WORKLOADS: &[ShellWorkload] = &[
    ShellWorkload {
        label: "simple_cargo_test",
        command_line: "cargo test --all-targets -- --nocapture",
    },
    ShellWorkload {
        label: "compound_pipeline",
        command_line: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test",
    },
    ShellWorkload {
        label: "env_wrapped_command",
        command_line: "RUSTDOCFLAGS=\"-D warnings\" env CARGO_TERM_COLOR=always cargo doc --no-deps",
    },
];

/// Stage 4 (`command_lint`): Command linter shell parsing (`InterceptedCommand::parse_all`) and
/// pipeline dispatch (`run_command_lint`).
#[divan::bench_group]
mod command_lint {
    use super::{
        Bencher, BytesCount, DEFAULT_CONFIG, InterceptedCommand, SHELL_WORKLOADS, ShellWorkload,
        black_box, run_command_lint,
    };

    #[divan::bench(args = SHELL_WORKLOADS)]
    fn parse_shell_commands(bencher: Bencher<'_, '_>, workload: &ShellWorkload) {
        bencher
            .counter(BytesCount::of_str(workload.command_line))
            .bench_local(|| InterceptedCommand::parse_all(black_box(workload.command_line)).len());
    }

    #[divan::bench(args = SHELL_WORKLOADS)]
    fn run_command_lint_pipeline(bencher: Bencher<'_, '_>, workload: &ShellWorkload) {
        bencher
            .counter(BytesCount::of_str(workload.command_line))
            .bench_local(|| {
                run_command_lint(black_box(workload.command_line), &DEFAULT_CONFIG).len()
            });
    }
}
