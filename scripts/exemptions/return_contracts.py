"""Exemption mutations for the return-contract batch (`docs/dev/return_contracts/03_design.md`)."""

from __future__ import annotations

from .model import ANNOTATIONS, AST, RETURNS, RS_RETURNS, Cluster, Mutation, Unmutated

UNEXPLAINED_RETURN = "src/code_lint/rules/unexplained_return.rs"

CLUSTER = Cluster(
    rules=(
        "positional_tuple_return",
        "non_boolean_predicate",
        "unexplained_return",
    ),
    mutations=(
        Mutation(
            label="variadic `tuple[T, ...]` is not a positional tuple (E1)",
            path=ANNOTATIONS,
            edits=(
                (
                    "(arguments.len() >= 2 && is_fixed_length).then_some(arguments.len())",
                    "(!arguments.is_empty()).then_some(arguments.len())",
                ),
            ),
            cases=(
                "positional_tuple_return::pass::variadic_or_single_element_tuple_exempt",
            ),
        ),
        Mutation(
            label="imposed Python signatures are not collected (E2)",
            path=RETURNS,
            edits=(
                (
                    ".filter(|signature| !is_exempt_from_signature_rules(signature))",
                    ".filter(|_| true)",
                ),
            ),
            cases=(
                "positional_tuple_return::pass::imposed_signature_exempt",
                "non_boolean_predicate::pass::imposed_signature_exempt",
            ),
        ),
        Mutation(
            label="Rust trait items and trait implementations are not collected (E3)",
            path=RS_RETURNS,
            edits=(
                (
                    "if is_trait_or_trait_impl || node.is_in_rust_inline_test() {",
                    "if node.is_in_rust_inline_test() {",
                ),
            ),
            cases=(
                "positional_tuple_return::pass::trait_contract_exempt",
                "non_boolean_predicate::pass::trait_contract_exempt",
            ),
        ),
        Mutation(
            label="Rust inline tests are not collected (E4)",
            path=RS_RETURNS,
            edits=(
                (
                    "if is_trait_or_trait_impl || node.is_in_rust_inline_test() {",
                    "if is_trait_or_trait_impl {",
                ),
            ),
            cases=(
                "positional_tuple_return::pass::inline_test_exempt",
                "non_boolean_predicate::pass::inline_test_exempt",
            ),
        ),
        Mutation(
            label="tuple displays in nested scopes do not count (E5)",
            path=RETURNS,
            edits=(
                (
                    "            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,\n",
                    "",
                ),
                ("            Expr::Lambda(_) => return,\n", ""),
            ),
            cases=("positional_tuple_return::pass::nested_scope_tuple_display_exempt",),
        ),
        Mutation(
            label="a starred tuple display has no fixed length (E5)",
            path=RETURNS,
            edits=(
                ("            && !tuple.elts.iter().any(Expr::is_starred_expr)\n", ""),
            ),
            cases=("positional_tuple_return::pass::starred_tuple_display_exempt",),
        ),
        Mutation(
            label="a predicate prefix must be followed by `_` (E7)",
            path=AST,
            edits=(
                (
                    ".and_then(|rest| rest.strip_prefix('_'))",
                    ".map(|rest| rest.trim_start_matches('_'))",
                ),
            ),
            cases=("non_boolean_predicate::pass::prefix_without_separator_exempt",),
        ),
        Mutation(
            label="`TypeGuard` / `TypeIs` are booleans (E8)",
            path=ANNOTATIONS,
            edits=(
                (
                    'const BOOLEAN_TYPES: &[&str] = &[BOOL_CONSTRUCTOR, "TypeGuard", "TypeIs"];',
                    "const BOOLEAN_TYPES: &[&str] = &[BOOL_CONSTRUCTOR];",
                ),
            ),
            cases=("non_boolean_predicate::pass::type_guard_exempt",),
        ),
        Mutation(
            label="Rust `Result<bool, E>` / `Poll<bool>` are booleans (E9)",
            path=RS_RETURNS,
            edits=(
                (
                    "let success = unwrap_rust_return_envelope(type_node, file);",
                    "let success = type_node;",
                ),
            ),
            cases=("non_boolean_predicate::pass::boolean_predicates@rust",),
        ),
        Mutation(
            label="aliases whose last segment ends in `Result` are results (E9)",
            path=RS_RETURNS,
            edits=(
                (
                    'let is_result_or_poll = base_terminal.ends_with("Result")',
                    'let is_result_or_poll = base_terminal == "Result"',
                ),
            ),
            cases=("non_boolean_predicate::pass::boolean_predicates@rust",),
        ),
        Mutation(
            label="an unannotated value return has an unknown type (E10)",
            path=RETURNS,
            edits=(
                (
                    "if results.returns_value || results.yields {\n                ReturnShape::Unknown",
                    "if false {\n                ReturnShape::Unknown",
                ),
            ),
            cases=("non_boolean_predicate::pass::unannotated_value_return_exempt",),
        ),
        Mutation(
            label="a stub body has an unknown result (E10)",
            path=RETURNS,
            edits=(("        if self.has_stub_body {", "        if false {"),),
            cases=("non_boolean_predicate::pass::stub_body_exempt",),
        ),
        Mutation(
            label="below both gate thresholds (unexplained-return E11)",
            path=UNEXPLAINED_RETURN,
            edits=(
                (
                    "                && signature.is_nontrivial(min_statements, min_cognitive_complexity)\n",
                    "",
                ),
            ),
            cases=("unexplained_return::pass::below_gate_exempt",),
        ),
        Mutation(
            label="undocumented functions are left to undocumented-nontrivial-function (unexplained-return E12)",
            path=UNEXPLAINED_RETURN,
            edits=(
                (
                    "signature.docstring().is_some_and(",
                    "signature.docstring().is_none_or(",
                ),
            ),
            cases=("unexplained_return::pass::undocumented_function_exempt",),
        ),
        Mutation(
            label="a Returns/Yields section or Return(s)/Yield(s) summary describes the result (unexplained-return E13)",
            path=UNEXPLAINED_RETURN,
            edits=(("!docstring.describes_result() && ", ""),),
            cases=(
                "unexplained_return::pass::returns_section_exempt",
                "unexplained_return::pass::result_verb_summary_exempt",
            ),
        ),
        Mutation(
            label="a predicate-named boolean is obvious (unexplained-return E14)",
            path=UNEXPLAINED_RETURN,
            edits=(("            && !ast::is_predicate_name(&signature.name)\n", ""),),
            cases=("unexplained_return::pass::predicate_name_boolean_exempt",),
        ),
        Mutation(
            label="a condition summary explains a boolean (unexplained-return E15)",
            path=UNEXPLAINED_RETURN,
            edits=(("            && !docstring.has_condition_summary())", ")"),),
            cases=("unexplained_return::pass::condition_summary_boolean_exempt",),
        ),
        Mutation(
            label="a single concrete type is obvious (unexplained-return E16)",
            path=UNEXPLAINED_RETURN,
            edits=(("|| union.branches.len() >= 2", "|| !union.branches.is_empty()"),),
            cases=("unexplained_return::pass::single_concrete_type_exempt",),
        ),
        Mutation(
            label="an unannotated function without a value return is obvious (unexplained-return E16)",
            path=UNEXPLAINED_RETURN,
            edits=(
                ("return results.returns_value || results.yields;", "return true;"),
            ),
            cases=("unexplained_return::pass::unannotated_without_value_exempt",),
        ),
        Mutation(
            label="nested functions are not checked (unexplained-return E17)",
            path=UNEXPLAINED_RETURN,
            edits=(
                (
                    "            !signature.is_nested()\n                && ",
                    "            ",
                ),
            ),
            cases=("unexplained_return::pass::nested_function_exempt",),
        ),
        Mutation(
            label="docstring-exempt methods are not checked (unexplained-return E17)",
            path=UNEXPLAINED_RETURN,
            edits=(
                (
                    "                && !signature.is_exempt_from_docstring_rules()\n",
                    "",
                ),
            ),
            cases=("unexplained_return::pass::docstring_exempt_method_exempt",),
        ),
        Mutation(
            label="property getters are not checked (unexplained-return E17)",
            path=UNEXPLAINED_RETURN,
            edits=(("                && !signature.is_property_getter()\n", ""),),
            cases=("unexplained_return::pass::property_getter_exempt",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts (return contracts)",
            reason="No exemption: named records instead of tuples, and predicates that return a boolean.",
            cases=(
                "positional_tuple_return::pass::named_record_returns",
                "non_boolean_predicate::pass::boolean_predicates@python",
            ),
        ),
        Unmutated(
            label="1-tuples and `()` are not positional tuples (Rust, E1)",
            reason=(
                "Guarded twice: `RustFunction::return_shape` only reports tuples of two or more elements, and"
                " `min-elements` rejects shorter ones, so no single edit flags them."
            ),
            cases=("positional_tuple_return::pass::short_tuple_exempt",),
        ),
        Unmutated(
            label="a tuple inside a collection is not a returned tuple (E6)",
            reason="Not visited: the return shape reads the tuple only through optionals, unions, results and pointers.",
            cases=("positional_tuple_return::pass::tuple_inside_collection_exempt",),
        ),
    ),
)
