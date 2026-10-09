"""Checks that each rule exemption has a dedicated test case that fails without the exemption.

Every entry disables one exemption in the source, runs the cases that exist to protect it, and
restores the file. A case that still passes tests nothing. The script also fails when a listed
case no longer exists, or when a `pass` case of a covered rule is not listed, so the mapping
cannot drift from the tests.

Usage (from the repository root): `python3 scripts/exemption_mutations.py [label substring]`.
Re-run it after any change to a covered rule or to the extractor behind it. Cases are named
`<rule>::<pass|fail>::<case>`, optionally suffixed with `@python` or `@rust` when the same case
name exists in both languages and the mutation only affects one of them.
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import sys
from collections.abc import Iterable, Mapping, Sequence
from typing import NamedTuple

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEST_LISTING_SUFFIX = ": test"
FAILED = "FAILED"
AST = "src/code_lint/ast.rs"
RUST = "src/code_lint/ast/rust.rs"
PYTHON = "src/code_lint/ast/python.rs"
CLASSES = "src/code_lint/ast/python/classes.rs"
CONSTRUCTOR_RULE = "src/code_lint/rules/constructor_after_method.rs"

FIELD = "field_after_method::pass::"
ASSOCIATED = "associated_item_after_method::pass::"
CONSTRUCTOR = "constructor_after_method::pass::"
UNCOLOCATED = "uncolocated_helper::pass::"
PRIVATE_FIRST = "private_before_public_function::pass::"
CALLEE = "callee_before_caller::pass::"
MAIN_GUARD = "statement_after_main_guard::"

COVERED_RULES = (
    "field_after_method",
    "associated_item_after_method",
    "constructor_after_method",
    "uncolocated_helper",
    "private_before_public_function",
    "callee_before_caller",
    "statement_after_main_guard",
)


class Mutation(NamedTuple):
    """Disables one exemption by applying `edits` (each `old` must occur exactly once)."""

    label: str
    path: str
    edits: tuple[tuple[str, str], ...]
    cases: tuple[str, ...]


class Unmutated(NamedTuple):
    """An exemption that no small source edit can disable, with the reason."""

    label: str
    reason: str
    cases: tuple[str, ...]


INLINE_TEST_ALWAYS_FALSE = (
    (
        "pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {",
        "pub fn is_in_rust_inline_test(&self, offset: usize) -> bool {\n"
        "        let _ = offset;\n        return false;",
    ),
)

MUTATIONS = (
    Mutation(
        "constructors grouped at the top",
        CONSTRUCTOR_RULE,
        (("            if seen_non_constructor {", "            if std::mem::replace(&mut seen_non_constructor, true) {"),),
        (CONSTRUCTOR + "constructors_before_methods", CONSTRUCTOR + "constructors_at_top_of_inherent_impl"),
    ),
    Mutation(
        "overloads and property accessors grouped",
        CLASSES,
        (("if is_property_accessor || continues_overload {", "if false {"),),
        (
            CONSTRUCTOR + "overloaded_init_grouped_at_first_declaration_exempt",
            PRIVATE_FIRST + "property_accessors_grouped_at_first_definition",
        ),
    ),
    Mutation(
        "private Rust new is not a constructor",
        RUST,
        (("let is_constructor = is_exported\n", "let is_constructor = true\n"),),
        (CONSTRUCTOR + "private_new_fn_exempt",),
    ),
    Mutation(
        "Rust new with self is not a constructor",
        RUST,
        (("&& !has_self\n", "\n"),),
        (CONSTRUCTOR + "self_receiver_new_method_exempt",),
    ),
    Mutation(
        "Rust new not returning Self is not a constructor",
        RUST,
        (("&& returns_self_type(&function, type_name);", ";"),),
        (CONSTRUCTOR + "new_fn_not_returning_self_exempt",),
    ),
    Mutation(
        "Rust inline test items skipped",
        AST,
        INLINE_TEST_ALWAYS_FALSE,
        (
            CONSTRUCTOR + "inline_test_before_constructor_exempt",
            UNCOLOCATED + "inline_test_fn_exempt",
            PRIVATE_FIRST + "inline_test_fn_exempt",
            ASSOCIATED + "inline_test_fn_exempt",
        ),
    ),
    Mutation(
        "helper right after its only caller",
        AST,
        (
            (
                "let is_just_after_owner = ((owner + 1)..pos).all(in_owner_cluster);",
                "let is_just_after_owner = false && ((owner + 1)..pos).all(in_owner_cluster);",
            ),
        ),
        (UNCOLOCATED + "helper_right_after_its_only_caller_allowed",),
    ),
    Mutation(
        "shared helper in the trailing section",
        AST,
        (("return pos > last_pub_idx;", "return false;"),),
        (UNCOLOCATED + "shared_helper_in_trailing_section_allowed",),
    ),
    Mutation(
        "single-caller helper in the trailing section",
        AST,
        (("let is_at_scope_end =\n        pos > last_pub_idx &&", "let is_at_scope_end =\n        false &&"),),
        (UNCOLOCATED + "all_private_helpers_at_end_of_scope",),
    ),
    Mutation(
        "second constructor keeps the trailing section valid",
        AST,
        (("all(|mid| roots_of[mid] != *roots)", "all(|mid| !in_owner_cluster(mid))"),),
        (UNCOLOCATED + "second_constructor_keeps_trailing_section_valid",),
    ),
    Mutation(
        "constructor helper after the constructor cluster",
        AST,
        (("|| (callables[owner].is_constructor", "|| (false"),),
        (UNCOLOCATED + "constructor_helper_after_constructor_cluster_allowed",),
    ),
    Mutation(
        "caller walk stops at public functions",
        AST,
        (
            (
                "pub_roots.push(caller);\n",
                "pub_roots.push(caller);\n                        stack.push(caller);\n",
            ),
        ),
        (UNCOLOCATED + "public_caller_of_public_function_does_not_share_its_helpers",),
    ),
    Mutation(
        "unreached private function left to private-before-public-function",
        AST,
        (
            (
                "&& pos < last_pub_idx\n            {\n                out.private_before_public.push(",
                "&& pos < last_pub_idx\n            {\n                out.uncolocated_helpers.push(",
            ),
        ),
        (UNCOLOCATED + "uncalled_private_function_exempt",),
    ),
    Mutation(
        "Python local bindings are not references",
        CLASSES,
        (("if !self.local_names.contains(ident) {", "if true {"),),
        (
            UNCOLOCATED + "class_method_local_binding_does_not_bridge_module_helper",
            CALLEE + "local_binding_shadowing_exempt@python",
        ),
    ),
    Mutation(
        "Rust local bindings are not references",
        RUST,
        (("if !local_names.contains(name)\n", "if true\n"),),
        (CALLEE + "local_binding_shadowing_exempt@rust",),
    ),
    Mutation(
        "helper below its only caller, before the next public function",
        AST,
        (("        if pos < max_root {", "        if last_pub.is_some_and(|last| pos < last) {"),),
        (
            PRIVATE_FIRST + "colocated_exclusive_helpers_between_public_entrypoints_allowed",
            PRIVATE_FIRST + "colocated_exclusive_helper_between_public_methods_allowed",
        ),
    ),
    Mutation(
        "Python dunder methods are public",
        CLASSES,
        (("if is_dunder || !name.starts_with('_')", "if !name.starts_with('_')"),),
        (PRIVATE_FIRST + "dunder_method_is_public",),
    ),
    Mutation(
        "Rust restricted visibility is public",
        RUST,
        (
            (
                "let is_exported = function.visibility().is_some();",
                'let is_exported = function.visibility().is_some_and(|v| v.syntax().text() == "pub");',
            ),
        ),
        (PRIVATE_FIRST + "restricted_visibility_is_public",),
    ),
    Mutation(
        "mutual recursion",
        AST,
        (("&& scc_id[caller] != scc_id[callee]", ""),),
        (CALLEE + "mutual_recursion_exempt",),
    ),
    Mutation(
        "functions flagged by an earlier stage",
        AST,
        (("if flagged_p1_or_p2[callee] || callables[callee]", "if callables[callee]"),),
        (CALLEE + "function_flagged_by_earlier_stage_not_reported_again",),
    ),
    Mutation(
        "calls involving a public function",
        AST,
        (
            (
                "if flagged_p1_or_p2[callee] || callables[callee].visibility == MethodVisibility::Public {",
                "if flagged_p1_or_p2[callee] {",
            ),
            ("&& callables[caller].visibility == MethodVisibility::Private", ""),
        ),
        (CALLEE + "public_callee_before_public_caller_exempt",),
    ),
    Mutation(
        "Rust trait impl blocks",
        RUST,
        (("&& impl_item.trait_().is_none()\n            && let Some(scope)", "&& let Some(scope)"),),
        (CALLEE + "trait_impl_exempt",),
    ),
    Mutation(
        "Rust self.field inside a macro",
        RUST,
        (("&& is_followed_by_open_paren(&token))", ")"),),
        (CALLEE + "macro_field_access_is_not_a_method_call",),
    ),
    Mutation(
        "reversed main guard comparison",
        PYTHON,
        (("        || (is_dunder_main_literal(left) && is_dunder_name_expr(right))", ""),),
        (MAIN_GUARD + "fail::reversed_main_guard_comparison",),
    ),
    Mutation(
        "non-guard comparisons on __name__",
        PYTHON,
        (("[ruff_python_ast::CmpOp::Eq]", "[_]"),),
        (MAIN_GUARD + "pass::non_main_name_comparisons_exempt",),
    ),
    Mutation(
        "subsequent main guards",
        PYTHON,
        ((".filter(|statement| !is_main_guard_statement(statement))", ""),),
        (MAIN_GUARD + "pass::subsequent_main_guard_exempt",),
    ),
)

UNMUTATED = (
    Unmutated(
        "positive layouts",
        "No exemption: the canonical order the rule asks for.",
        (
            FIELD + "fields_before_methods",
            ASSOCIATED + "associated_items_before_methods",
            PRIVATE_FIRST + "shared_helper_below_all_its_public_callers_allowed",
            CALLEE + "top_down_caller_before_callee_across_tiers",
            CALLEE + "top_down_impl_order",
            MAIN_GUARD + "pass::main_guard_at_end_of_module",
        ),
    ),
    Unmutated(
        "scope boundaries",
        "Each class body and each `impl` block is collected as its own scope.",
        (
            CONSTRUCTOR + "nested_class_tracks_constructors_independently",
            CONSTRUCTOR + "separate_impl_blocks_checked_independently",
            FIELD + "nested_class_checked_independently",
            ASSOCIATED + "separate_impl_blocks_checked_independently",
        ),
    ),
    Unmutated(
        "constructs the extractor never visits",
        "Only direct class-body `AnnAssign` with a name target, direct `impl`/`trait` "
        "`type`/`const` items, and module-level statements after the guard are scanned.",
        (
            FIELD + "unannotated_assignment_after_method_exempt",
            FIELD + "method_local_annotation_exempt",
            FIELD + "attribute_target_annotation_exempt",
            ASSOCIATED + "macro_call_after_fn_exempt",
            ASSOCIATED + "local_items_inside_fn_exempt",
            MAIN_GUARD + "pass::guard_else_branch_exempt",
            MAIN_GUARD + "pass::nested_main_guard_inside_function_exempt",
        ),
    ),
    Unmutated(
        "scope without public functions",
        "With no public function there is no boundary to order against.",
        (PRIVATE_FIRST + "private_only_scope_exempt",),
    ),
)


def main() -> int:
    label_filter = sys.argv[1] if len(sys.argv) > 1 else ""
    test_names = list_tests()
    problems = list(check_coverage(test_names))
    for mutation in MUTATIONS:
        if label_filter in mutation.label:
            problems += run_mutation(mutation, test_names)
    print("\nProblems:" if problems else "\nAll exemptions are protected.")
    for problem in problems:
        print(f"  {problem}")
    return 1 if problems else 0


def list_tests() -> tuple[str, ...]:
    listing = cargo_lib_tests("--list").stdout
    return tuple(line.removesuffix(TEST_LISTING_SUFFIX) for line in listing.splitlines() if line.endswith(TEST_LISTING_SUFFIX))


def check_coverage(test_names: Sequence[str]) -> tuple[str, ...]:
    """Reports listed cases that do not exist and `pass` cases of covered rules that are not listed."""
    problems = []
    listed = set()
    for entry in (*MUTATIONS, *UNMUTATED):
        for case in entry.cases:
            matches = resolve(case, test_names)
            if not matches:
                problems.append(f"{entry.label}: no test case `{case}`")
            listed.update(matches)
    for test_name in test_names:
        rule = re.search(r"rules::(\w+)::tests::pass::", test_name)
        if rule and rule.group(1) in COVERED_RULES and test_name not in listed:
            problems.append(f"unlisted pass case `{test_name}`: map it to an exemption above")
    return tuple(problems)


def resolve(case: str, test_names: Iterable[str]) -> tuple[str, ...]:
    """Returns the generated test names of `case`, whose `case_N_` prefix shifts as cases are added."""
    case, _, language = case.partition("@")
    rule, kind, name = case.split("::")
    pattern = re.compile(rf"rules::{rule}::tests::{kind}::case_\d+_{name}$")
    matches = tuple(sorted(test_name for test_name in test_names if pattern.search(test_name)))
    # `rule_test!` numbers Python cases before Rust cases.
    if language == "python":
        return matches[:1]
    if language == "rust":
        return matches[-1:]
    return matches


def run_mutation(mutation: Mutation, test_names: Sequence[str]) -> tuple[str, ...]:
    path = ROOT / mutation.path
    original = path.read_text()
    mutated = original
    for old, new in mutation.edits:
        if mutated.count(old) != 1:
            return (f"{mutation.label}: edit target found {mutated.count(old)} times in {mutation.path}: {old!r}",)
        mutated = mutated.replace(old, new, 1)
    cases = [test_name for case in mutation.cases for test_name in resolve(case, test_names)]
    path.write_text(mutated)
    try:
        outcomes = run_cases(cases)
    finally:
        path.write_text(original)
    problems = []
    for test_name in cases:
        status = outcomes.get(test_name, "not run")
        print(f"{'killed' if status == FAILED else 'SURVIVED':9} {mutation.label}: {test_name}")
        if status != FAILED:
            problems.append(f"{mutation.label}: `{test_name}` {status} without the exemption")
    return tuple(problems)


def run_cases(test_names: Sequence[str]) -> Mapping[str, str]:
    result = cargo_lib_tests("--exact", *test_names)
    if "test result:" not in result.stdout:
        return dict.fromkeys(test_names, "did not build")
    return dict(re.findall(rf"^test (\S+) \.\.\. (ok|{FAILED})$", result.stdout, flags=re.MULTILINE))


def cargo_lib_tests(*arguments: str) -> subprocess.CompletedProcess[str]:
    """Runs library tests with lints capped, since a mutation often leaves a helper unused.

    A separate target directory keeps the capped build from invalidating the normal one.
    """
    environment = {
        **os.environ,
        "RUSTFLAGS": "--cap-lints=warn",
        "CARGO_TARGET_DIR": str(ROOT / "target" / "exemption-mutations"),
    }
    return subprocess.run(
        ["cargo", "test", "--lib", "--", *arguments], cwd=ROOT, env=environment, capture_output=True, text=True
    )


if __name__ == "__main__":
    sys.exit(main())
