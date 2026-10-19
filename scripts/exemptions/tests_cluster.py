"""Exemption mutations for Batch 5: Tests (`packed-assertion`, `too-many-assertions`,
`logic-in-tests`, `multi-phase-test`, `duplicated-test-setup`, `test-calls-test`,
`sleep-in-tests`, `zero-sleep-in-tests`, `yield-loop-in-tests`, `mock-in-tests`,
`mock-call-assertion`, `fake-without-protocol`)."""

from __future__ import annotations

from .model import (
    CALLS,
    CLASSES,
    DUPLICATED_TEST_SETUP,
    FAKE_WITHOUT_PROTOCOL,
    PY_CALLS,
    PY_TESTS,
    RS_CALLS,
    RS_TESTS,
    SLEEP_IN_TESTS,
    TEST_ANALYSIS,
    TOO_MANY_ASSERTIONS,
    YIELD_LOOP_IN_TESTS,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "packed_assertion",
        "too_many_assertions",
        "logic_in_tests",
        "multi_phase_test",
        "duplicated_test_setup",
        "test_calls_test",
        "sleep_in_tests",
        "zero_sleep_in_tests",
        "yield_loop_in_tests",
        "mock_in_tests",
        "mock_call_assertion",
        "fake_without_protocol",
    ),
    mutations=(
        # --- packed-assertion ---
        Mutation(
            label="Python disjunctive `or` condition exempt from packed-assertion",
            path=PY_TESTS,
            edits=(
                (
                    "                    has_top_level_logical_and: matches!(\n"
                    "                        test,\n"
                    "                        Expr::BoolOp(bool_op) if bool_op.op == ruff_python_ast::BoolOp::And\n"
                    "                    ),",
                    "                    has_top_level_logical_and: matches!(test, Expr::BoolOp(_)),",
                ),
            ),
            cases=("packed_assertion::pass::disjunctive_or_condition@python",),
        ),
        Mutation(
            label="Rust disjunctive `||` condition exempt from packed-assertion",
            path=RS_TESTS,
            edits=(
                (
                    "                if first.kind() == SyntaxKind::AMP && second.kind() == SyntaxKind::AMP",
                    "                if first.kind() == SyntaxKind::PIPE && second.kind() == SyntaxKind::PIPE",
                ),
            ),
            cases=("packed_assertion::pass::disjunctive_or_condition@rust",),
        ),
        Mutation(
            label="Python `and` inside function call argument exempt from packed-assertion",
            path=PY_TESTS,
            edits=(
                (
                    "                    has_top_level_logical_and: matches!(\n"
                    "                        test,\n"
                    "                        Expr::BoolOp(bool_op) if bool_op.op == ruff_python_ast::BoolOp::And\n"
                    "                    ),",
                    '                    has_top_level_logical_and: format!("{test:?}").contains("And"),',
                ),
            ),
            cases=("packed_assertion::pass::logical_and_inside_function_call@python",),
        ),
        Mutation(
            label="Rust `&&` inside function call argument exempt from packed-assertion",
            path=RS_TESTS,
            edits=(
                (
                    "    if token_tree_has_direct_logical_and(&token_tree) {",
                    '    if token_tree.syntax().text().to_string().contains("&&") {',
                ),
            ),
            cases=("packed_assertion::pass::logical_and_inside_function_call@rust",),
        ),
        Mutation(
            label="Python single-element boolean tuple exempt from packed-assertion",
            path=PY_TESTS,
            edits=(
                (
                    "    elements.len() >= 2\n"
                    "        && elements\n"
                    "            .iter()\n"
                    "            .all(|element| matches!(element, Expr::BooleanLiteral(_)))",
                    "    !elements.is_empty()\n"
                    "        && elements\n"
                    "            .iter()\n"
                    "            .all(|element| matches!(element, Expr::BooleanLiteral(_)))",
                ),
            ),
            cases=("packed_assertion::pass::single_element_boolean_tuple@python",),
        ),
        Mutation(
            label="Rust single-element boolean tuple exempt from packed-assertion",
            path=RS_TESTS,
            edits=(
                (
                    "    items.len() >= 2\n        && items.iter().all(|item| {",
                    "    !items.is_empty()\n        && items.iter().all(|item| {",
                ),
            ),
            cases=("packed_assertion::pass::single_element_boolean_tuple@rust",),
        ),
        Mutation(
            label="Python non-boolean tuple/list equality exempt from packed-assertion",
            path=PY_TESTS,
            edits=(
                (
                    "    elements.len() >= 2\n"
                    "        && elements\n"
                    "            .iter()\n"
                    "            .all(|element| matches!(element, Expr::BooleanLiteral(_)))",
                    "    elements.len() >= 2",
                ),
            ),
            cases=(
                "packed_assertion::pass::non_boolean_tuple_equality@python",
                "packed_assertion::pass::non_boolean_list_equality",
            ),
        ),
        Mutation(
            label="Python single-value collection comparison exempt from packed-assertion",
            path=PY_TESTS,
            edits=(
                (
                    "                                .filter(|operand| is_multi_element_collection_expr(operand))\n"
                    "                                .count()\n"
                    "                                >= 2",
                    "                                .filter(|operand| is_multi_element_collection_expr(operand))\n"
                    "                                .count()\n"
                    "                                >= 1",
                ),
            ),
            cases=(
                "packed_assertion::pass::non_boolean_tuple_equality@python",
                "packed_assertion::pass::non_boolean_list_equality",
                "packed_assertion::pass::single_value_dict_equality",
            ),
        ),
        Mutation(
            label="Rust non-boolean tuple/array equality exempt from packed-assertion",
            path=RS_TESTS,
            edits=(
                (
                    "    items.len() >= 2\n"
                    "        && items.iter().all(|item| {\n"
                    "            matches!(\n"
                    "                item,\n"
                    "                SyntaxElement::Token(token)\n"
                    "                    if matches!(token.kind(), SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW)\n"
                    "            )\n"
                    "        })",
                    "    items.len() >= 2",
                ),
            ),
            cases=(
                "packed_assertion::pass::non_boolean_tuple_equality@rust",
                "packed_assertion::pass::non_boolean_array_equality",
                "packed_assertion::pass::call_with_collection_result",
            ),
        ),
        Mutation(
            label="Rust function-call argument list exempt from packed-assertion collection check",
            path=RS_TESTS,
            edits=(
                (
                    "    matches!(\n"
                    "        preceding.kind(),\n"
                    "        SyntaxKind::L_PAREN | SyntaxKind::L_BRACK | SyntaxKind::L_CURLY | SyntaxKind::COMMA\n"
                    "    )",
                    "    let _ = preceding;\n    true",
                ),
            ),
            cases=("packed_assertion::pass::call_with_collection_result",),
        ),
        # --- too-many-assertions, logic-in-tests & multi-phase-test ---
        Mutation(
            label="Python non-test helper function exempt from test-function rules",
            path=PY_TESTS,
            edits=(
                (
                    "            if let Stmt::FunctionDef(func_def) = statement {\n"
                    "                if is_test_function_def(func_def) {",
                    "            if let Stmt::FunctionDef(func_def) = statement {\n"
                    "                let _ = is_test_function_def(func_def);\n"
                    "                if true {",
                ),
            ),
            cases=(
                "too_many_assertions::pass::helper_function_exempt@python",
                "logic_in_tests::pass::helper_function_exempt@python",
                "multi_phase_test::pass::helper_function_exempt@python",
            ),
        ),
        Mutation(
            label="Rust non-test helper function exempt from test-function rules",
            path=RS_TESTS,
            edits=(
                (
                    "        if is_test_function(&function) {",
                    "        let _ = is_test_function(&function);\n        if true {",
                ),
            ),
            cases=(
                "too_many_assertions::pass::helper_function_exempt@rust",
                "logic_in_tests::pass::helper_function_exempt@rust",
                "multi_phase_test::pass::helper_function_exempt@rust",
            ),
        ),
        Mutation(
            label="assertion count at exact threshold allowed by too-many-assertions",
            path=TOO_MANY_ASSERTIONS,
            edits=(
                (
                    "        .filter(|test_fn| test_fn.count > max_allowed)",
                    "        .filter(|test_fn| test_fn.count >= max_allowed)",
                ),
            ),
            cases=(
                "too_many_assertions::pass::exact_threshold_of_four_allowed@python",
                "too_many_assertions::pass::exact_threshold_of_four_allowed@rust",
            ),
        ),
        Mutation(
            label="Python nested function assertions not counted toward enclosing test",
            path=PY_TESTS,
            edits=(
                (
                    "                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}",
                    "                Stmt::ClassDef(_) => {}",
                ),
            ),
            cases=(
                "too_many_assertions::pass::nested_function_assertions_not_counted@python",
                "multi_phase_test::pass::nested_function_assertions_not_counted",
            ),
        ),
        Mutation(
            label="Python nested class assertions not counted toward enclosing test",
            path=PY_TESTS,
            edits=(
                (
                    "                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}",
                    '                Stmt::FunctionDef(func) if func.name.as_str() != "verify" => {}',
                ),
            ),
            cases=("too_many_assertions::pass::nested_class_assertions_not_counted",),
        ),
        Mutation(
            label="Python type-narrowing guard assertions exempt from assertion counting",
            path=PY_TESTS,
            edits=(
                (
                    "    narrowed_variable_name(assert_statement)\n"
                    "        .is_some_and(|name| body_loads_name_after(body, name, assert_statement.range().end()))",
                    "    let _ = (assert_statement, body);\n    false",
                ),
            ),
            cases=(
                "too_many_assertions::pass::narrowing_is_not_none_guard_exempt",
                "too_many_assertions::pass::narrowing_isinstance_guard_exempt",
                "multi_phase_test::pass::narrowing_is_not_none_before_action_exempt",
            ),
        ),
        Mutation(
            label="Rust nested fn assertions not counted toward enclosing test",
            path=RS_TESTS,
            edits=(
                (
                    "    if ast::Fn::can_cast(node.kind()) {\n        return 0;\n    }",
                    "    if false && ast::Fn::can_cast(node.kind()) {\n        return 0;\n    }",
                ),
            ),
            cases=(
                "too_many_assertions::pass::nested_function_assertions_not_counted@rust",
            ),
        ),
        Mutation(
            label="Python value-returning setup helper with internal assert exempt from assertion-helper classification",
            path=PY_TESTS,
            edits=(
                (
                    "                || (is_unit_python_function(func_def)",
                    "                || ((is_unit_python_function(func_def) || true)",
                ),
            ),
            cases=(
                "too_many_assertions::pass::value_returning_setup_helper_with_assert_not_counted@python",
            ),
        ),
        Mutation(
            label="Rust value-returning setup helper with internal assert exempt from assertion-helper classification",
            path=RS_TESTS,
            edits=(
                (
                    "                && is_unit_rust_function(function)",
                    "                && (is_unit_rust_function(function) || true)",
                ),
            ),
            cases=(
                "too_many_assertions::pass::value_returning_setup_helper_with_assert_not_counted@rust",
            ),
        ),
        # --- logic-in-tests ---
        Mutation(
            label="Python nested function exempt from logic-in-tests",
            path=PY_TESTS,
            edits=(
                (
                    "                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,",
                    "                Stmt::ClassDef(_) => return,",
                ),
            ),
            cases=("logic_in_tests::pass::nested_function_exempt@python",),
        ),
        Mutation(
            label="Python nested class exempt from logic-in-tests",
            path=PY_TESTS,
            edits=(
                (
                    "                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,",
                    '                Stmt::FunctionDef(func) if func.name.as_str() != "get" => return,',
                ),
            ),
            cases=("logic_in_tests::pass::nested_class_exempt",),
        ),
        Mutation(
            label="Python lambda callback exempt from logic-in-tests",
            path=PY_TESTS,
            edits=(
                (
                    "                Expr::Lambda(_) => return,",
                    "                Expr::Lambda(_) => None,",
                ),
            ),
            cases=("logic_in_tests::pass::lambda_callback_exempt",),
        ),
        Mutation(
            label="Python unfiltered comprehensions exempt from logic-in-tests",
            path=PY_TESTS,
            edits=(
                (
                    "                Expr::ListComp(comp)\n"
                    "                    if comp\n"
                    "                        .generators\n"
                    "                        .iter()\n"
                    "                        .any(|generator| !generator.ifs.is_empty()) =>",
                    "                Expr::ListComp(_) =>",
                ),
            ),
            cases=("logic_in_tests::pass::unfiltered_comprehensions_allowed",),
        ),
        Mutation(
            label="Rust nested fn item exempt from logic-in-tests",
            path=RS_TESTS,
            edits=(
                (
                    "    if (ast::Item::can_cast(node.kind()) && !ast::MacroCall::can_cast(node.kind()))\n"
                    "        || ast::ClosureExpr::can_cast(node.kind())",
                    "    if ast::ClosureExpr::can_cast(node.kind())",
                ),
            ),
            cases=("logic_in_tests::pass::nested_function_exempt@rust",),
        ),
        Mutation(
            label="Rust closure callback exempt from logic-in-tests",
            path=RS_TESTS,
            edits=(
                (
                    "    if (ast::Item::can_cast(node.kind()) && !ast::MacroCall::can_cast(node.kind()))\n"
                    "        || ast::ClosureExpr::can_cast(node.kind())",
                    "    if ast::Item::can_cast(node.kind()) && !ast::MacroCall::can_cast(node.kind())",
                ),
            ),
            cases=("logic_in_tests::pass::closure_callback_exempt",),
        ),
        # --- multi-phase-test ---
        Mutation(
            label="Python narrowing guard between assertions not treated as action in multi-phase-test",
            path=PY_TESTS,
            edits=(
                (
                    "        if let Stmt::Assert(assert_statement) = statement\n"
                    "            && is_narrowing_guard_assert(assert_statement, body)\n"
                    "        {\n"
                    "            return TestStatementRole::InspectionOrGuard;\n"
                    "        }",
                    "        let _ = statement;",
                ),
            ),
            cases=(
                "multi_phase_test::pass::narrowing_isinstance_between_assertions_exempt",
            ),
        ),
        Mutation(
            label="consecutive assertions without intervening action allowed in multi-phase-test",
            path=TEST_ANALYSIS,
            edits=(
                (
                    "            TestStatementRole::Assertion => {\n"
                    "                if seen_action_after_assertion {",
                    "            TestStatementRole::Assertion => {\n"
                    "                let _ = seen_action_after_assertion;\n"
                    "                if seen_assertion {",
                ),
            ),
            cases=(
                "multi_phase_test::pass::single_phase_multiple_assertions_allowed@python",
                "multi_phase_test::pass::narrowing_isinstance_between_assertions_exempt",
                "multi_phase_test::pass::call_free_projection_binding_between_assertions_exempt@python",
                "multi_phase_test::pass::capsys_readouterr_after_pytest_raises_exempt",
                "multi_phase_test::pass::single_phase_multiple_assertions_allowed@rust",
                "multi_phase_test::pass::local_item_declaration_between_assertions_exempt",
                "multi_phase_test::pass::call_free_projection_binding_between_assertions_exempt@rust",
            ),
        ),
        Mutation(
            label="Python call-free projection binding and capsys.readouterr() between assertions exempt in multi-phase-test",
            path=PY_TESTS,
            edits=(
                (
                    "        } else if is_python_inspection_binding(statement) {\n"
                    "            TestStatementRole::InspectionOrGuard",
                    "        } else if is_python_inspection_binding(statement) && false {\n"
                    "            TestStatementRole::InspectionOrGuard",
                ),
            ),
            cases=(
                "multi_phase_test::pass::call_free_projection_binding_between_assertions_exempt@python",
                "multi_phase_test::pass::capsys_readouterr_after_pytest_raises_exempt",
            ),
        ),
        Mutation(
            label="Rust call-free projection binding between assertions exempt in multi-phase-test",
            path=RS_TESTS,
            edits=(
                (
                    "            } else if is_rust_inspection_binding(&statement_node) {\n"
                    "                TestStatementRole::InspectionOrGuard",
                    "            } else if is_rust_inspection_binding(&statement_node) && false {\n"
                    "                TestStatementRole::InspectionOrGuard",
                ),
            ),
            cases=(
                "multi_phase_test::pass::call_free_projection_binding_between_assertions_exempt@rust",
            ),
        ),
        Mutation(
            label="Rust local item declaration exempt from multi-phase-test and duplicated-test-setup",
            path=RS_TESTS,
            edits=(
                (
                    "        .filter(|statement| {\n"
                    "            !matches!(\n"
                    "                statement,\n"
                    "                ast::Stmt::Item(item) if !matches!(item, ast::Item::MacroCall(_))\n"
                    "            )\n"
                    "        })",
                    "        .filter(|_| true)",
                ),
            ),
            cases=(
                "multi_phase_test::pass::local_item_declaration_between_assertions_exempt",
                "duplicated_test_setup::pass::local_item_declarations_not_counted_as_setup",
            ),
        ),
        # --- duplicated-test-setup ---
        Mutation(
            label="test count below min-tests threshold allowed by duplicated-test-setup",
            path=DUPLICATED_TEST_SETUP,
            edits=(
                (
                    "            if group.len() < effective_min_tests {",
                    "            if group.len() + 1 < effective_min_tests {",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::below_min_tests_threshold_allowed@python",
                "duplicated_test_setup::pass::below_min_tests_threshold_allowed@rust",
            ),
        ),
        Mutation(
            label="statement count below min-statements threshold allowed by duplicated-test-setup",
            path=DUPLICATED_TEST_SETUP,
            edits=(
                (
                    "    let effective_min_statements = min_statements.max(1);",
                    "    let effective_min_statements = min_statements.saturating_sub(1).max(1);",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::below_min_statements_threshold_allowed@python",
                "duplicated_test_setup::pass::below_min_statements_threshold_allowed@rust",
            ),
        ),
        Mutation(
            label="Python non-test helper function exempt from duplicated-test-setup",
            path=PY_TESTS,
            edits=(
                (
                    "        if !is_test_function_def(func_def) {\n            continue;\n        }",
                    "        let _ = is_test_function_def(func_def);",
                ),
            ),
            cases=("duplicated_test_setup::pass::helper_function_exempt@python",),
        ),
        Mutation(
            label="Rust non-test helper function exempt from duplicated-test-setup",
            path=RS_TESTS,
            edits=(
                (
                    "        if !is_test_function(function) {\n            continue;\n        }",
                    "        let _ = is_test_function(function);",
                ),
            ),
            cases=("duplicated_test_setup::pass::helper_function_exempt@rust",),
        ),
        Mutation(
            label="Python leading docstring not counted as setup in duplicated-test-setup",
            path=PY_TESTS,
            edits=(
                (
                    "    let statements = if docstrings::docstring_of(&func_def.body, file).is_some() {\n"
                    "        &func_def.body[1..]\n"
                    "    } else {\n"
                    "        &func_def.body[..]\n"
                    "    };",
                    "    let statements = &func_def.body[..];",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::leading_docstring_not_counted_as_setup",
            ),
        ),
        Mutation(
            label="Python nested function not counted as setup in duplicated-test-setup",
            path=PY_TESTS,
            edits=(
                (
                    "        .filter(|statement| !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)))",
                    "        .filter(|statement| !matches!(statement, Stmt::ClassDef(_)))",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::nested_function_not_counted_as_setup",
            ),
        ),
        Mutation(
            label="Python nested class not counted as setup in duplicated-test-setup",
            path=PY_TESTS,
            edits=(
                (
                    "        .filter(|statement| !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)))",
                    "        .filter(|statement| !matches!(statement, Stmt::FunctionDef(_)))",
                ),
            ),
            cases=("duplicated_test_setup::pass::nested_class_not_counted_as_setup",),
        ),
        Mutation(
            label="Python assertions not counted as setup in duplicated-test-setup",
            path=PY_TESTS,
            edits=(
                (
                    "        .take_while(|statement| {\n"
                    "            count_python_assertions_in_slice(\n"
                    "                std::slice::from_ref(*statement),\n"
                    "                &func_def.body,\n"
                    "                assertion_helpers,\n"
                    "            ) == 0\n"
                    "        })",
                    "        .take_while(|_| true)",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::assertions_not_counted_as_setup@python",
            ),
        ),
        Mutation(
            label="Rust assertions not counted as setup in duplicated-test-setup",
            path=RS_TESTS,
            edits=(
                (
                    "        .take_while(|statement_node| count_rust_assertions(statement_node, assertion_helpers) == 0)",
                    "        .take_while(|_| true)",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::assertions_not_counted_as_setup@rust",
            ),
        ),
        Mutation(
            label="separate test classes or modules do not pool across scopes in duplicated-test-setup",
            path=DUPLICATED_TEST_SETUP,
            edits=(
                (
                    "    for scope in ast::collect_test_setup_scopes(file) {",
                    "    for scope in [ast::collect_test_setup_scopes(file).into_iter().flatten().collect::<Vec<_>>()] {",
                ),
            ),
            cases=(
                "duplicated_test_setup::pass::separate_test_classes_do_not_pool_across_scopes",
                "duplicated_test_setup::pass::separate_test_modules_do_not_pool_across_scopes",
            ),
        ),
        # --- sleep-in-tests & zero-sleep-in-tests ---
        Mutation(
            label="zero-duration sleep excluded from sleep-in-tests",
            path=SLEEP_IN_TESTS,
            edits=(
                (
                    "    rule.check_banned_calls_where(path, file, banned, |call_match| {\n"
                    "        !has_zero_duration_argument(call_match)\n"
                    "    })",
                    "    rule.check_banned_calls_where(path, file, banned, |_| true)",
                ),
            ),
            cases=(
                "sleep_in_tests::pass::zero_duration_sleep_handled_separately@python",
                "sleep_in_tests::pass::zero_duration_sleep_handled_separately@rust",
            ),
        ),
        Mutation(
            label="non-zero sleep excluded from zero-sleep-in-tests",
            path=SLEEP_IN_TESTS,
            edits=(
                (
                    "    rule.check_banned_calls_where(path, file, banned, has_zero_duration_argument)",
                    "    rule.check_banned_calls_where(path, file, banned, |_| true)",
                ),
            ),
            cases=(
                "zero_sleep_in_tests::pass::non_zero_sleep@python",
                "zero_sleep_in_tests::pass::non_zero_sleep@rust",
            ),
        ),
        Mutation(
            label="multi-argument sleep call excluded from zero-sleep-in-tests",
            path=SLEEP_IN_TESTS,
            edits=(
                (
                    "    let [argument] = call_match.arguments.as_slice() else {\n"
                    "        return false;\n"
                    "    };",
                    "    let Some(argument) = call_match.arguments.first() else {\n"
                    "        return false;\n"
                    "    };",
                ),
            ),
            cases=("zero_sleep_in_tests::pass::multi_arg_sleep",),
        ),
        # --- yield-loop-in-tests ---
        Mutation(
            label="yield outside a loop excluded from yield-loop-in-tests",
            path=YIELD_LOOP_IN_TESTS,
            edits=(
                (
                    "    rule.check_banned_calls_where(path, file, banned, |call_match| call_match.is_in_loop)",
                    "    rule.check_banned_calls_where(path, file, banned, |_| true)",
                ),
            ),
            cases=(
                "yield_loop_in_tests::pass::single_checkpoint_outside_loop",
                "yield_loop_in_tests::pass::single_yield_now_outside_loop",
            ),
        ),
        Mutation(
            label="Python for-loop iterator expression exempt from yield-loop-in-tests",
            path=PY_CALLS,
            edits=(
                (
                    "                    self.visit_expr(&for_stmt.iter);\n"
                    "                    self.in_loop = true;",
                    "                    self.in_loop = true;\n"
                    "                    self.visit_expr(&for_stmt.iter);",
                ),
            ),
            cases=("yield_loop_in_tests::pass::for_loop_iter_expression_exempt",),
        ),
        Mutation(
            label="Python for-loop else clause exempt from yield-loop-in-tests",
            path=PY_CALLS,
            edits=(
                (
                    "                    self.visit_body(&for_stmt.body);\n"
                    "                    self.in_loop = enclosing_loop;\n"
                    "                    self.visit_body(&for_stmt.orelse);",
                    "                    self.visit_body(&for_stmt.body);\n"
                    "                    self.visit_body(&for_stmt.orelse);",
                ),
            ),
            cases=("yield_loop_in_tests::pass::for_loop_else_clause_exempt",),
        ),
        Mutation(
            label="Python while-loop else clause exempt from yield-loop-in-tests",
            path=PY_CALLS,
            edits=(
                (
                    "                    self.visit_body(&while_stmt.body);\n"
                    "                    self.in_loop = enclosing_loop;\n"
                    "                    self.visit_body(&while_stmt.orelse);",
                    "                    self.visit_body(&while_stmt.body);\n"
                    "                    self.visit_body(&while_stmt.orelse);",
                ),
            ),
            cases=("yield_loop_in_tests::pass::while_loop_else_clause_exempt",),
        ),
        Mutation(
            label="Python nested function inside loop exempt from yield-loop-in-tests",
            path=PY_CALLS,
            edits=(
                (
                    "            if matches!(statement, Stmt::FunctionDef(_)) {\n"
                    "                self.in_loop = false;\n"
                    "            }",
                    "",
                ),
            ),
            cases=(
                "yield_loop_in_tests::pass::nested_function_inside_loop_exempt@python",
            ),
        ),
        Mutation(
            label="Python lambda inside loop exempt from yield-loop-in-tests",
            path=PY_CALLS,
            edits=(
                (
                    "                Expr::Lambda(_) => {\n"
                    "                    self.in_except_clause = false;\n"
                    "                    self.in_loop = false;\n"
                    "                }",
                    "                Expr::Lambda(_) => {\n"
                    "                    self.in_except_clause = false;\n"
                    "                }",
                ),
            ),
            cases=("yield_loop_in_tests::pass::lambda_inside_loop_exempt",),
        ),
        Mutation(
            label="Rust for-loop iterable expression exempt from yield-loop-in-tests",
            path=RS_CALLS,
            edits=(
                (
                    "        if let Some(for_expr) = ast::ForExpr::cast(ancestor)\n"
                    "            && for_expr\n"
                    "                .loop_body()\n"
                    "                .is_some_and(|body| body.syntax().text_range().contains_range(range))\n"
                    "        {",
                    "        if ast::ForExpr::can_cast(ancestor.kind()) {",
                ),
            ),
            cases=("yield_loop_in_tests::pass::for_loop_iterable_expression_exempt",),
        ),
        Mutation(
            label="Rust nested function inside loop exempt from yield-loop-in-tests",
            path=RS_CALLS,
            edits=(
                (
                    "        if ast::Fn::can_cast(ancestor.kind()) || ast::ClosureExpr::can_cast(ancestor.kind()) {",
                    "        if ast::ClosureExpr::can_cast(ancestor.kind()) {",
                ),
            ),
            cases=(
                "yield_loop_in_tests::pass::nested_function_inside_loop_exempt@rust",
            ),
        ),
        Mutation(
            label="Rust closure inside loop exempt from yield-loop-in-tests",
            path=RS_CALLS,
            edits=(
                (
                    "        if ast::Fn::can_cast(ancestor.kind()) || ast::ClosureExpr::can_cast(ancestor.kind()) {",
                    "        if ast::Fn::can_cast(ancestor.kind()) {",
                ),
            ),
            cases=("yield_loop_in_tests::pass::closure_inside_loop_exempt",),
        ),
        # --- test-calls-test ---
        Mutation(
            label="Python non-test helper function exempt from test-calls-test",
            path=PY_TESTS,
            edits=(
                (
                    "            Stmt::FunctionDef(func_def) if is_test_function_def(func_def) => {",
                    "            Stmt::FunctionDef(func_def) => {",
                ),
            ),
            cases=("test_calls_test::pass::shared_non_test_helper",),
        ),
        Mutation(
            label="Python module-level call outside function exempt from test-calls-test",
            path=PY_TESTS,
            edits=(
                (
                    "        let Some(caller_fn) = candidate.enclosing_functions.first() else {\n"
                    "            continue;\n"
                    "        };",
                    "        let default_fn = EnclosingFunction {\n"
                    "            name: String::new(),\n"
                    "            name_span: SourceSpan::new(0, 0),\n"
                    "            is_top_level: true,\n"
                    "            enclosing_class: None,\n"
                    "        };\n"
                    "        let caller_fn = candidate.enclosing_functions.first().unwrap_or(&default_fn);",
                ),
            ),
            cases=("test_calls_test::pass::module_level_call_outside_function_exempt",),
        ),
        Mutation(
            label="Python method call on collaborator exempt from test-calls-test",
            path=PY_TESTS,
            edits=(
                (
                    "            .map(|_| PythonTestTarget::Class { class_name, method })\n"
                    "    } else {\n"
                    "        None\n"
                    "    }",
                    "            .map(|_| PythonTestTarget::Class { class_name, method })\n"
                    "    } else {\n"
                    "        candidate\n"
                    "            .method_name\n"
                    "            .as_deref()\n"
                    "            .filter(|method| module_tests.contains(method))\n"
                    "            .map(PythonTestTarget::Module)\n"
                    "    }",
                ),
            ),
            cases=("test_calls_test::pass::method_named_test_on_collaborator_exempt",),
        ),
        Mutation(
            label="Python non-Test* class methods exempt from test-calls-test",
            path=PY_TESTS,
            edits=(
                (
                    "            Stmt::ClassDef(class_def) if is_test_class_def(class_def) => {",
                    "            Stmt::ClassDef(class_def) => {",
                ),
            ),
            cases=("test_calls_test::pass::method_on_non_test_class_exempt",),
        ),
        Mutation(
            label="Python local binding shadowing test name exempt from test-calls-test",
            path=PY_TESTS,
            edits=(
                (
                    "        (module_tests.contains(callee) && !is_local).then_some(PythonTestTarget::Module(callee))",
                    "        module_tests.contains(callee).then_some(PythonTestTarget::Module(callee))",
                ),
            ),
            cases=("test_calls_test::pass::parameter_named_test_callable_exempt",),
        ),
        Mutation(
            label="Python recursive self-call exempt from test-calls-test",
            path=PY_TESTS,
            edits=(
                (
                    "        if is_recursive_python_test_call(&candidate, caller_fn, &target) {\n"
                    "            continue;\n"
                    "        }",
                    "        if false {\n            continue;\n        }",
                ),
            ),
            cases=("test_calls_test::pass::recursive_self_call_exempt@python",),
        ),
        Mutation(
            label="Rust helper without #[test] attribute exempt from test-calls-test",
            path=RS_TESTS,
            edits=(
                (
                    "        if has_test_attribute(&fn_decl) {",
                    "        if true {",
                ),
            ),
            cases=(
                "test_calls_test::pass::shared_helper_without_test_attribute",
                "test_calls_test::pass::helper_named_test_prefix_without_test_attribute_exempt",
            ),
        ),
        Mutation(
            label="Rust method call on struct exempt from test-calls-test",
            path=RS_TESTS,
            edits=(
                (
                    "    if candidate.method_name.is_some() {\n        return None;\n    }",
                    "    if let Some(method) = candidate.method_name.as_deref() {\n"
                    "        return Some(LocalRustCallee {\n"
                    "            super_count: 0,\n"
                    "            name: method,\n"
                    "        });\n"
                    "    }",
                ),
            ),
            cases=("test_calls_test::pass::method_named_test_on_struct_exempt",),
        ),
        Mutation(
            label="Rust associated function on type exempt from test-calls-test",
            path=RS_TESTS,
            edits=(
                (
                    "            .all(|segment| matches!(*segment, SELF_KEYWORD | SUPER_KEYWORD))",
                    "            .all(|segment| !segment.is_empty())",
                ),
            ),
            cases=("test_calls_test::pass::associated_function_on_type_exempt",),
        ),
        Mutation(
            label="Rust local binding shadowing test name exempt from test-calls-test",
            path=RS_TESTS,
            edits=(
                (
                    "        if !is_test_target || is_local {",
                    "        if !is_test_target {",
                ),
            ),
            cases=("test_calls_test::pass::parameter_callable_exempt",),
        ),
        Mutation(
            label="Rust recursive self-call exempt from test-calls-test",
            path=RS_TESTS,
            edits=(
                (
                    "        if super_count == 0 && callee_name == caller_fn.name {\n"
                    "            continue;\n"
                    "        }",
                    "        if false {\n            continue;\n        }",
                ),
            ),
            cases=("test_calls_test::pass::recursive_self_call_exempt@rust",),
        ),
        Mutation(
            label="Rust helper in sibling module not matched against #[test] in another module",
            path=RS_TESTS,
            edits=(
                (
                    "            let module_span = enclosing_rust_module_span(fn_decl.syntax());",
                    "            let module_span = SourceSpan::new(0, 0);",
                ),
                (
                    "        let Some(target_module) = target_rust_module_span(&candidate.node, super_count) else {",
                    "        let Some(target_module) = Some(SourceSpan::new(0, 0)) else {",
                ),
            ),
            cases=("test_calls_test::pass::helper_in_sibling_module_exempt",),
        ),
        # --- semantic/calls.rs (sleep-in-tests, mock-in-tests, mock-call-assertion) ---
        Mutation(
            label="method calls on unbound receivers not matched against bare literal callees",
            path=CALLS,
            edits=(
                (
                    "                ResolvedName::Unbound => literal_callees.contains(candidate.callee.as_str()),",
                    "                ResolvedName::Unbound => {\n"
                    "                    literal_callees.contains(candidate.callee.as_str())\n"
                    "                        || candidate.method_name.as_deref().is_some_and(|method| {\n"
                    "                            literal_callees.iter().any(|entry| {\n"
                    "                                *entry == method\n"
                    '                                    || entry.ends_with(&format!(".{method}"))\n'
                    '                                    || entry.ends_with(&format!("::{method}"))\n'
                    "                            })\n"
                    "                        })\n"
                    "                }",
                ),
            ),
            cases=(
                "sleep_in_tests::pass::injected_fake_clock_sleep",
                "sleep_in_tests::pass::custom_receiver_method",
                "sleep_in_tests::pass::injected_fake_clock_method",
                "mock_in_tests::pass::http_client_patch_method",
            ),
        ),
        Mutation(
            label="callee imported from unrelated module exempt from find_banned_calls",
            path=CALLS,
            edits=(
                (
                    "                ResolvedName::Imported(path) => literal_callees.contains(path.as_str()),",
                    "                ResolvedName::Imported(_) => literal_callees.contains(candidate.callee.as_str()),",
                ),
            ),
            cases=("mock_in_tests::pass::unrelated_imported_patch_exempt",),
        ),
        Mutation(
            label="locally defined function exempt from find_banned_calls",
            path=CALLS,
            edits=(
                (
                    "                ResolvedName::Local => false,",
                    "                ResolvedName::Local => literal_callees.contains(candidate.callee.as_str()),",
                ),
            ),
            cases=("mock_in_tests::pass::locally_defined_patch_helper_exempt",),
        ),
        Mutation(
            label="wildcard method pattern matches exact method name only",
            path=CALLS,
            edits=(
                (
                    "                method_callees.contains(method)",
                    '                method_callees.contains(method) || method.starts_with("assert_")',
                ),
            ),
            cases=("mock_call_assertion::pass::unrelated_method_call",),
        ),
        # --- fake-without-protocol ---
        Mutation(
            label="collaborator base class exempts Fake* class from fake-without-protocol",
            path=FAKE_WITHOUT_PROTOCOL,
            edits=(
                (
                    "                && decl.bases.iter().all(PythonBaseClass::is_structural_marker)",
                    "                && true",
                ),
            ),
            cases=(
                "fake_without_protocol::pass::inherits_domain_protocol",
                "fake_without_protocol::pass::inherits_qualified_base",
                "fake_without_protocol::pass::inherits_generic_protocol",
                "fake_without_protocol::pass::inherits_pep695_generic_protocol",
            ),
        ),
        Mutation(
            label="Fake* class inheriting both collaborator and structural marker exempt",
            path=FAKE_WITHOUT_PROTOCOL,
            edits=(
                (
                    ".all(PythonBaseClass::is_structural_marker)",
                    ".any(PythonBaseClass::is_structural_marker)",
                ),
            ),
            cases=(
                "fake_without_protocol::pass::inherits_collaborator_with_generic_marker",
            ),
        ),
        Mutation(
            label="non-Fake class without base exempt from fake-without-protocol",
            path=CLASSES,
            edits=(
                (
                    "    name.trim_start_matches('_')\n        .strip_prefix(\"Fake\")",
                    "    Some(name.trim_start_matches('_'))",
                ),
            ),
            cases=("fake_without_protocol::pass::non_fake_class_without_base",),
        ),
        Mutation(
            label="word starting with Fake followed by lowercase letter exempt from fake-without-protocol",
            path=CLASSES,
            edits=(
                (
                    "        .is_some_and(|rest| !rest.starts_with(|character: char| character.is_ascii_lowercase()))",
                    "        .is_some_and(|_| true)",
                ),
            ),
            cases=("fake_without_protocol::pass::word_starting_with_fake_exempt",),
        ),
        Mutation(
            label="lowercase fake_ prefix exempt from fake-without-protocol",
            path=CLASSES,
            edits=(
                (
                    "    name.trim_start_matches('_')\n        .strip_prefix(\"Fake\")",
                    "    name.trim_start_matches('_')\n"
                    "        .to_ascii_lowercase()\n"
                    '        .strip_prefix("fake")\n'
                    "        .map(str::to_owned)\n"
                    "        .as_deref()",
                ),
            ),
            cases=("fake_without_protocol::pass::lowercase_fake_prefix_exempt",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts for Batch 5 test rules",
            reason=(
                "No exemption: atomic single-condition assertions, `matches!` macro assertion, "
                "dedicated async yield primitives (`anyio.lowlevel.checkpoint`, "
                "`tokio::task::yield_now`), and state-based fakes without mock/sleep calls."
            ),
            cases=(
                "packed_assertion::pass::atomic_assertion@python",
                "packed_assertion::pass::atomic_assertion@rust",
                "logic_in_tests::pass::matches_macro_allowed",
                "zero_sleep_in_tests::pass::anyio_checkpoint",
                "zero_sleep_in_tests::pass::tokio_yield_now",
                "mock_in_tests::pass::state_based_fake_repository",
                "mock_call_assertion::pass::state_assertion_on_fake",
                "mock_call_assertion::pass::return_value_assertion",
            ),
        ),
    ),
)
