"""Exemption mutations for Batch 3: Collection Types II."""

from __future__ import annotations

from .model import (
    ANNOTATIONS,
    CLASSES,
    COMMENTS,
    CONTRACT,
    FUNCTIONS,
    MUTABLE_COLLECTION_ATTRIBUTE,
    MUTABLE_MODULE_CONSTANT,
    NULLABLE_COLLECTION_RETURN,
    PARAMETER_USAGE,
    PYTHON,
    RUST,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "mutable_collection_parameter",
        "mutable_collection_return",
        "mutable_collection_attribute",
        "nullable_collection_return",
        "mutable_module_constant",
    ),
    mutations=(
        # --- Parameter mutation and escape exemptions (mutable-collection-parameter) ---
        Mutation(
            label="mutating sequence methods mark parameter as mutated",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            func => self.visit_expr(func),",
                    "            Expr::Attribute(attr) => {\n"
                    '                if matches!(attr.attr.as_str(), "append" | "sort") {\n'
                    "                    self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "                } else {\n"
                    "                    self.visit_expr(call.func.as_ref());\n"
                    "                }\n"
                    "            }\n"
                    "            func => self.visit_expr(func),",
                ),
            ),
            cases=("mutable_collection_parameter::pass::mutating_sequence_methods",),
        ),
        Mutation(
            label="mutating mapping methods mark parameter as mutated",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            func => self.visit_expr(func),",
                    "            Expr::Attribute(attr) => {\n"
                    '                if attr.attr.as_str() == "setdefault" {\n'
                    "                    self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "                } else {\n"
                    "                    self.visit_expr(call.func.as_ref());\n"
                    "                }\n"
                    "            }\n"
                    "            func => self.visit_expr(func),",
                ),
            ),
            cases=("mutable_collection_parameter::pass::mutating_mapping_methods",),
        ),
        Mutation(
            label="mutating set methods mark parameter as mutated",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            func => self.visit_expr(func),",
                    "            Expr::Attribute(attr) => {\n"
                    '                if attr.attr.as_str() == "discard" {\n'
                    "                    self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "                } else {\n"
                    "                    self.visit_expr(call.func.as_ref());\n"
                    "                }\n"
                    "            }\n"
                    "            func => self.visit_expr(func),",
                ),
            ),
            cases=("mutable_collection_parameter::pass::mutating_set_methods",),
        ),
        Mutation(
            label="subscript Store context marks parameter as mutated",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "Expr::Subscript(sub) if matches!(sub.ctx, ruff_python_ast::ExprContext::Load) => {",
                    "Expr::Subscript(sub)\n"
                    "                if matches!(\n"
                    "                    sub.ctx,\n"
                    "                    ruff_python_ast::ExprContext::Load | ruff_python_ast::ExprContext::Store\n"
                    "                ) =>\n"
                    "            {",
                ),
            ),
            cases=(
                "mutable_collection_parameter::pass::subscript_write",
                "mutable_collection_parameter::pass::slice_write",
                "mutable_collection_parameter::pass::subscript_augmented_write",
            ),
        ),
        Mutation(
            label="subscript Del context marks parameter as mutated",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "Expr::Subscript(sub) if matches!(sub.ctx, ruff_python_ast::ExprContext::Load) => {",
                    "Expr::Subscript(sub)\n"
                    "                if matches!(\n"
                    "                    sub.ctx,\n"
                    "                    ruff_python_ast::ExprContext::Load | ruff_python_ast::ExprContext::Del\n"
                    "                ) =>\n"
                    "            {",
                ),
            ),
            cases=("mutable_collection_parameter::pass::subscript_deletion",),
        ),
        Mutation(
            label="augmented assignment target marks parameter as mutated",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            _ => walk_stmt(self, statement),",
                    "            Stmt::AugAssign(aug) => self.visit_expr(&aug.value),\n"
                    "            _ => walk_stmt(self, statement),",
                ),
            ),
            cases=("mutable_collection_parameter::pass::augmented_assignment_on_parameter",),
        ),
        Mutation(
            label="passing parameter to unknown function marks it as escaping",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "        let Some(positional_role) = positional_role else {\n"
                    "            self.visit_arguments(&call.arguments);\n"
                    "            return;\n"
                    "        };",
                    "        let positional_role = positional_role.unwrap_or(UseRole::Read);",
                ),
            ),
            cases=(
                "mutable_collection_parameter::pass::passed_to_unknown_function",
                "mutable_collection_parameter::pass::passed_to_method_argument",
            ),
        ),
        Mutation(
            label="assigning parameter to attribute or local variable marks it as escaping",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            _ => walk_stmt(self, statement),",
                    "            Stmt::Assign(assign) => self.visit_expr_as(&assign.value, UseRole::Read),\n"
                    "            _ => walk_stmt(self, statement),",
                ),
            ),
            cases=(
                "mutable_collection_parameter::pass::stored_in_attribute",
                "mutable_collection_parameter::pass::aliased_to_local_variable",
            ),
        ),
        Mutation(
            label="storing parameter in list display marks it as escaping",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "                _ => self.visit_expr(element),",
                    "                _ => self.visit_expr_as(element, UseRole::Read),",
                ),
            ),
            cases=("mutable_collection_parameter::pass::stored_in_container",),
        ),
        Mutation(
            label="returning parameter marks it as escaping",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            _ => walk_stmt(self, statement),",
                    "            Stmt::Return(_) => {}\n"
                    "            _ => walk_stmt(self, statement),",
                ),
            ),
            cases=("mutable_collection_parameter::pass::returned",),
        ),
        Mutation(
            label="yielding parameter marks it as escaping",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            Expr::YieldFrom(yield_from) => {",
                    "            Expr::Yield(_) => {}\n"
                    "            Expr::YieldFrom(yield_from) => {",
                ),
            ),
            cases=("mutable_collection_parameter::pass::yielded",),
        ),
        Mutation(
            label="nested function body visited for parameter mutations",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "                self.visit_nested_scope(Some(&func.parameters), |visitor| {\n"
                    "                    visitor.visit_body(&func.body);\n"
                    "                });",
                    "",
                ),
            ),
            cases=("mutable_collection_parameter::pass::mutated_inside_nested_closure",),
        ),
        Mutation(
            label="nested function default evaluated in enclosing scope before shadowing",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "                // Defaults are evaluated in the enclosing scope; only the body is shadowed.\n"
                    "                self.visit_parameters(&func.parameters);",
                    "",
                ),
            ),
            cases=("mutable_collection_parameter::pass::shadowing_nested_def_default_captures_parameter",),
        ),
        Mutation(
            label="ellipsis stub body exempt from parameter usage rules",
            path=FUNCTIONS,
            edits=(
                (
                    "        Stmt::Expr(expr_statement) => {\n"
                    "            matches!(expr_statement.value.as_ref(), Expr::EllipsisLiteral(_))\n"
                    "        }",
                    "        Stmt::Expr(_) => false,",
                ),
            ),
            cases=("mutable_collection_parameter::pass::ellipsis_stub_body_exempt",),
        ),
        Mutation(
            label="pass stub body exempt from parameter usage rules",
            path=FUNCTIONS,
            edits=(
                (
                    "        Stmt::Pass(_) => true,",
                    "        Stmt::Pass(_) => false,",
                ),
            ),
            cases=("mutable_collection_parameter::pass::pass_stub_body_exempt",),
        ),
        Mutation(
            label="raise NotImplementedError stub body exempt from parameter usage rules",
            path=FUNCTIONS,
            edits=(
                (
                    "        Stmt::Raise(raise_statement) => {\n"
                    "            raise_statement.cause.is_none()\n"
                    "                && raise_statement.exc.as_deref().is_some_and(|exc| match exc {\n"
                    "                    Expr::Name(name) => name.id == NOT_IMPLEMENTED_ERROR,\n"
                    "                    Expr::Call(call) => {\n"
                    "                        resolve_path_and_terminal_expr(&call.func, source).1\n"
                    "                            == NOT_IMPLEMENTED_ERROR\n"
                    "                    }\n"
                    "                    _ => false,\n"
                    "                })\n"
                    "        }",
                    "        Stmt::Raise(_) => false,",
                ),
            ),
            cases=("mutable_collection_parameter::pass::raise_not_implemented_stub_body_exempt",),
        ),
        Mutation(
            label="Protocol class methods exempt from collection type rules (batch 3)",
            path=CLASSES,
            edits=((
                "matches!(terminal.as_str(), PROTOCOL_CLASS | ABC_CLASS)",
                "terminal.as_str() == ABC_CLASS",
            ),),
            cases=(
                "mutable_collection_parameter::pass::protocol_class_exempt",
                "mutable_collection_return::pass::protocol_class_exempt",
                "mutable_collection_attribute::pass::protocol_class_exempt",
                "nullable_collection_return::pass::protocol_class_exempt",
            ),
        ),
        Mutation(
            label="ABC class exempt from collection type rules (batch 3)",
            path=CLASSES,
            edits=((
                "matches!(terminal.as_str(), PROTOCOL_CLASS | ABC_CLASS)",
                "terminal.as_str() == PROTOCOL_CLASS",
            ),),
            cases=(
                "mutable_collection_attribute::pass::abc_class_exempt",
                "nullable_collection_return::pass::abc_class_exempt",
            ),
        ),
        Mutation(
            label="@override decorator exempt from Python signature rules (batch 3)",
            path=FUNCTIONS,
            edits=((
                'OVERRIDE_DECORATOR | "overload"',
                '"overload"',
            ),),
            cases=(
                "mutable_collection_parameter::pass::override_exempt",
                "mutable_collection_return::pass::override_exempt",
                "nullable_collection_return::pass::override_exempt",
            ),
        ),
        Mutation(
            label="@overload decorator exempt from Python signature rules (batch 3)",
            path=FUNCTIONS,
            edits=((
                '| "overload" |',
                "|",
            ),),
            cases=(
                "mutable_collection_return::pass::overload_exempt",
                "nullable_collection_return::pass::overload_exempt",
            ),
        ),
        Mutation(
            label="@abstractmethod decorator exempt from Python signature rules (batch 3)",
            path=FUNCTIONS,
            edits=((
                '| "abstractmethod" |',
                "|",
            ),),
            cases=(
                "mutable_collection_return::pass::abstractmethod_exempt",
                "nullable_collection_return::pass::abstractmethod_exempt",
            ),
        ),
        Mutation(
            label="@pytest.fixture decorator exempt from Python signature rules (batch 3)",
            path=FUNCTIONS,
            edits=((
                '| "fixture" |',
                "|",
            ),),
            cases=("nullable_collection_return::pass::pytest_fixture_exempt",),
        ),
        Mutation(
            label="@<fn>.register singledispatch decorator exempt from Python signature rules (batch 3)",
            path=FUNCTIONS,
            edits=((
                '| "register" |',
                "|",
            ),),
            cases=("nullable_collection_return::pass::singledispatch_register_exempt",),
        ),
        Mutation(
            label="data-model dunder methods exempt from Python signature rules (batch 3)",
            path=FUNCTIONS,
            edits=((
                '!matches!(func_name, "__init__" | "__new__" | "__call__")',
                "false",
            ),),
            cases=("nullable_collection_return::pass::data_model_dunder_exempt",),
        ),
        Mutation(
            label="preceding header comment satisfies RequireExplanation (batch 3)",
            path=CONTRACT,
            edits=((
                "if mode == EnforcementMode::RequireExplanation && !diagnostics.is_empty() {",
                "if false && mode == EnforcementMode::RequireExplanation && !diagnostics.is_empty() {",
            ),),
            cases=(
                "mutable_collection_return::pass::explained_by_header_comment",
                "mutable_collection_attribute::pass::explained_public_mutable_attribute",
                "nullable_collection_return::pass::explained_by_preceding_header_comment",
                "nullable_collection_return::pass::explained_on_decorated_function_header",
                "nullable_collection_return::pass::explained_attributed_rust_function_exempt",
            ),
        ),
        # --- Local return-value mutation exemptions (mutable-collection-return) ---
        Mutation(
            label="chained call mutation exempts function in mutable-collection-return",
            path=PYTHON,
            edits=(
                (
                    "                if let Some(callee_name) = called_terminal_name_expr(receiver, self.source) {\n"
                    "                    self.mutated_functions.insert(callee_name);",
                    "                if false {",
                ),
            ),
            cases=(
                "mutable_collection_return::pass::locally_mutated_via_chained_call",
                "mutable_collection_return::pass::known_gap_same_named_callee_exempts_function",
            ),
        ),
        Mutation(
            label="assigned variable mutation exempts function in mutable-collection-return",
            path=PYTHON,
            edits=(
                (
                    "                    Stmt::Assign(assign) => {\n"
                    "                        if let [target] = assign.targets.as_slice() {\n"
                    "                            self.record_binding(target, &assign.value);\n"
                    "                        }\n"
                    "                        walk_stmt(self, statement);\n"
                    "                    }",
                    "                    Stmt::Assign(_) => walk_stmt(self, statement),",
                ),
            ),
            cases=("mutable_collection_return::pass::locally_mutated_via_assigned_variable",),
        ),
        Mutation(
            label="walrus binding mutation exempts function in mutable-collection-return",
            path=PYTHON,
            edits=(
                (
                    "                if let Expr::Named(named) = expr {\n"
                    "                    self.record_binding(&named.target, &named.value);\n"
                    "                }",
                    "",
                ),
            ),
            cases=("mutable_collection_return::pass::locally_mutated_via_walrus_binding",),
        ),
        Mutation(
            label="mutable-collection-return and mutable-collection-attribute only unwrap transparent wrappers",
            path=ANNOTATIONS,
            edits=(
                (
                    "            if depth == AnnotationTraversalDepth::CovariantPositions\n"
                    "                && is_std_type_constructor_prefix(&base_path, &base_terminal)\n"
                    "                && !is_concrete_collection_constructor(&base_path, &base_terminal)\n"
                    "            {",
                    "            if is_std_type_constructor_prefix(&base_path, &base_terminal)\n"
                    "                && !is_concrete_collection_constructor(&base_path, &base_terminal)\n"
                    "            {",
                ),
            ),
            cases=(
                "mutable_collection_return::pass::nested_mutable_type_not_checked",
                "mutable_collection_attribute::pass::nested_mutable_elements_not_checked",
            ),
        ),
        # --- Class attribute exemptions (mutable-collection-attribute) ---
        Mutation(
            label="in-class attribute mutation exempt from mutable-collection-attribute",
            path=MUTABLE_COLLECTION_ATTRIBUTE,
            edits=(
                (
                    "            || attribute.is_mutated_in_class\n",
                    "",
                ),
            ),
            cases=("mutable_collection_attribute::pass::mutated_in_class_method",),
        ),
        Mutation(
            label="underscore-prefixed private attribute exempt from mutable-collection-attribute",
            path=MUTABLE_COLLECTION_ATTRIBUTE,
            edits=(
                (
                    "if attribute.name.starts_with('_')",
                    "if false",
                ),
            ),
            cases=("mutable_collection_attribute::pass::private_mutable_attributes_exempt",),
        ),
        Mutation(
            label="TypedDict keys exempt from mutable-collection-attribute",
            path=MUTABLE_COLLECTION_ATTRIBUTE,
            edits=(
                (
                    "            || attribute.is_typed_dict_key\n",
                    "",
                ),
            ),
            cases=("mutable_collection_attribute::pass::typed_dict_keys_exempt",),
        ),
        # --- Nullable collection return exemptions (nullable-collection-return) ---
        Mutation(
            label="fixed-length record tuple exempt from nullable-collection-return",
            path=NULLABLE_COLLECTION_RETURN,
            edits=(
                (
                    '        let is_variadic = args.len() == 2 && args[1].text() == "...";\n'
                    "        if !is_variadic {\n"
                    "            return None;\n"
                    "        }",
                    "",
                ),
            ),
            cases=("nullable_collection_return::pass::fixed_length_record_tuple_nullable",),
        ),
        Mutation(
            label="union mixing collection and non-collection type exempt from nullable-collection-return",
            path=NULLABLE_COLLECTION_RETURN,
            edits=(
                (
                    "                let type_name = python_collection_branch_type(branch)?;",
                    "                let Some(type_name) = python_collection_branch_type(branch) else { continue; };",
                ),
            ),
            cases=("nullable_collection_return::pass::mixed_collection_with_non_collection_union_exempt",),
        ),
        Mutation(
            label="Rust trait declaration exempt from nullable-collection-return",
            path=RUST,
            edits=(
                (
                    "    ast::Trait::can_cast(enclosing_item.kind())\n"
                    "        || ast::Impl::cast(enclosing_item).is_some_and(|impl_item| impl_item.trait_().is_some())",
                    "    ast::Impl::cast(enclosing_item).is_some_and(|impl_item| impl_item.trait_().is_some())",
                ),
            ),
            cases=("nullable_collection_return::pass::trait_declaration_exempt",),
        ),
        Mutation(
            label="Rust trait impl exempt from nullable-collection-return",
            path=RUST,
            edits=(
                (
                    "    ast::Trait::can_cast(enclosing_item.kind())\n"
                    "        || ast::Impl::cast(enclosing_item).is_some_and(|impl_item| impl_item.trait_().is_some())",
                    "    ast::Trait::can_cast(enclosing_item.kind())",
                ),
            ),
            cases=("nullable_collection_return::pass::trait_impl_exempt",),
        ),
        # --- Module constant exemptions (mutable-module-constant) ---
        Mutation(
            label="Mapping-annotated dict literal/comprehension/call exempt from mutable-module-constant",
            path=MUTABLE_MODULE_CONSTANT,
            edits=(
                (
                    "    if is_read_only_mapping && built.name == DICT {\n"
                    "        return None;\n"
                    "    }",
                    "",
                ),
            ),
            cases=(
                "mutable_module_constant::pass::mapping_annotated_dict_literal_exempt",
                "mutable_module_constant::pass::mapping_annotated_dict_comprehension_exempt",
                "mutable_module_constant::pass::mapping_annotated_dict_call_exempt",
            ),
        ),
        Mutation(
            label="dunder __all__ exempt from mutable-module-constant",
            path=MUTABLE_MODULE_CONSTANT,
            edits=(
                (
                    "        if is_dunder || !assignment.is_constant() {",
                    "        if !assignment.is_constant() {",
                ),
            ),
            cases=("mutable_module_constant::pass::dunder_all_exempt",),
        ),
        Mutation(
            label="lowercase module variable without Final exempt from mutable-module-constant",
            path=MUTABLE_MODULE_CONSTANT,
            edits=(
                (
                    "        if is_dunder || !assignment.is_constant() {",
                    "        if is_dunder {",
                ),
            ),
            cases=("mutable_module_constant::pass::lowercase_module_state_not_flagged",),
        ),
        Mutation(
            label="class-body assignments not collected by collect_module_assignments",
            path=PYTHON,
            edits=(
                (
                    "            Stmt::With(with_statement) => {\n"
                    "                collect_module_assignments_in_stmts(&with_statement.body, file, out);\n"
                    "            }",
                    "            Stmt::With(with_statement) => {\n"
                    "                collect_module_assignments_in_stmts(&with_statement.body, file, out);\n"
                    "            }\n"
                    "            Stmt::ClassDef(class_def) => {\n"
                    "                collect_module_assignments_in_stmts(&class_def.body, file, out);\n"
                    "            }",
                ),
            ),
            cases=("mutable_module_constant::pass::class_scope_not_flagged",),
        ),
        Mutation(
            label="function-body assignments not collected by collect_module_assignments",
            path=PYTHON,
            edits=(
                (
                    "            Stmt::With(with_statement) => {\n"
                    "                collect_module_assignments_in_stmts(&with_statement.body, file, out);\n"
                    "            }",
                    "            Stmt::With(with_statement) => {\n"
                    "                collect_module_assignments_in_stmts(&with_statement.body, file, out);\n"
                    "            }\n"
                    "            Stmt::FunctionDef(func_def) => {\n"
                    "                collect_module_assignments_in_stmts(&func_def.body, file, out);\n"
                    "            }",
                ),
            ),
            cases=("mutable_module_constant::pass::function_scope_not_flagged",),
        ),
        Mutation(
            label="attribute target not collected by collect_module_assignments",
            path=PYTHON,
            edits=(
                (
                    "            Stmt::AnnAssign(ann) => {\n"
                    "                if let Expr::Name(target) = ann.target.as_ref() {",
                    "            Stmt::AnnAssign(ann) => {\n"
                    "                let target_id = match ann.target.as_ref() {\n"
                    "                    Expr::Name(target) => Some(target.id.to_string()),\n"
                    "                    Expr::Attribute(attr) => Some(attr.attr.to_string()),\n"
                    "                    _ => None,\n"
                    "                };\n"
                    "                if let Some(name) = target_id {",
                ),
                (
                    "                        name: target.id.to_string(),\n"
                    "                        annotation: Some(AstNode::from_span(",
                    "                        name,\n"
                    "                        annotation: Some(AstNode::from_span(",
                ),
            ),
            cases=("mutable_module_constant::pass::attribute_target_not_flagged",),
        ),
        Mutation(
            label="unpacking target not collected by collect_module_assignments",
            path=PYTHON,
            edits=(
                (
                    "            Stmt::Assign(assign) => {\n"
                    "                if let [Expr::Name(target)] = assign.targets.as_slice() {\n"
                    "                    out.push(PythonModuleAssignment {\n"
                    "                        name: target.id.to_string(),\n"
                    "                        annotation: None,\n"
                    "                        value: Some(AstNode::from_span(\n"
                    "                            file,\n"
                    "                            span_from_ruff_range(assign.value.range()),\n"
                    "                        )),",
                    "            Stmt::Assign(assign) => {\n"
                    "                let target_and_val = match assign.targets.as_slice() {\n"
                    "                    [Expr::Name(target)] => Some((target.id.to_string(), assign.value.as_ref())),\n"
                    "                    [Expr::Tuple(tuple)] => tuple.elts.first().and_then(|elt| {\n"
                    "                        let val = if let Expr::Tuple(val_tuple) = assign.value.as_ref() {\n"
                    "                            val_tuple.elts.first().unwrap_or(&assign.value)\n"
                    "                        } else {\n"
                    "                            assign.value.as_ref()\n"
                    "                        };\n"
                    "                        if let Expr::Name(target) = elt { Some((target.id.to_string(), val)) } else { None }\n"
                    "                    }),\n"
                    "                    _ => None,\n"
                    "                };\n"
                    "                if let Some((name, val_expr)) = target_and_val {\n"
                    "                    out.push(PythonModuleAssignment {\n"
                    "                        name,\n"
                    "                        annotation: None,\n"
                    "                        value: Some(AstNode::from_span(\n"
                    "                            file,\n"
                    "                            span_from_ruff_range(val_expr.range()),\n"
                    "                        )),",
                ),
            ),
            cases=("mutable_module_constant::pass::unpacking_target_not_flagged",),
        ),
        Mutation(
            label="TypeAlias annotation does not classify value as runtime mutable constructor",
            path=MUTABLE_MODULE_CONSTANT,
            edits=(
                (
                    "    let built = collection_display(value).or_else(|| {\n"
                    "        call_callee(value)\n"
                    "            .and_then(|callee| collection_type(&callee))\n"
                    "            .filter(is_runtime_mutable_collection_constructor)\n"
                    "    })?;",
                    "    let built = collection_display(value)\n"
                    "        .or_else(|| {\n"
                    "            call_callee(value)\n"
                    "                .and_then(|callee| collection_type(&callee))\n"
                    "                .filter(is_runtime_mutable_collection_constructor)\n"
                    "        })\n"
                    "        .or_else(|| {\n"
                    "            collect_collection_types(value, AnnotationTraversalDepth::TransparentWrappersOnly)\n"
                    "                .into_iter()\n"
                    "                .find(|collection_type| collection_type.kind == CollectionKind::ConcreteMutable)\n"
                    "        })?;",
                ),
            ),
            cases=("mutable_module_constant::pass::typing_type_alias_not_flagged",),
        ),
        Mutation(
            label="PEP 695 type statement not collected by collect_module_assignments",
            path=PYTHON,
            edits=(
                (
                    "            Stmt::With(with_statement) => {\n"
                    "                collect_module_assignments_in_stmts(&with_statement.body, file, out);\n"
                    "            }",
                    "            Stmt::With(with_statement) => {\n"
                    "                collect_module_assignments_in_stmts(&with_statement.body, file, out);\n"
                    "            }\n"
                    "            Stmt::TypeAlias(type_alias) => {\n"
                    "                if let Expr::Name(target) = type_alias.name.as_ref() {\n"
                    "                    out.push(PythonModuleAssignment {\n"
                    "                        name: target.id.to_string(),\n"
                    "                        annotation: Some(AstNode::from_span(\n"
                    "                            file,\n"
                    "                            span_from_ruff_range(type_alias.value.range()),\n"
                    "                        )),\n"
                    "                        value: None,\n"
                    "                        has_final_annotation: true,\n"
                    "                    });\n"
                    "                }\n"
                    "            }",
                ),
            ),
            cases=("mutable_module_constant::pass::pep695_type_alias_not_flagged",),
        ),
        Mutation(
            label="from collections.abc import Set exempts unqualified Set in mutable-module-constant",
            path=ANNOTATIONS,
            edits=((
                "        ResolvedName::Imported(resolved) => {\n"
                "            let terminal = resolved\n"
                "                .rsplit_once('.')\n"
                "                .map_or(resolved.as_str(), |(_, terminal)| terminal)\n"
                "                .to_owned();\n"
                "            Some((resolved, terminal))\n"
                "        }",
                "        ResolvedName::Imported(_) => Some((path, terminal)),",
            ),),
            cases=("mutable_module_constant::pass::collections_abc_set_import_exempts_set_annotation",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts for Batch 3 collection rules",
            reason=(
                "No exemption: read-only collection ABCs (`Sequence`, `Mapping`, `AbstractSet`), "
                "non-nullable return types, non-collection return types (`str`, `int`, `Callable`), "
                "collections of nullable elements (`Sequence[int | None]`, `Vec<Option<i32>>`), "
                "and immutable module constant values (`tuple`, `frozenset`, `MappingProxyType`, `frozendict`)."
            ),
            cases=(
                "mutable_collection_return::pass::readonly_abstract_return_types",
                "mutable_collection_attribute::pass::readonly_abstract_attributes",
                "nullable_collection_return::pass::non_nullable_collection_returns",
                "nullable_collection_return::pass::non_collection_nullable_returns",
                "nullable_collection_return::pass::collection_of_nullable_elements",
                "nullable_collection_return::pass::callable_with_nullable_collection_parameter_or_return_not_flagged",
                "nullable_collection_return::pass::non_nullable_rust_collection_returns",
                "nullable_collection_return::pass::non_collection_rust_option_returns",
                "nullable_collection_return::pass::collection_of_option_elements_not_flagged",
                "mutable_module_constant::pass::immutable_collection_literals",
                "mutable_module_constant::pass::immutable_collection_constructors",
            ),
        ),
        Unmutated(
            label="constructs the return-type extractor never visits",
            reason=(
                "`nullable-collection-return` inspects only `signature.return_type_node` (Python) and "
                "`function.return_type` (Rust), never parameter annotations, class attributes, or struct fields."
            ),
            cases=(
                "nullable_collection_return::pass::nullable_parameter_not_flagged@python",
                "nullable_collection_return::pass::nullable_attribute_not_flagged",
                "nullable_collection_return::pass::nullable_parameter_not_flagged@rust",
                "nullable_collection_return::pass::nullable_struct_field_not_flagged",
            ),
        ),
    ),
)
