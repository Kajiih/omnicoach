"""Exemption mutations for Batch 2: Collection Types I."""

from __future__ import annotations

from .model import (
    ANNOTATIONS,
    CLASSES,
    COMMENTS,
    CONCRETE_COLLECTION_ATTRIBUTE,
    CONCRETE_COLLECTION_PARAMETER,
    CONTRACT,
    FUNCTIONS,
    INLINE_PUBLIC_ATTRIBUTE_ANNOTATION,
    PARAMETER_USAGE,
    SPECIFIC_COLLECTION_PARAMETER,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "concrete_collection_parameter",
        "concrete_collection_return",
        "concrete_collection_attribute",
        "specific_collection_parameter",
        "inline_public_attribute_annotation",
    ),
    mutations=(
        # --- Shared collection-type classification (annotations.rs) ---
        Mutation(
            label="read-only collection ABCs exempt from concrete-collection-*",
            path=ANNOTATIONS,
            edits=(
                (
                    "    } else if is_std_type_constructor(path, terminal, READ_ONLY_COLLECTION_ABCS) {\n"
                    "        CollectionKind::AbstractReadOnly",
                    "    } else if is_std_type_constructor(path, terminal, READ_ONLY_COLLECTION_ABCS) {\n"
                    "        CollectionKind::ConcreteMutable",
                ),
            ),
            cases=(
                "concrete_collection_parameter::pass::abstract_collections",
                "concrete_collection_return::pass::abstract_return_types",
                "concrete_collection_attribute::pass::abstract_read_only_attributes",
            ),
        ),
        Mutation(
            label="mutable collection ABCs exempt from concrete-collection-attribute",
            path=ANNOTATIONS,
            edits=(
                (
                    "    } else if is_std_type_constructor(path, terminal, MUTABLE_COLLECTION_ABCS) {\n"
                    "        CollectionKind::AbstractMutable",
                    "    } else if is_std_type_constructor(path, terminal, MUTABLE_COLLECTION_ABCS) {\n"
                    "        CollectionKind::ConcreteMutable",
                ),
            ),
            cases=("concrete_collection_attribute::pass::abstract_mutable_attributes",),
        ),
        Mutation(
            label="immutable collection builtins exempt from concrete-collection-*",
            path=ANNOTATIONS,
            edits=(
                (
                    "    } else if is_std_type_constructor(path, terminal, IMMUTABLE_COLLECTIONS) {\n"
                    "        CollectionKind::Immutable",
                    "    } else if is_std_type_constructor(path, terminal, IMMUTABLE_COLLECTIONS) {\n"
                    "        CollectionKind::ConcreteMutable",
                ),
            ),
            cases=(
                "concrete_collection_parameter::pass::immutable_concrete_builtins",
                "concrete_collection_return::pass::immutable_concrete_return_types",
                "concrete_collection_attribute::pass::immutable_concrete_attributes",
            ),
        ),
        Mutation(
            label="non-collection builtin types (bytes, str) exempt from concrete-collection-parameter",
            path=ANNOTATIONS,
            edits=(
                (
                    "    } else {\n        return None;\n    };",
                    "    } else {\n        CollectionKind::ConcreteMutable\n    };",
                ),
            ),
            cases=("concrete_collection_parameter::pass::non_collection_builtin_types",),
        ),
        Mutation(
            label="collections.abc.Set distinguished from typing.Set in concrete-collection-*",
            path=ANNOTATIONS,
            edits=(
                (
                    "        TYPING_SET_CONSTRUCTOR => {\n"
                    "            matches!(\n"
                    "                path,\n"
                    '                TYPING_SET_CONSTRUCTOR | "typing.Set" | "typing_extensions.Set"\n'
                    "            )\n"
                    "        }",
                    "        TYPING_SET_CONSTRUCTOR => is_std_type_constructor_prefix(path, terminal),",
                ),
            ),
            cases=(
                "concrete_collection_parameter::pass::unaliased_collections_abc_set_detected",
                "concrete_collection_return::pass::unaliased_collections_abc_set_detected",
                "concrete_collection_attribute::pass::unaliased_collections_abc_set_detected",
            ),
        ),
        Mutation(
            label="Annotated metadata arguments ignored by collect_collection_types",
            path=ANNOTATIONS,
            edits=(
                (
                    "            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_FIRST_ARG_WRAPPERS) {\n"
                    "                if let Some(first_arg) = type_args.first() {\n"
                    "                    collect_collection_types_expr(first_arg, file, depth, out);\n"
                    "                }\n"
                    "                return;\n"
                    "            }",
                    "            if is_std_type_constructor(&base_path, &base_terminal, TRANSPARENT_FIRST_ARG_WRAPPERS) {\n"
                    "                for arg in &type_args {\n"
                    "                    collect_collection_types_expr(arg, file, depth, out);\n"
                    "                }\n"
                    "                return;\n"
                    "            }",
                ),
            ),
            cases=("concrete_collection_parameter::pass::annotated_metadata_with_list_ignored",),
        ),
        Mutation(
            label="contravariant Callable parameter list ignored by collect_collection_types",
            path=ANNOTATIONS,
            edits=(
                (
                    "        Expr::Name(_) | Expr::Attribute(_) => push_collection_type(expr, file, out),",
                    "        Expr::Name(_) | Expr::Attribute(_) => push_collection_type(expr, file, out),\n"
                    "        Expr::List(list) => {\n"
                    "            for elt in &list.elts {\n"
                    "                collect_collection_types_expr(elt, file, depth, out);\n"
                    "            }\n"
                    "        }",
                ),
                (
                    '                    TYPE_MAPPING | "Callable" => {\n'
                    "                        if let Some(second_arg) = type_args.get(1) {\n"
                    "                            collect_collection_types_expr(second_arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }",
                    '                    TYPE_MAPPING | "Callable" => {\n'
                    "                        for arg in &type_args {\n"
                    "                            collect_collection_types_expr(arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }",
                ),
            ),
            cases=("concrete_collection_parameter::pass::callable_contravariant_parameter_ignored",),
        ),
        Mutation(
            label="invariant mutable outer containers not traversed covariantly",
            path=ANNOTATIONS,
            edits=(
                (
                    "                    \"AsyncGenerator\" => {\n"
                    "                        if let Some(yield_arg) = type_args.first() {\n"
                    "                            collect_collection_types_expr(yield_arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }\n"
                    "                    _ => {}",
                    "                    \"AsyncGenerator\" => {\n"
                    "                        if let Some(yield_arg) = type_args.first() {\n"
                    "                            collect_collection_types_expr(yield_arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }\n"
                    "                    _ => {\n"
                    "                        for arg in &type_args {\n"
                    "                            collect_collection_types_expr(arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }",
                ),
            ),
            cases=("concrete_collection_parameter::pass::invariant_mutable_outer_container_ignored",),
        ),
        Mutation(
            label="unknown custom generics not traversed covariantly",
            path=ANNOTATIONS,
            edits=(
                (
                    "            if depth == AnnotationTraversalDepth::CovariantPositions\n"
                    "                && is_std_type_constructor_prefix(&base_path, &base_terminal)\n"
                    "                && !is_concrete_collection_constructor(&base_path, &base_terminal)\n"
                    "            {",
                    "            if depth == AnnotationTraversalDepth::CovariantPositions {",
                ),
                (
                    "                    \"AsyncGenerator\" => {\n"
                    "                        if let Some(yield_arg) = type_args.first() {\n"
                    "                            collect_collection_types_expr(yield_arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }\n"
                    "                    _ => {}",
                    "                    \"AsyncGenerator\" => {\n"
                    "                        if let Some(yield_arg) = type_args.first() {\n"
                    "                            collect_collection_types_expr(yield_arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }\n"
                    "                    _ => {\n"
                    "                        for arg in &type_args {\n"
                    "                            collect_collection_types_expr(arg, file, depth, out);\n"
                    "                        }\n"
                    "                    }",
                ),
            ),
            cases=("concrete_collection_parameter::pass::unknown_custom_generic_ignored",),
        ),
        Mutation(
            label="local class definition shadowing typing name exempt from collection classification",
            path=ANNOTATIONS,
            edits=(("        ResolvedName::Local => None,", "        ResolvedName::Local => Some((path, terminal)),"),),
            cases=("concrete_collection_parameter::pass::local_class_shadows_typing_name",),
        ),
        # --- Signature, parameter, decorator, and class exemptions ---
        Mutation(
            label="variadic *args parameter exempt from concrete-collection-parameter",
            path=FUNCTIONS,
            edits=(("PythonParameterKind::VarPositional,", "PythonParameterKind::Positional,"),),
            cases=("concrete_collection_parameter::pass::variadic_args_exempt",),
        ),
        Mutation(
            label="variadic **kwargs parameter exempt from concrete-collection-parameter",
            path=FUNCTIONS,
            edits=(("PythonParameterKind::VarKeyword,", "PythonParameterKind::Positional,"),),
            cases=("concrete_collection_parameter::pass::variadic_kwargs_exempt",),
        ),
        Mutation(
            label="method receiver parameter exempt from concrete-collection-parameter",
            path=CONCRETE_COLLECTION_PARAMETER,
            edits=(
                (
                    "if param.is_variadic() || param.kind == PythonParameterKind::Receiver {",
                    "if param.is_variadic() {",
                ),
            ),
            cases=("concrete_collection_parameter::pass::receiver_parameter_exempt",),
        ),
        Mutation(
            label="@override decorator exempt from Python signature rules",
            path=FUNCTIONS,
            edits=((
                'OVERRIDE_DECORATOR | "overload"',
                '"overload"',
            ),),
            cases=(
                "concrete_collection_parameter::pass::override_exempt",
                "concrete_collection_return::pass::override_exempt",
                "specific_collection_parameter::pass::override_exempt",
            ),
        ),
        Mutation(
            label="@overload decorator exempt from Python signature rules",
            path=FUNCTIONS,
            edits=(('| "overload" |', "|"),),
            cases=(
                "concrete_collection_parameter::pass::overload_exempt",
                "concrete_collection_return::pass::overload_exempt",
                "specific_collection_parameter::pass::overload_exempt",
            ),
        ),
        Mutation(
            label="@abstractmethod decorator exempt from Python signature rules",
            path=FUNCTIONS,
            edits=(('| "abstractmethod" |', "|"),),
            cases=(
                "concrete_collection_parameter::pass::abstractmethod_exempt",
                "concrete_collection_return::pass::abstractmethod_exempt",
                "specific_collection_parameter::pass::abstractmethod_exempt",
            ),
        ),
        Mutation(
            label="@pytest.fixture decorator exempt from Python signature rules",
            path=FUNCTIONS,
            edits=(('| "fixture" |', "|"),),
            cases=("concrete_collection_parameter::pass::pytest_fixture_exempt",),
        ),
        Mutation(
            label="@singledispatch.register decorator exempt from Python signature rules",
            path=FUNCTIONS,
            edits=(('| "register" |', "|"),),
            cases=("concrete_collection_parameter::pass::singledispatch_register_exempt",),
        ),
        Mutation(
            label="@property.setter decorator exempt from Python signature rules",
            path=FUNCTIONS,
            edits=(('| "setter"', ""),),
            cases=("concrete_collection_parameter::pass::property_setter_exempt",),
        ),
        Mutation(
            label="Python data-model dunder methods exempt from signature rules",
            path=FUNCTIONS,
            edits=(
                (
                    '!matches!(func_name, "__init__" | "__new__" | "__call__")',
                    "false",
                ),
            ),
            cases=(
                "concrete_collection_parameter::pass::data_model_dunders_exempt",
                "concrete_collection_return::pass::data_model_dunder_exempt",
                "specific_collection_parameter::pass::data_model_dunder_exempt",
            ),
        ),
        Mutation(
            label="Protocol class methods and attributes exempt from collection rules",
            path=CLASSES,
            edits=(
                (
                    "matches!(terminal.as_str(), PROTOCOL_CLASS | ABC_CLASS)",
                    "matches!(terminal.as_str(), ABC_CLASS)",
                ),
            ),
            cases=(
                "concrete_collection_parameter::pass::protocol_class_exempt",
                "concrete_collection_return::pass::protocol_class_exempt",
                "concrete_collection_attribute::pass::protocol_class_exempt",
                "specific_collection_parameter::pass::protocol_class_exempt",
            ),
        ),
        Mutation(
            label="ABC class methods and attributes exempt from collection rules",
            path=CLASSES,
            edits=(
                (
                    "matches!(terminal.as_str(), PROTOCOL_CLASS | ABC_CLASS)",
                    "matches!(terminal.as_str(), PROTOCOL_CLASS)",
                ),
            ),
            cases=(
                "concrete_collection_parameter::pass::abc_class_exempt",
                "concrete_collection_attribute::pass::abc_class_exempt",
                "specific_collection_parameter::pass::abc_class_exempt",
            ),
        ),
        # --- Explanation comment exemptions (contract.rs & comments.rs) ---
        Mutation(
            label="RequireExplanation comment suppression on collection rules",
            path=CONTRACT,
            edits=(
                (
                    "if mode == EnforcementMode::RequireExplanation && !diagnostics.is_empty() {",
                    "if false && mode == EnforcementMode::RequireExplanation && !diagnostics.is_empty() {",
                ),
            ),
            cases=(
                "concrete_collection_return::pass::explained_by_preceding_header_comment",
                "concrete_collection_return::pass::explained_on_decorated_function_header",
                "concrete_collection_attribute::pass::explained_public_concrete_attribute",
                "specific_collection_parameter::pass::explained_sequence_parameter",
            ),
        ),
        Mutation(
            label="explanation comment above decorated function header",
            path=COMMENTS,
            edits=(
                (
                    "        let Some(header_lines) = statements::enclosing_statement_header_range(file, span) else {\n"
                    "            return false;\n"
                    "        };",
                    "        let _ = (file, span);\n"
                    "        if true {\n"
                    "            return false;\n"
                    "        }\n"
                    "        let header_lines = 0..=0;",
                ),
            ),
            cases=("concrete_collection_return::pass::explained_on_decorated_function_header",),
        ),
        # --- concrete-collection-attribute ---
        Mutation(
            label="private _-prefixed attributes exempt from concrete-collection-attribute",
            path=CONCRETE_COLLECTION_ATTRIBUTE,
            edits=(
                (
                    "if attribute.name.starts_with('_') || attribute.is_in_protocol_or_abc {",
                    "if attribute.is_in_protocol_or_abc {",
                ),
            ),
            cases=("concrete_collection_attribute::pass::private_attributes_exempt",),
        ),
        # --- specific-collection-parameter ---
        Mutation(
            label="len() call requires Collection capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(("BUILTIN_LEN => Some(UseRole::Sized),", "BUILTIN_LEN => Some(UseRole::Identity),"),),
            cases=("specific_collection_parameter::pass::collection_parameter_with_len",),
        ),
        Mutation(
            label="`in` membership test requires Collection capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            let role = if has_in_operator && idx + 1 == comp.operands.len() {\n"
                    "                UseRole::Sized",
                    "            let role = if has_in_operator && idx + 1 == comp.operands.len() {\n"
                    "                UseRole::Identity",
                ),
            ),
            cases=("specific_collection_parameter::pass::collection_parameter_with_membership",),
        ),
        Mutation(
            label="truthiness check requires Collection capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "UseRole::Truthiness | UseRole::Sized => usage.needs_collection = true,",
                    "UseRole::Sized => usage.needs_collection = true,\n            UseRole::Truthiness => {}",
                ),
            ),
            cases=("specific_collection_parameter::pass::collection_parameter_with_truthiness_check",),
        ),
        Mutation(
            label="multiple iteration passes require Collection capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "        } else if self.needs_collection || self.iteration_count > 1 {\n"
                    "            ParameterCollectionCapability::Collection\n"
                    "        } else if self.iteration_count == 1 {",
                    "        } else if self.needs_collection {\n"
                    "            ParameterCollectionCapability::Collection\n"
                    "        } else if self.iteration_count >= 1 {",
                ),
            ),
            cases=("specific_collection_parameter::pass::collection_parameter_with_multiple_passes",),
        ),
        Mutation(
            label="reversed() call requires Sequence capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "fn is_single_pass_iterable_builtin(name: &str) -> bool {\n"
                    "    ITERATING_COLLECTION_BUILTINS.contains(&name)",
                    "fn is_single_pass_iterable_builtin(name: &str) -> bool {\n"
                    '    name == "reversed" || ITERATING_COLLECTION_BUILTINS.contains(&name)',
                ),
            ),
            cases=("specific_collection_parameter::pass::reversed_requires_sequence",),
        ),
        Mutation(
            label=".count() method call requires Sequence capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            Expr::Attribute(attr) if READONLY_COLLECTION_METHODS.contains(&attr.attr.as_str()) => {\n"
                    "                self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "            }",
                    '            Expr::Attribute(attr) if attr.attr.as_str() == "count" => {\n'
                    "                self.visit_expr_as(&attr.value, UseRole::Iterated);\n"
                    "            }\n"
                    "            Expr::Attribute(attr) if READONLY_COLLECTION_METHODS.contains(&attr.attr.as_str()) => {\n"
                    "                self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "            }",
                ),
            ),
            cases=("specific_collection_parameter::pass::count_method_requires_sequence",),
        ),
        Mutation(
            label=".index() method call requires Sequence capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            Expr::Attribute(attr) if READONLY_COLLECTION_METHODS.contains(&attr.attr.as_str()) => {\n"
                    "                self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "            }",
                    '            Expr::Attribute(attr) if attr.attr.as_str() == "index" => {\n'
                    "                self.visit_expr_as(&attr.value, UseRole::Iterated);\n"
                    "            }\n"
                    "            Expr::Attribute(attr) if READONLY_COLLECTION_METHODS.contains(&attr.attr.as_str()) => {\n"
                    "                self.visit_expr_as(&attr.value, UseRole::Read);\n"
                    "            }",
                ),
            ),
            cases=("specific_collection_parameter::pass::index_method_requires_sequence",),
        ),
        Mutation(
            label="subscript indexing requires Sequence capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            Expr::Subscript(sub) if matches!(sub.ctx, ruff_python_ast::ExprContext::Load) => {\n"
                    "                self.visit_expr_as(&sub.value, UseRole::Read);",
                    "            Expr::Subscript(sub) if matches!(sub.ctx, ruff_python_ast::ExprContext::Load) => {\n"
                    "                self.visit_expr_as(&sub.value, UseRole::Iterated);",
                ),
            ),
            cases=("specific_collection_parameter::pass::indexing_requires_sequence",),
        ),
        Mutation(
            label="escaping parameter usages require Sequence capability in specific-collection-parameter",
            path=PARAMETER_USAGE,
            edits=(
                (
                    "            UseRole::Escape => {\n"
                    "                usage.mutated_or_escaping = true;\n"
                    "                usage.needs_sequence = true;\n"
                    "            }",
                    "            UseRole::Escape => {\n"
                    "                usage.mutated_or_escaping = true;\n"
                    "                usage.iteration_count += 1;\n"
                    "            }",
                ),
            ),
            cases=(
                "specific_collection_parameter::pass::sequence_match_pattern_requires_sequence",
                "specific_collection_parameter::pass::passed_to_method_exempt",
                "specific_collection_parameter::pass::passed_to_unknown_function_exempt",
                "specific_collection_parameter::pass::bool_op_operand_escapes",
            ),
        ),
        Mutation(
            label="unused parameter exempt from specific-collection-parameter",
            path=SPECIFIC_COLLECTION_PARAMETER,
            edits=(
                (
                    "ParameterCollectionCapability::Unused | ParameterCollectionCapability::Sequence => {",
                    "ParameterCollectionCapability::Sequence => {",
                ),
                (
                    'ParameterCollectionCapability::Iterable => "collections.abc.Iterable",',
                    'ParameterCollectionCapability::Unused | ParameterCollectionCapability::Iterable => "collections.abc.Iterable",',
                ),
            ),
            cases=("specific_collection_parameter::pass::unused_parameter_not_flagged",),
        ),
        # --- inline-public-attribute-annotation ---
        Mutation(
            label="private _-prefixed attributes exempt from inline-public-attribute-annotation",
            path=INLINE_PUBLIC_ATTRIBUTE_ANNOTATION,
            edits=(
                (
                    "!attribute.name.starts_with('_')",
                    "true",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::inline_private_attribute_annotations_exempt",),
        ),
        Mutation(
            label="bare Final inline annotation exempt from inline-public-attribute-annotation",
            path=INLINE_PUBLIC_ATTRIBUTE_ANNOTATION,
            edits=(("&& !attribute.is_bare_final()", ""),),
            cases=(
                "inline_public_attribute_annotation::pass::bare_final_inline_annotation_exempt",
                "inline_public_attribute_annotation::pass::annotated_wrapped_bare_final_exempt",
            ),
        ),
        Mutation(
            label="Annotated[Final, ...] unwrapped by is_bare_final_annotation_expr",
            path=ANNOTATIONS,
            edits=(
                (
                    "pub(super) fn is_bare_final_annotation_expr(type_expr: &Expr, file: &ParsedFile) -> bool {\n"
                    "    let unwrapped = unwrap_annotated_expr(type_expr, file);",
                    "pub(super) fn is_bare_final_annotation_expr(type_expr: &Expr, file: &ParsedFile) -> bool {\n"
                    "    let unwrapped = type_expr;",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::annotated_wrapped_bare_final_exempt",),
        ),
        Mutation(
            label="@staticmethod methods exempt from inline-public-attribute-annotation",
            path=FUNCTIONS,
            edits=(
                (
                    '        decorator.terminal_name == "staticmethod"\n'
                    '            || (!allow_classmethod_cls && decorator.terminal_name == "classmethod")',
                    '        !allow_classmethod_cls && decorator.terminal_name == "classmethod"',
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::staticmethod_exempt",),
        ),
        Mutation(
            label="@classmethod methods exempt from inline-public-attribute-annotation",
            path=FUNCTIONS,
            edits=(
                (
                    '        decorator.terminal_name == "staticmethod"\n'
                    '            || (!allow_classmethod_cls && decorator.terminal_name == "classmethod")',
                    '        decorator.terminal_name == "staticmethod"',
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::classmethod_exempt",),
        ),
        Mutation(
            label="methods whose first parameter is cls exempt from inline-public-attribute-annotation",
            path=FUNCTIONS,
            edits=(
                (
                    "    if !allow_classmethod_cls && first.name != SELF_PARAMETER {\n"
                    "        return None;\n"
                    "    }",
                    "",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::cls_first_parameter_method_exempt",),
        ),
        Mutation(
            label="methods whose first parameter is not a positional receiver exempt from inline-public-attribute-annotation",
            path=FUNCTIONS,
            edits=(
                (
                    "    if first.kind != PythonParameterKind::Receiver {\n"
                    "        return None;\n"
                    "    }",
                    "",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::keyword_only_self_parameter_method_exempt",),
        ),
        Mutation(
            label="annotated attributes on objects other than self exempt from inline-public-attribute-annotation",
            path=CLASSES,
            edits=(("&& object.id == SELF_RECEIVER", ""),),
            cases=("inline_public_attribute_annotation::pass::other_object_attribute_annotation_exempt",),
        ),
        Mutation(
            label="nested functions inside methods not entered by collect_self_annotated_assignments",
            path=CLASSES,
            edits=(
                (
                    "Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,",
                    "Stmt::ClassDef(_) => return,",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::nested_function_inside_method_not_entered",),
        ),
        Mutation(
            label="@dataclass classes exempt from inline-public-attribute-annotation",
            path=CLASSES,
            edits=(
                (
                    "                DATACLASS_DECORATOR\n"
                    "                    | QUALIFIED_DATACLASS_DECORATOR\n"
                    "                    |",
                    "               ",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::dataclass_post_init_inline_annotation_exempt",),
        ),
        Mutation(
            label="attrs @define classes exempt from inline-public-attribute-annotation",
            path=CLASSES,
            edits=(('| "define"', ""),),
            cases=("inline_public_attribute_annotation::pass::attrs_define_inline_annotation_exempt",),
        ),
        Mutation(
            label="Pydantic BaseModel subclasses exempt from inline-public-attribute-annotation",
            path=CLASSES,
            edits=(
                (
                    "        || base_class_terminals(class_def, &file.source)\n"
                    "            .iter()\n"
                    '            .any(|terminal| terminal == "BaseModel")',
                    "",
                ),
            ),
            cases=("inline_public_attribute_annotation::pass::pydantic_base_model_inline_annotation_exempt",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts",
            reason="No exemption: attributes declared in the class body or assigned without inline type annotations.",
            cases=(
                "inline_public_attribute_annotation::pass::class_body_attribute_annotations",
                "inline_public_attribute_annotation::pass::unannotated_public_attribute_assignments_exempt",
            ),
        ),
        Unmutated(
            label="constructs the extractor never visits",
            reason=(
                "String literals are not parsed as type expressions; `collect_class_attributes` and "
                "`collect_instance_attribute_annotations` only visit `Stmt::ClassDef` bodies; and "
                "`collect_self_annotated_assignments` only matches `self.<attr>: <type>` (`Expr::Attribute` "
                "with an `Expr::Name` receiver)."
            ),
            cases=(
                "concrete_collection_parameter::pass::known_gap_string_annotation_not_parsed",
                "concrete_collection_attribute::pass::module_variable_ignored",
                "concrete_collection_attribute::pass::function_local_variable_ignored",
                "inline_public_attribute_annotation::pass::chained_attribute_annotation_exempt",
                "inline_public_attribute_annotation::pass::local_variable_annotations_in_method_exempt",
                "inline_public_attribute_annotation::pass::module_level_function_with_self_param_exempt",
            ),
        ),
    ),
)
