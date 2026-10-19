"""Exemption mutations for the `Topic::DECLARATION_ORDER` rules."""

from __future__ import annotations

from .model import (
    AST,
    CALL_CLUSTERS,
    INLINE_TEST_ALWAYS_FALSE,
    PY_DECL_ORDER,
    RS_DECL_ORDER,
    SCOPE_GRAPH,
    Cluster,
    Mutation,
    Unmutated,
)

FIELD = "field_after_method::pass::"
ASSOCIATED = "associated_item_after_method::pass::"
CONSTRUCTOR = "constructor_after_method::pass::"
UNCOLOCATED = "uncolocated_helper::pass::"
PRIVATE_FIRST = "private_before_public::pass::"
CALLEE = "callee_before_caller::pass::"
MAIN_GUARD = "statement_after_main_guard::"

MUTATIONS = (
    Mutation(
        "constructors grouped at the top",
        CALL_CLUSTERS,
        (
            (
                "if let Some(first_non_constructor) = items.iter().position(|item| !item.is_constructor) {",
                "if let Some(first_non_constructor) = items.iter().position(|_| true) {",
            ),
        ),
        (
            CONSTRUCTOR + "constructors_before_methods",
            CONSTRUCTOR + "constructors_at_top_of_inherent_impl",
        ),
    ),
    Mutation(
        "overloads and property accessors grouped",
        PY_DECL_ORDER,
        (("if is_property_accessor || continues_overload {", "if false {"),),
        (
            CONSTRUCTOR + "overloaded_init_grouped_at_first_declaration_exempt",
            PRIVATE_FIRST + "property_accessors_grouped_at_first_definition",
        ),
    ),
    Mutation(
        "private Rust new is not a constructor",
        RS_DECL_ORDER,
        (("let is_constructor = is_exported\n", "let is_constructor = true\n"),),
        (CONSTRUCTOR + "private_new_fn_exempt",),
    ),
    Mutation(
        "Rust new with self is not a constructor",
        RS_DECL_ORDER,
        (("&& !has_self\n", "\n"),),
        (CONSTRUCTOR + "self_receiver_new_method_exempt",),
    ),
    Mutation(
        "Rust new not returning Self is not a constructor",
        RS_DECL_ORDER,
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
        CALL_CLUSTERS,
        (("(unit.is_some() && self.in_cluster[subject]) ||", "false ||"),),
        (UNCOLOCATED + "helper_right_after_its_only_caller_allowed",),
    ),
    Mutation(
        "single-caller helper in the trailing section",
        CALL_CLUSTERS,
        (
            (
                "|| self.is_trailing(subject)\n",
                "|| (unit.is_none() && self.is_trailing(subject))\n",
            ),
        ),
        (UNCOLOCATED + "all_private_helpers_at_end_of_scope",),
    ),
    Mutation(
        "cluster below the last public function is not split from the trailing section",
        CALL_CLUSTERS,
        (("self.in_cluster[subject]\n                    || !(", "!("),),
        (UNCOLOCATED + "cluster_of_last_public_function_before_shared_helper_allowed",),
    ),
    Mutation(
        "constructors form one entrypoint unit",
        SCOPE_GRAPH,
        (("first_constructor.unwrap_or(root)", "root"),),
        (UNCOLOCATED + "constructor_helper_after_constructor_cluster_allowed",),
    ),
    Mutation(
        "caller walk stops at public functions",
        SCOPE_GRAPH,
        (
            (
                "roots.push(caller);\n",
                "roots.push(caller);\n                        stack.push(caller);\n",
            ),
        ),
        (UNCOLOCATED + "public_caller_of_public_function_does_not_share_its_helpers",),
    ),
    Mutation(
        "unreached private function left to private-before-public",
        CALL_CLUSTERS,
        (
            (
                "Constraint::Below(last_public),\n                CallClusterRule::PrivateBeforePublic,",
                "Constraint::Below(last_public),\n                CallClusterRule::UncolocatedHelper,",
            ),
        ),
        (UNCOLOCATED + "uncalled_private_function_exempt",),
    ),
    Mutation(
        "Python local bindings are not references",
        PY_DECL_ORDER,
        (("if !self.local_names.contains(ident) {", "if true {"),),
        (
            UNCOLOCATED + "class_method_local_binding_does_not_bridge_module_helper",
            CALLEE + "local_binding_shadowing_exempt@python",
        ),
    ),
    Mutation(
        "Rust local bindings are not references",
        RS_DECL_ORDER,
        (("if !local_names.contains(name)\n", "if true\n"),),
        (CALLEE + "local_binding_shadowing_exempt@rust",),
    ),
    Mutation(
        "helper below its only caller, before the next public function",
        CALL_CLUSTERS,
        (
            (
                "Constraint::Below(root),",
                "Constraint::Below(last_public.unwrap_or(root)),",
            ),
        ),
        (
            PRIVATE_FIRST
            + "colocated_exclusive_helpers_between_public_entrypoints_allowed",
            PRIVATE_FIRST + "colocated_exclusive_helper_between_public_methods_allowed",
        ),
    ),
    Mutation(
        "Python dunder methods are public",
        PY_DECL_ORDER,
        (("if is_dunder || !name.starts_with('_')", "if !name.starts_with('_')"),),
        (PRIVATE_FIRST + "dunder_method_is_public",),
    ),
    Mutation(
        "Rust restricted visibility is public",
        RS_DECL_ORDER,
        (
            (
                "let is_exported = function.visibility().is_some();",
                'let is_exported = function.visibility().is_some_and(|v| v.syntax().text() == "pub");',
            ),
        ),
        (PRIVATE_FIRST + "restricted_visibility_is_public",),
    ),
    Mutation(
        "Python runtime evaluation pinning",
        CALL_CLUSTERS,
        (
            (
                "if let Some(pin) = items[subject].pinned_before {",
                "if let Some(pin) = None::<usize> {",
            ),
        ),
        (
            PRIVATE_FIRST + "import_time_pinned_helper_exempt",
            PRIVATE_FIRST + "decorator_pinned_helper_exempt",
            PRIVATE_FIRST + "base_class_pinned_before_subclass_exempt",
            PRIVATE_FIRST + "runtime_annotation_pinned_without_future_annotations",
        ),
    ),
    Mutation(
        "mutual recursion",
        CALL_CLUSTERS,
        (
            (
                "            && graph.private_recursion_group[edge.higher]\n"
                "                != graph.private_recursion_group[edge.lower]\n",
                "",
            ),
        ),
        (CALLEE + "mutual_recursion_exempt",),
    ),
    Mutation(
        "one finding per item",
        CALL_CLUSTERS,
        (
            (
                ".filter_map(|subject_violations| {\n            subject_violations.iter().find_map(",
                ".flat_map(|subject_violations| {\n            subject_violations.iter().filter_map(",
            ),
        ),
        (CALLEE + "function_flagged_by_earlier_stage_not_reported_again",),
    ),
    Mutation(
        "calls involving a public function",
        CALL_CLUSTERS,
        (
            (
                "if is_private(edge.higher)\n            && is_private(edge.lower)\n            &&",
                "if",
            ),
        ),
        (CALLEE + "public_callee_before_public_caller_exempt",),
    ),
    Mutation(
        "Rust trait impl blocks",
        RS_DECL_ORDER,
        (
            (
                "&& impl_item.trait_().is_none()\n            && let Some(scope)",
                "&& let Some(scope)",
            ),
        ),
        (CALLEE + "trait_impl_exempt",),
    ),
    Mutation(
        "Rust self.field inside a macro",
        RS_DECL_ORDER,
        (("&& is_followed_by_open_paren(&token))", ")"),),
        (CALLEE + "macro_field_access_is_not_a_method_call",),
    ),
    Mutation(
        "signature types are not encapsulated callees",
        SCOPE_GRAPH,
        (("is_type(candidate) && !signature[candidate]", "is_type(candidate)"),),
        (CALLEE + "signature_closure_type_not_encapsulated_exempt",),
    ),
    Mutation(
        "reversed main guard comparison",
        PY_DECL_ORDER,
        (
            (
                "        || (is_dunder_main_literal(left) && is_dunder_name_expr(right))",
                "",
            ),
        ),
        (MAIN_GUARD + "fail::reversed_main_guard_comparison",),
    ),
    Mutation(
        "non-guard comparisons on __name__",
        PY_DECL_ORDER,
        (("[ruff_python_ast::CmpOp::Eq]", "[_]"),),
        (MAIN_GUARD + "pass::non_main_name_comparisons_exempt",),
    ),
    Mutation(
        "subsequent main guards",
        PY_DECL_ORDER,
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
            PRIVATE_FIRST + "private_helper_after_pub_struct_impl_allowed",
            UNCOLOCATED + "shared_helper_in_trailing_section_allowed",
            UNCOLOCATED + "second_constructor_keeps_trailing_section_valid",
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

CLUSTER = Cluster(
    rules=(
        "field_after_method",
        "associated_item_after_method",
        "constructor_after_method",
        "uncolocated_helper",
        "private_before_public",
        "callee_before_caller",
        "statement_after_main_guard",
    ),
    mutations=MUTATIONS,
    unmutated=UNMUTATED,
)
