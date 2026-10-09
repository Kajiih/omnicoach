"""Exemption mutations for Batch 6: Positional Data, Scopes & Globals
(`repeated-index-access`, `identical-positional-types`, `environment-variable-in-function`,
`nested-function`)."""

from __future__ import annotations

from .model import (
    ENVIRONMENT_VARIABLE_IN_FUNCTION,
    FUNCTIONS,
    IDENTICAL_POSITIONAL_TYPES,
    POSITIONAL_READS,
    PYTHON,
    REPEATED_INDEX_ACCESS,
    RUST,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "repeated_index_access",
        "identical_positional_types",
        "environment_variable_in_function",
        "nested_function",
    ),
    mutations=(
        # --- nested-function ---
        Mutation(
            label="top-level functions exempt from nested-function",
            path=FUNCTIONS,
            edits=(
                (
                    "    let mut visitor = NestedFunctionVisitor {\n"
                    "        file,\n"
                    "        in_function: false,\n"
                    "        out: Vec::new(),\n"
                    "    };",
                    "    let mut visitor = NestedFunctionVisitor {\n"
                    "        file,\n"
                    "        in_function: true,\n"
                    "        out: Vec::new(),\n"
                    "    };",
                ),
            ),
            cases=("nested_function::pass::top_level_functions_allowed",),
        ),
        Mutation(
            label="class methods and methods of local classes exempt from nested-function",
            path=FUNCTIONS,
            edits=(
                (
                    "                Stmt::ClassDef(_) => {\n"
                    "                    let prev = self.in_function;\n"
                    "                    self.in_function = false;",
                    "                Stmt::ClassDef(_) => {\n"
                    "                    let prev = self.in_function;\n"
                    "                    self.in_function = true;",
                ),
            ),
            cases=(
                "nested_function::pass::class_methods_allowed",
                "nested_function::pass::method_of_local_class_not_flagged",
            ),
        ),
        # --- environment-variable-in-function ---
        Mutation(
            label="module, class, and static scope exempt from environment-variable-in-function",
            path=ENVIRONMENT_VARIABLE_IN_FUNCTION,
            edits=(
                (
                    "    enclosing_functions\n"
                    "        .first()\n"
                    "        .map(|function| function.name.as_str())",
                    "    enclosing_functions\n"
                    "        .first()\n"
                    "        .map(|function| function.name.as_str())\n"
                    '        .or(Some("<module>"))',
                ),
            ),
            cases=(
                "environment_variable_in_function::pass::module_scope_call_allowed",
                "environment_variable_in_function::pass::module_scope_subscript_allowed",
                "environment_variable_in_function::pass::class_scope_call_allowed",
                "environment_variable_in_function::pass::static_lazy_lock_allowed",
            ),
        ),
        Mutation(
            label="from_env boundary function exempt from environment-variable-in-function",
            path=ENVIRONMENT_VARIABLE_IN_FUNCTION,
            edits=(
                (
                    '        "from_env" | "from_environ" | "load_env" => true,',
                    '        "from_environ" | "load_env" => true,',
                ),
            ),
            cases=(
                "environment_variable_in_function::pass::from_env_boundary_allowed@python",
                "environment_variable_in_function::pass::from_env_boundary_allowed@rust",
            ),
        ),
        Mutation(
            label="from_environ boundary function exempt from environment-variable-in-function",
            path=ENVIRONMENT_VARIABLE_IN_FUNCTION,
            edits=(
                (
                    '        "from_env" | "from_environ" | "load_env" => true,',
                    '        "from_env" | "load_env" => true,',
                ),
            ),
            cases=(
                "environment_variable_in_function::pass::from_environ_boundary_allowed@python",
                "environment_variable_in_function::pass::from_environ_boundary_allowed@rust",
            ),
        ),
        Mutation(
            label="load_env boundary function exempt from environment-variable-in-function",
            path=ENVIRONMENT_VARIABLE_IN_FUNCTION,
            edits=(
                (
                    '        "from_env" | "from_environ" | "load_env" => true,',
                    '        "from_env" | "from_environ" => true,',
                ),
            ),
            cases=(
                "environment_variable_in_function::pass::load_env_boundary_allowed@python",
                "environment_variable_in_function::pass::load_env_boundary_allowed@rust",
            ),
        ),
        Mutation(
            label="top-level main function exempt from environment-variable-in-function",
            path=ENVIRONMENT_VARIABLE_IN_FUNCTION,
            edits=(
                (
                    '        "main" => is_top_level,',
                    '        "main" => false && is_top_level,',
                ),
            ),
            cases=(
                "environment_variable_in_function::pass::main_entrypoint_allowed",
                "environment_variable_in_function::pass::top_level_main_allowed",
                "environment_variable_in_function::pass::lambda_in_main_exempt",
                "environment_variable_in_function::pass::closure_in_main_exempt",
            ),
        ),
        Mutation(
            label="functions nested inside boundary function inherit exemption in environment-variable-in-function",
            path=ENVIRONMENT_VARIABLE_IN_FUNCTION,
            edits=(
                (
                    "    if enclosing_functions\n"
                    "        .iter()\n"
                    "        .any(|function| is_exempt_boundary_function(&function.name, function.is_top_level))",
                    "    if enclosing_functions\n"
                    "        .first()\n"
                    "        .is_some_and(|function| is_exempt_boundary_function(&function.name, function.is_top_level))",
                ),
            ),
            cases=(
                "environment_variable_in_function::pass::nested_fn_in_main_exempt@python",
                "environment_variable_in_function::pass::nested_fn_in_main_exempt@rust",
            ),
        ),
        Mutation(
            label="Python subscript on non-environ name exempt from environment-variable-in-function",
            path=PYTHON,
            edits=(
                (
                    "        Expr::Name(name) => name.id.as_str() == ENVIRON_NAME,",
                    "        Expr::Name(_) => true,",
                ),
            ),
            cases=("environment_variable_in_function::pass::dict_subscript_allowed",),
        ),
        Mutation(
            label="Python `.environ` subscript on non-`os` receiver exempt from environment-variable-in-function",
            path=PYTHON,
            edits=(
                (
                    '                && matches!(attr.value.as_ref(), Expr::Name(obj) if obj.id.as_str() == "os")',
                    "",
                ),
            ),
            cases=("environment_variable_in_function::pass::non_os_environ_subscript_allowed",),
        ),
        # --- identical-positional-types ---
        Mutation(
            label="functions with fewer than min-positional-parameters exempt from identical-positional-types",
            path=IDENTICAL_POSITIONAL_TYPES,
            edits=(
                (
                    "    if positional_params.len() < min_args {\n        return None;\n    }",
                    "    let _ = min_args;",
                ),
            ),
            cases=("identical_positional_types::pass::fewer_than_min_args",),
        ),
        Mutation(
            label="keyword-only parameters excluded from identical-positional-types",
            path=FUNCTIONS,
            edits=(
                (
                    "        result.push(build_parameter_with_default(\n"
                    "            param_with_default,\n"
                    "            PythonParameterKind::KeywordOnly,\n"
                    "            file,\n"
                    "        ));",
                    "        result.push(build_parameter_with_default(\n"
                    "            param_with_default,\n"
                    "            PythonParameterKind::Positional,\n"
                    "            file,\n"
                    "        ));",
                ),
            ),
            cases=(
                "identical_positional_types::pass::keyword_only_separator",
                "identical_positional_types::pass::after_varargs_is_keyword_only",
            ),
        ),
        Mutation(
            label="leading `self` receiver excluded from positional parameter count",
            path=FUNCTIONS,
            edits=(
                (
                    '        let kind = if is_first_param && matches!(name, SELF_PARAMETER | "cls") {',
                    '        let kind = if is_first_param && name == "cls" {',
                ),
            ),
            cases=("identical_positional_types::pass::method_self_excluded_below_min",),
        ),
        Mutation(
            label="leading `cls` receiver excluded from positional parameter count",
            path=FUNCTIONS,
            edits=(
                (
                    '        let kind = if is_first_param && matches!(name, SELF_PARAMETER | "cls") {',
                    "        let kind = if is_first_param && name == SELF_PARAMETER {",
                ),
            ),
            cases=("identical_positional_types::pass::classmethod_cls_excluded_below_min",),
        ),
        Mutation(
            label="`**kwargs` variadic parameter excluded from identical-positional-types",
            path=FUNCTIONS,
            edits=(
                (
                    "        result.push(build_variadic_parameter(\n"
                    "            kwarg,\n"
                    "            PythonParameterKind::VarKeyword,\n"
                    "            file,\n"
                    "        ));",
                    "        result.push(build_variadic_parameter(\n"
                    "            kwarg,\n"
                    "            PythonParameterKind::Positional,\n"
                    "            file,\n"
                    "        ));",
                ),
            ),
            cases=("identical_positional_types::pass::var_keyword_excluded",),
        ),
        Mutation(
            label="unannotated parameters not grouped as duplicates in identical-positional-types",
            path=IDENTICAL_POSITIONAL_TYPES,
            edits=(
                (
                    "        if let Some(ref type_annotation) = param.type_text {",
                    '        let fallback = String::new();\n'
                    '        let type_annotation = param.type_text.as_ref().unwrap_or(&fallback);\n'
                    "        {",
                ),
            ),
            cases=("identical_positional_types::pass::untyped_parameters_not_grouped",),
        ),
        Mutation(
            label="data-model dunder methods exempt from identical-positional-types",
            path=FUNCTIONS,
            edits=(
                (
                    "        is_exempt_dunder_method(&self.name) || self.has_exempt_signature_decorator",
                    "        self.has_exempt_signature_decorator",
                ),
            ),
            cases=("identical_positional_types::pass::dunder_method_exempt",),
        ),
        Mutation(
            label="@overload decorator exempt from identical-positional-types",
            path=FUNCTIONS,
            edits=(('| "overload"', ""),),
            cases=("identical_positional_types::pass::decorator_overload_exempt",),
        ),
        Mutation(
            label="@fixture decorator exempt from identical-positional-types",
            path=FUNCTIONS,
            edits=(('| "fixture"', ""),),
            cases=("identical_positional_types::pass::decorator_fixture_exempt",),
        ),
        Mutation(
            label="@override decorator exempt from identical-positional-types",
            path=FUNCTIONS,
            edits=(("OVERRIDE_DECORATOR | ", ""),),
            cases=("identical_positional_types::pass::decorator_override_exempt",),
        ),
        Mutation(
            label="@abstractmethod decorator exempt from identical-positional-types",
            path=FUNCTIONS,
            edits=(('| "abstractmethod"', ""),),
            cases=("identical_positional_types::pass::decorator_abstractmethod_exempt",),
        ),
        Mutation(
            label="@<fn>.register decorator exempt from identical-positional-types",
            path=FUNCTIONS,
            edits=(('| "register"', ""),),
            cases=("identical_positional_types::pass::decorator_singledispatch_register_exempt",),
        ),
        # --- repeated-index-access ---
        Mutation(
            label="single distinct position read exempt from repeated-index-access",
            path=REPEATED_INDEX_ACCESS,
            edits=(
                (
                    "            group.positions.len() >= min_positions",
                    "            group.positions.len() >= min_positions.min(1)",
                ),
            ),
            cases=(
                "repeated_index_access::pass::single_position_read",
                "repeated_index_access::pass::newtype_single_field",
            ),
        ),
        Mutation(
            label="reading the same position twice deduplicated in repeated-index-access",
            path=REPEATED_INDEX_ACCESS,
            edits=(
                (
                    "                group.positions.insert(read.position);",
                    "                group.positions.insert(read.position + group.positions.len() as i64);",
                ),
            ),
            cases=("repeated_index_access::pass::same_position_twice",),
        ),
        Mutation(
            label="Python call receiver excluded from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "        let is_stable = is_stable_receiver_expr(&subscript.value);",
                    "        let is_stable = {\n"
                    "            let _ = is_stable_receiver_expr(&subscript.value);\n"
                    "            true\n"
                    "        };",
                ),
            ),
            cases=("repeated_index_access::pass::call_receiver@python",),
        ),
        Mutation(
            label="Rust call receiver excluded from repeated-index-access",
            path=RUST,
            edits=(
                (
                    "    } else if is_stable_receiver(&receiver)",
                    "    } else if { let _ = is_stable_receiver(&receiver); true }",
                ),
            ),
            cases=("repeated_index_access::pass::call_receiver@rust",),
        ),
        Mutation(
            label="leading placeholder count enforced in repeated-index-access",
            path=REPEATED_INDEX_ACCESS,
            edits=(
                (
                    "    let leading_gaps = leading\n"
                    "        .clone()\n"
                    "        .next_back()\n"
                    "        .map_or(0, |&last| last.unsigned_abs() + 1 - leading.count() as u64);",
                    "    let _ = leading;\n    let leading_gaps = 0;",
                ),
            ),
            cases=(
                "repeated_index_access::pass::three_placeholders_over_limit@python",
                "repeated_index_access::pass::three_placeholders_over_limit@rust",
            ),
        ),
        Mutation(
            label="trailing placeholder count enforced in repeated-index-access",
            path=REPEATED_INDEX_ACCESS,
            edits=(
                (
                    "    let trailing_gaps = trailing\n"
                    "        .clone()\n"
                    "        .next()\n"
                    "        .map_or(0, |&first| first.unsigned_abs() - trailing.count() as u64);",
                    "    let _ = trailing;\n    let trailing_gaps = 0;",
                ),
            ),
            cases=("repeated_index_access::pass::tail_placeholders_over_limit",),
        ),
        Mutation(
            label="Python subscript Store context exempts receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "        let is_load = matches!(subscript.ctx, ruff_python_ast::ExprContext::Load);",
                    "        let is_load = matches!(\n"
                    "            subscript.ctx,\n"
                    "            ruff_python_ast::ExprContext::Load | ruff_python_ast::ExprContext::Store\n"
                    "        );",
                ),
            ),
            cases=(
                "repeated_index_access::pass::subscript_write_exempts_receiver",
                "repeated_index_access::pass::augmented_write_exempts_receiver",
                "repeated_index_access::pass::tuple_target_exempts_receiver",
                "repeated_index_access::pass::for_target_exempts_receiver",
            ),
        ),
        Mutation(
            label="Python `del` subscript exempts receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "        let is_load = matches!(subscript.ctx, ruff_python_ast::ExprContext::Load);",
                    "        let is_load = matches!(\n"
                    "            subscript.ctx,\n"
                    "            ruff_python_ast::ExprContext::Load | ruff_python_ast::ExprContext::Del\n"
                    "        );",
                ),
            ),
            cases=("repeated_index_access::pass::delete_exempts_receiver",),
        ),
        Mutation(
            label="Python non-literal subscript (slice, variable, tuple key) exempts receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "        } else {\n            scope.exempt_receivers.insert(receiver_text);\n        }",
                    "        }",
                ),
            ),
            cases=(
                "repeated_index_access::pass::slice_exempts_receiver",
                "repeated_index_access::pass::variable_index_exempts_receiver",
                "repeated_index_access::pass::tuple_key_exempts_receiver",
            ),
        ),
        Mutation(
            label="Python non-decimal integer index exempts receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "    if !text.bytes().all(|byte| byte.is_ascii_digit()) {\n"
                    "        return None;\n"
                    "    }",
                    "    if let ruff_python_ast::Number::Int(int_val) = &number.value {\n"
                    "        return int_val.as_i64();\n"
                    "    }",
                ),
            ),
            cases=("repeated_index_access::pass::non_decimal_index_exempts_receiver",),
        ),
        Mutation(
            label="Python `for` loop iteration exempts receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(("                self.record_exempt_receiver(&for_statement.iter);\n", ""),),
            cases=("repeated_index_access::pass::iterated_exempts_receiver",),
        ),
        Mutation(
            label="Python collection builtins (`enumerate`, `len`) exempt receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "            Expr::Name(func_name) if is_collection_builtin(func_name.id.as_str()) => {\n"
                    "                for arg in &call.arguments.args {\n"
                    "                    self.record_exempt_receiver(arg);\n"
                    "                }\n"
                    "            }",
                    "",
                ),
            ),
            cases=(
                "repeated_index_access::pass::enumerate_exempts_receiver",
                "repeated_index_access::pass::len_exempts_receiver",
            ),
        ),
        Mutation(
            label="Python mutating method call exempts receiver from repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "            Expr::Attribute(attr) if MUTATING_METHODS.contains(&attr.attr.as_str()) => {\n"
                    "                self.record_exempt_receiver(&attr.value);\n"
                    "            }",
                    "",
                ),
            ),
            cases=("repeated_index_access::pass::mutating_method_exempts_receiver",),
        ),
        Mutation(
            label="Python default parameter values evaluated in enclosing scope in repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "                self.visit_parameters(&func.parameters);\n"
                    "                if let Some(returns) = &func.returns {\n"
                    "                    self.visit_annotation(returns);\n"
                    "                }\n"
                    "                let prev_scope = self.current_scope.replace(ScopePositionalReads::default());",
                    "                let prev_scope = self.current_scope.replace(ScopePositionalReads::default());\n"
                    "                self.visit_parameters(&func.parameters);\n"
                    "                if let Some(returns) = &func.returns {\n"
                    "                    self.visit_annotation(returns);\n"
                    "                }",
                ),
            ),
            cases=("repeated_index_access::pass::default_arguments_in_enclosing_scope",),
        ),
        Mutation(
            label="Python positional reads scoped per function in repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "                let prev_scope = self.current_scope.replace(ScopePositionalReads::default());\n"
                    "                self.visit_body(&func.body);\n"
                    "                if let Some(finished_scope) = std::mem::replace(&mut self.current_scope, prev_scope)\n"
                    "                {\n"
                    "                    self.out.push(finished_scope);\n"
                    "                }",
                    "                self.visit_body(&func.body);",
                ),
            ),
            cases=("repeated_index_access::pass::reads_split_across_functions@python",),
        ),
        Mutation(
            label="Python lambda body skipped in repeated-index-access",
            path=POSITIONAL_READS,
            edits=(("            Expr::Lambda(_) => return,\n", ""),),
            cases=("repeated_index_access::pass::lambda_body_skipped",),
        ),
        Mutation(
            label="Python class body ignored in repeated-index-access",
            path=POSITIONAL_READS,
            edits=(
                (
                    "            Stmt::ClassDef(_) => {\n"
                    "                let prev_scope = self.current_scope.take();\n"
                    "                walk_stmt(self, statement);\n"
                    "                self.current_scope = prev_scope;\n"
                    "            }",
                    "",
                ),
            ),
            cases=("repeated_index_access::pass::class_body_ignored",),
        ),
        Mutation(
            label="Rust tuple field assignment exempts receiver from repeated-index-access",
            path=RUST,
            edits=(
                (
                    "                if !matches!(bin_expr.op_kind(), Some(ast::BinaryOp::Assignment { .. })) {\n"
                    "                    return false;\n"
                    "                }",
                    "                let _ = bin_expr;\n                return false;",
                ),
            ),
            cases=("repeated_index_access::pass::field_write_exempts_receiver",),
        ),
        Mutation(
            label="Rust mutable borrow of tuple field exempts receiver from repeated-index-access",
            path=RUST,
            edits=(
                (
                    "            SyntaxKind::REF_EXPR => {\n"
                    "                return ast::RefExpr::cast(parent)\n"
                    "                    .is_some_and(|ref_expr| ref_expr.mut_token().is_some());\n"
                    "            }",
                    "",
                ),
            ),
            cases=("repeated_index_access::pass::mutable_borrow_exempts_receiver",),
        ),
        Mutation(
            label="Rust destructuring swap assignment exempts receiver from repeated-index-access",
            path=RUST,
            edits=(
                (
                    "            SyntaxKind::TUPLE_EXPR | SyntaxKind::PAREN_EXPR => place = parent,\n",
                    "",
                ),
            ),
            cases=("repeated_index_access::pass::swap_assignment_exempts_receiver",),
        ),
        Mutation(
            label="Rust items outside functions ignored in repeated-index-access",
            path=RUST,
            edits=(
                (
                    "    let mut out = Vec::new();\n"
                    "    collect_positional_reads_rec(parsed.tree().syntax(), file, None, &mut out);\n"
                    "    out",
                    "    let mut out = Vec::new();\n"
                    "    let mut root_scope = ScopePositionalReads::default();\n"
                    "    collect_positional_reads_rec(parsed.tree().syntax(), file, Some(&mut root_scope), &mut out);\n"
                    "    out.push(root_scope);\n"
                    "    out",
                ),
            ),
            cases=("repeated_index_access::pass::items_outside_functions_ignored",),
        ),
        Mutation(
            label="Rust positional reads scoped per function in repeated-index-access",
            path=RUST,
            edits=(
                (
                    "    if ast::Fn::can_cast(node.kind()) {\n"
                    "        let mut function_scope = ScopePositionalReads::default();\n"
                    "        for child in node.children() {\n"
                    "            collect_positional_reads_rec(&child, file, Some(&mut function_scope), out);\n"
                    "        }\n"
                    "        out.push(function_scope);\n"
                    "        return;\n"
                    "    }",
                    "    if ast::Fn::can_cast(node.kind()) && scope.is_none() {\n"
                    "        let mut function_scope = ScopePositionalReads::default();\n"
                    "        collect_positional_reads_rec(&node.parent().unwrap(), file, Some(&mut function_scope), out);\n"
                    "        out.push(function_scope);\n"
                    "        return;\n"
                    "    }",
                ),
            ),
            cases=("repeated_index_access::pass::reads_split_across_functions@rust",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts for Batch 6 rules",
            reason=(
                "No exemption: functions that unpack tuples or destructure structs instead of "
                "indexing by position, and functions whose positional parameter types are all distinct."
            ),
            cases=(
                "repeated_index_access::pass::canonical_unpacking",
                "repeated_index_access::pass::canonical_destructuring",
                "identical_positional_types::pass::distinct_positional_types",
            ),
        ),
        Unmutated(
            label="constructs the Batch 6 extractors never visit",
            reason=(
                "`find_nested_functions` only visits `Stmt::FunctionDef` (never `Expr::Lambda`); "
                "`rust::collect_call_candidates` only visits `CallExpr`/`MethodCallExpr` (never "
                "`MacroCall` such as `env!`); and `ra_ap_syntax` leaves macro arguments as unparsed "
                "`TokenTree`s rather than `ast::FieldExpr` nodes."
            ),
            cases=(
                "nested_function::pass::lambda_callbacks_allowed",
                "environment_variable_in_function::pass::compile_time_env_macro_allowed",
                "repeated_index_access::pass::known_gap_macro_arguments_not_inspected",
            ),
        ),
    ),
)
