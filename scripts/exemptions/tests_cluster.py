"""Exemption mutations for Batch 5: Tests (`packed-assertion`, `too-many-assertions`,
`sleep-in-tests`, `zero-sleep-in-tests`, `mock-in-tests`, `mock-call-assertion`,
`fake-without-protocol`)."""

from __future__ import annotations

from .model import (
    CALLS,
    CLASSES,
    DUPLICATED_TEST_SETUP,
    FAKE_WITHOUT_PROTOCOL,
    PYTHON,
    RUST,
    SLEEP_IN_TESTS,
    TOO_MANY_ASSERTIONS,
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
        "sleep_in_tests",
        "zero_sleep_in_tests",
        "mock_in_tests",
        "mock_call_assertion",
        "fake_without_protocol",
    ),
    mutations=(
        # --- packed-assertion ---
        Mutation(
            label="Python disjunctive `or` condition exempt from packed-assertion",
            path=PYTHON,
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
            path=RUST,
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
            path=PYTHON,
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
            path=RUST,
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
            path=PYTHON,
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
            path=RUST,
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
            path=PYTHON,
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
            path=PYTHON,
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
            path=RUST,
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
            path=RUST,
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
            path=PYTHON,
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
            path=RUST,
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
                    "        .filter(|(_, _, assertion_count)| *assertion_count > max_allowed)",
                    "        .filter(|(_, _, assertion_count)| *assertion_count >= max_allowed)",
                ),
            ),
            cases=(
                "too_many_assertions::pass::exact_threshold_of_four_allowed@python",
                "too_many_assertions::pass::exact_threshold_of_four_allowed@rust",
            ),
        ),
        Mutation(
            label="Python nested function assertions not counted toward enclosing test",
            path=PYTHON,
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
            path=PYTHON,
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
            path=PYTHON,
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
            path=RUST,
            edits=(
                (
                    "    if ast::Fn::can_cast(node.kind()) {\n        return 0;\n    }",
                    "    if false && ast::Fn::can_cast(node.kind()) {\n        return 0;\n    }",
                ),
            ),
            cases=("too_many_assertions::pass::nested_function_assertions_not_counted@rust",),
        ),
        # --- logic-in-tests ---
        Mutation(
            label="Python nested function exempt from logic-in-tests",
            path=PYTHON,
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
            path=PYTHON,
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
            path=PYTHON,
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
            path=PYTHON,
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
            path=RUST,
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
            path=RUST,
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
            path=PYTHON,
            edits=(
                (
                    "        if let Stmt::Assert(assert_statement) = statement\n"
                    "            && is_narrowing_guard_assert(assert_statement, body)\n"
                    "        {\n"
                    "            continue;\n"
                    "        }",
                    "        let _ = statement;",
                ),
            ),
            cases=("multi_phase_test::pass::narrowing_isinstance_between_assertions_exempt",),
        ),
        Mutation(
            label="Python consecutive assertions without intervening action allowed in multi-phase-test",
            path=PYTHON,
            edits=(
                (
                    "        if count_python_assertions_in_slice(std::slice::from_ref(statement), body) > 0 {\n"
                    "            if seen_action_after_assertion {",
                    "        if count_python_assertions_in_slice(std::slice::from_ref(statement), body) > 0 {\n"
                    "            let _ = seen_action_after_assertion;\n"
                    "            if seen_assertion {",
                ),
            ),
            cases=(
                "multi_phase_test::pass::single_phase_multiple_assertions_allowed@python",
                "multi_phase_test::pass::narrowing_isinstance_between_assertions_exempt",
            ),
        ),
        Mutation(
            label="Rust consecutive assertions without intervening action allowed in multi-phase-test",
            path=RUST,
            edits=(
                (
                    "        if count_rust_assertions(&statement_node) > 0 {\n"
                    "            if seen_action_after_assertion {",
                    "        if count_rust_assertions(&statement_node) > 0 {\n"
                    "            let _ = seen_action_after_assertion;\n"
                    "            if seen_assertion {",
                ),
            ),
            cases=(
                "multi_phase_test::pass::single_phase_multiple_assertions_allowed@rust",
                "multi_phase_test::pass::local_item_declaration_between_assertions_exempt",
            ),
        ),
        Mutation(
            label="Rust local item declaration exempt from multi-phase-test and duplicated-test-setup",
            path=RUST,
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
            path=PYTHON,
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
            path=RUST,
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
            path=PYTHON,
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
            cases=("duplicated_test_setup::pass::leading_docstring_not_counted_as_setup",),
        ),
        Mutation(
            label="Python nested function not counted as setup in duplicated-test-setup",
            path=PYTHON,
            edits=(
                (
                    "        .filter(|statement| !matches!(statement, Stmt::FunctionDef(_) | Stmt::ClassDef(_)))",
                    "        .filter(|statement| !matches!(statement, Stmt::ClassDef(_)))",
                ),
            ),
            cases=("duplicated_test_setup::pass::nested_function_not_counted_as_setup",),
        ),
        Mutation(
            label="Python nested class not counted as setup in duplicated-test-setup",
            path=PYTHON,
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
            path=PYTHON,
            edits=(
                (
                    "        .take_while(|statement| {\n"
                    "            count_python_assertions_in_slice(std::slice::from_ref(*statement), &func_def.body) == 0\n"
                    "        })",
                    "        .take_while(|_| true)",
                ),
            ),
            cases=("duplicated_test_setup::pass::assertions_not_counted_as_setup@python",),
        ),
        Mutation(
            label="Rust assertions not counted as setup in duplicated-test-setup",
            path=RUST,
            edits=(
                (
                    "        .take_while(|statement_node| count_rust_assertions(statement_node) == 0)",
                    "        .take_while(|_| true)",
                ),
            ),
            cases=("duplicated_test_setup::pass::assertions_not_counted_as_setup@rust",),
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
        # --- semantic/calls.rs (sleep-in-tests, mock-in-tests, mock-call-assertion) ---
        Mutation(
            label="method calls on unbound receivers not matched against bare literal callees",
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
                    "                && class\n"
                    "                    .bases\n"
                    "                    .iter()\n"
                    "                    .all(PythonBaseClass::is_structural_marker)",
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
                    "                    .all(PythonBaseClass::is_structural_marker)",
                    "                    .any(PythonBaseClass::is_structural_marker)",
                ),
            ),
            cases=("fake_without_protocol::pass::inherits_collaborator_with_generic_marker",),
        ),
        Mutation(
            label="non-Fake class without base exempt from fake-without-protocol",
            path=CLASSES,
            edits=(
                (
                    '    name.trim_start_matches(\'_\')\n        .strip_prefix("Fake")',
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
                    '    name.trim_start_matches(\'_\')\n        .strip_prefix("Fake")',
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
