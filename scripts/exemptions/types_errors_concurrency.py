"""Exemption mutations for Batch 7: Types, Records, Errors & Concurrency
(`dynamic-attribute-access`, `type-cast`, `mutable-dataclass`, `unslotted-dataclass`,
`suppressed-exception`, `error-log-in-except`, `unstructured-task`)."""

from __future__ import annotations

from .model import (
    AST,
    CALLS,
    CLASSES,
    COMMENTS,
    ERROR_LOG_IN_EXCEPT,
    PYTHON,
    SUPPRESSED_EXCEPTION,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "dynamic_attribute_access",
        "type_cast",
        "mutable_dataclass",
        "unslotted_dataclass",
        "suppressed_exception",
        "error_log_in_except",
        "unstructured_task",
    ),
    mutations=(
        # --- dynamic-attribute-access, type-cast, unstructured-task (semantic/calls.rs) ---
        Mutation(
            label="method calls on unbound receivers not matched against bare literal callees in Batch 7 rules",
            path=CALLS,
            edits=(
                (
                    "                ResolvedName::Unbound => literal_callees.contains(candidate.callee.as_str()),",
                    "                ResolvedName::Unbound => {\n"
                    "                    literal_callees.contains(candidate.callee.as_str())\n"
                    "                        || candidate\n"
                    "                            .method_name\n"
                    "                            .as_deref()\n"
                    "                            .is_some_and(|method| literal_callees.contains(method))\n"
                    "                }",
                ),
            ),
            cases=(
                "dynamic_attribute_access::pass::custom_receiver_getattr",
                "dynamic_attribute_access::pass::custom_receiver_setattr",
                "dynamic_attribute_access::pass::custom_receiver_hasattr",
                "dynamic_attribute_access::pass::custom_receiver_delattr",
                "type_cast::pass::polars_column_cast",
                "type_cast::pass::custom_method_cast",
                "unstructured_task::pass::task_group_allowed",
            ),
        ),
        Mutation(
            label="callee imported from unrelated module exempt from type-cast",
            path=CALLS,
            edits=(
                (
                    "                ResolvedName::Imported(path) => literal_callees.contains(path.as_str()),",
                    "                ResolvedName::Imported(_) => literal_callees.contains(candidate.callee.as_str()),",
                ),
            ),
            cases=("type_cast::pass::unrelated_imported_cast",),
        ),
        Mutation(
            label="locally defined cast function exempt from type-cast",
            path=CALLS,
            edits=(
                (
                    "                ResolvedName::Local => false,",
                    "                ResolvedName::Local => literal_callees.contains(candidate.callee.as_str()),",
                ),
            ),
            cases=("type_cast::pass::locally_defined_cast",),
        ),
        Mutation(
            label="logger instance `.error(...)` not matched against module-qualified `logging.error`",
            path=CALLS,
            edits=(
                (
                    '        } else if let Some(method_name) = trimmed.strip_prefix("*.") {',
                    "        } else if let Some((_, method_name)) = trimmed.split_once('.') {",
                ),
            ),
            cases=("error_log_in_except::pass::logger_instance_error_not_in_banned_calls",),
        ),
        # --- mutable-dataclass & unslotted-dataclass ---
        Mutation(
            label="dataclass passing required keyword argument exempt from mutable-dataclass and unslotted-dataclass",
            path=CLASSES,
            edits=(
                (
                    "        self.dataclass_decorator()\n"
                    "            .is_some_and(|decorator| !decorator.has_arg(key))",
                    "        let _ = key;\n        self.dataclass_decorator().is_some()",
                ),
            ),
            cases=(
                "mutable_dataclass::pass::unqualified_frozen_allowed",
                "mutable_dataclass::pass::qualified_frozen_allowed",
                "unslotted_dataclass::pass::unqualified_slots_allowed",
                "unslotted_dataclass::pass::qualified_slots_allowed",
            ),
        ),
        Mutation(
            label="explicit `frozen=False` and `slots=False` opt-out allowed by mutable-dataclass and unslotted-dataclass",
            path=PYTHON,
            edits=(
                (
                    "                .filter_map(|kw| Some(kw.arg.as_ref()?.id.to_string()))",
                    "                .filter(|kw| !matches!(&kw.value, Expr::BooleanLiteral(b) if !b.value))\n"
                    "                .filter_map(|kw| Some(kw.arg.as_ref()?.id.to_string()))",
                ),
            ),
            cases=(
                "mutable_dataclass::pass::explicit_mutable_opt_out_allowed",
                "unslotted_dataclass::pass::explicit_no_slots_opt_out_allowed",
            ),
        ),
        Mutation(
            label="undecorated class exempt from mutable-dataclass and unslotted-dataclass",
            path=CLASSES,
            edits=(
                (
                    "        self.dataclass_decorator()\n"
                    "            .is_some_and(|decorator| !decorator.has_arg(key))",
                    "        self.decorators.is_empty()\n"
                    "            || self.dataclass_decorator().is_some_and(|decorator| !decorator.has_arg(key))",
                ),
            ),
            cases=(
                "mutable_dataclass::pass::undecorated_class_exempt",
                "unslotted_dataclass::pass::undecorated_class_exempt",
            ),
        ),
        Mutation(
            label="class with non-dataclass decorator exempt from mutable-dataclass and unslotted-dataclass",
            path=CLASSES,
            edits=(
                (
                    "        self.decorators.iter().find(|decorator| {\n"
                    "            matches!(\n"
                    "                decorator.path.as_str(),\n"
                    "                DATACLASS_DECORATOR | QUALIFIED_DATACLASS_DECORATOR\n"
                    "            )\n"
                    "        })",
                    "        self.decorators.first()",
                ),
            ),
            cases=(
                "mutable_dataclass::pass::other_decorator_class_exempt",
                "unslotted_dataclass::pass::other_decorator_class_exempt",
            ),
        ),
        # --- suppressed-exception ---
        Mutation(
            label="inline explanation comment on `with` line licenses suppressed-exception",
            path=COMMENTS,
            edits=(
                (
                    "    fn has_inline_explanation(&self, line: usize) -> bool {\n"
                    "        self.comment_on_line(line).is_some_and(|text| {\n"
                    "            let cleaned = clean_explanation(text.as_ref());\n"
                    "            is_substantive_explanation(cleaned)\n"
                    "        })\n"
                    "    }",
                    "    fn has_inline_explanation(&self, line: usize) -> bool {\n"
                    "        let _ = line;\n"
                    "        false\n"
                    "    }",
                ),
            ),
            cases=(
                "suppressed_exception::pass::single_line_inline",
                "suppressed_exception::pass::multiline_parenthesized_with_inline",
                "suppressed_exception::pass::multiline_header_trailing_comment",
            ),
        ),
        Mutation(
            label="preceding standalone comment block licenses suppressed-exception",
            path=COMMENTS,
            edits=(
                (
                    "        while curr_line > 0 {",
                    "        while false && curr_line > 0 {",
                ),
            ),
            cases=(
                "suppressed_exception::pass::preceding_comment_block",
                "suppressed_exception::pass::multiline_parenthesized_with_preceding",
            ),
        ),
        Mutation(
            label="preceding comment above multiline `with (` header licenses suppressed-exception",
            path=COMMENTS,
            edits=(
                (
                    "        self.has_adjacent_explanation(*header_lines.start())",
                    "        false",
                ),
            ),
            cases=("suppressed_exception::pass::multiline_parenthesized_with_preceding",),
        ),
        Mutation(
            label="trailing comment on closing `):` of multiline `with` header licenses suppressed-exception",
            path=COMMENTS,
            edits=(
                (
                    "            || header_lines\n"
                    "                .into_iter()\n"
                    "                .any(|header_line| self.has_inline_explanation(header_line))",
                    "",
                ),
            ),
            cases=("suppressed_exception::pass::multiline_header_trailing_comment",),
        ),
        Mutation(
            label="suppress() call outside `with` context manager exempt from suppressed-exception",
            path=SUPPRESSED_EXCEPTION,
            edits=(
                (
                    "    rule.check_banned_calls_where(path, file, banned_calls, |matched| {\n"
                    "        matched.is_with_context_manager\n"
                    "    })",
                    "    rule.check_banned_calls_where(path, file, banned_calls, |_| true)",
                ),
            ),
            cases=("suppressed_exception::pass::suppress_call_outside_with_ignored",),
        ),
        # --- error-log-in-except ---
        Mutation(
            label="logging.error outside `except` handler exempt from error-log-in-except",
            path=ERROR_LOG_IN_EXCEPT,
            edits=(
                (
                    "    rule.check_banned_calls_where(path, file, banned, |matched| matched.is_in_except_clause)",
                    "    rule.check_banned_calls_where(path, file, banned, |_| true)",
                ),
            ),
            cases=(
                "error_log_in_except::pass::logging_error_outside_except",
                "error_log_in_except::pass::logging_error_in_else_block",
                "error_log_in_except::pass::logging_error_in_finally_block",
            ),
        ),
        Mutation(
            label="`else` and `finally` clauses of `try` statement excluded from `in_except_clause`",
            path=AST,
            edits=(
                (
                    "            if matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {\n"
                    "                self.in_except_clause = false;\n"
                    "            }",
                    "            if matches!(statement, Stmt::Try(_)) {\n"
                    "                self.in_except_clause = true;\n"
                    "            }",
                ),
            ),
            cases=(
                "error_log_in_except::pass::logging_error_in_else_block",
                "error_log_in_except::pass::logging_error_in_finally_block",
            ),
        ),
        Mutation(
            label="nested function definition inside `except` resets `in_except_clause`",
            path=AST,
            edits=(
                (
                    "            if matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {\n"
                    "                self.in_except_clause = false;\n"
                    "            }",
                    "",
                ),
            ),
            cases=("error_log_in_except::pass::logging_error_in_nested_function_inside_except",),
        ),
        Mutation(
            label="lambda expression inside `except` resets `in_except_clause`",
            path=AST,
            edits=(("                Expr::Lambda(_) => self.in_except_clause = false,\n", ""),),
            cases=("error_log_in_except::pass::logging_error_in_lambda_inside_except",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts for Batch 7 rules",
            reason=(
                "No exemption: direct attribute access, dict `.get()`, `isinstance` narrowing, "
                "`logging.exception` inside `except`, `anyio.create_task_group()` / `tg.start_soon`, "
                "and unrelated method names (`create_task_record`)."
            ),
            cases=(
                "dynamic_attribute_access::pass::direct_attribute_access",
                "dynamic_attribute_access::pass::dict_lookup",
                "type_cast::pass::isinstance_narrowing",
                "error_log_in_except::pass::logging_exception_in_except",
                "unstructured_task::pass::anyio_task_group_allowed",
                "unstructured_task::pass::unrelated_method_allowed",
            ),
        ),
    ),
)
