"""Exemption mutations for Batch 4: Names (`abbreviated-name`, `single-letter-name`,
`type-suffixed-name`, `primitive-duration`)."""

from __future__ import annotations

from .model import (
    ABBREVIATED_NAME,
    BINDINGS,
    RUST,
    SCOPES,
    SINGLE_LETTER_NAME,
    TYPE_SUFFIXED_NAME,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "abbreviated_name",
        "single_letter_name",
        "type_suffixed_name",
        "primitive_duration",
    ),
    mutations=(
        Mutation(
            label="whole-word segment splitting in abbreviated-name",
            path=ABBREVIATED_NAME,
            edits=(
                (
                    "        for segment in split_segments(&name) {\n"
                    "            if banned.contains(&segment) {",
                    "        let lower = name.to_lowercase();\n"
                    "        for segment in banned {\n"
                    "            if lower.contains(segment.as_str()) {",
                ),
            ),
            cases=(
                "abbreviated_name::pass::substring_containing_banned_token_allowed@python",
                "abbreviated_name::pass::substring_containing_banned_token_allowed@rust",
            ),
        ),
        Mutation(
            label="Python import statement classified as BindingKind::Import",
            path=SCOPES,
            edits=(
                (
                    "            Stmt::Import(import_statement) => {\n"
                    "                push_bindings(&mut self.bindings, BindingKind::Import, |out| {",
                    "            Stmt::Import(import_statement) => {\n"
                    "                push_bindings(&mut self.bindings, BindingKind::Value, |out| {",
                ),
            ),
            cases=(
                "abbreviated_name::pass::unaliased_import_statement_exempt",
                "abbreviated_name::pass::aliased_import_exempt@python",
                "single_letter_name::pass::aliased_import_exempt",
                "type_suffixed_name::pass::unaliased_import_exempt@python",
                "primitive_duration::pass::unaliased_import_exempt",
            ),
        ),
        Mutation(
            label="Python from-import statement classified as BindingKind::Import",
            path=SCOPES,
            edits=(
                (
                    "            Stmt::ImportFrom(import_from) => {\n"
                    "                push_bindings(&mut self.bindings, BindingKind::Import, |out| {",
                    "            Stmt::ImportFrom(import_from) => {\n"
                    "                push_bindings(&mut self.bindings, BindingKind::Value, |out| {",
                ),
            ),
            cases=(
                "abbreviated_name::pass::unaliased_from_import_exempt",
                "type_suffixed_name::pass::aliased_import_exempt@python",
                "primitive_duration::pass::aliased_import_exempt",
            ),
        ),
        Mutation(
            label="Rust use statement classified as BindingKind::Import",
            path=RUST,
            edits=(
                (
                    "    if let Some(use_item) = ast::Use::cast(node.clone()) {\n"
                    "        if let Some(use_tree) = use_item.use_tree() {\n"
                    "            push_bindings(bindings, BindingKind::Import, |out| {",
                    "    if let Some(use_item) = ast::Use::cast(node.clone()) {\n"
                    "        if let Some(use_tree) = use_item.use_tree() {\n"
                    "            push_bindings(bindings, BindingKind::Value, |out| {",
                ),
            ),
            cases=(
                "abbreviated_name::pass::unaliased_imports_exempt",
                "abbreviated_name::pass::aliased_import_exempt@rust",
                "type_suffixed_name::pass::unaliased_import_exempt@rust",
                "type_suffixed_name::pass::aliased_import_exempt@rust",
                "primitive_duration::pass::import_alias_exempt",
            ),
        ),
        Mutation(
            label="Python @override method classified as BindingKind::ContractMember",
            path=SCOPES,
            edits=(
                (
                    "                let kind = if has_override_decorator(&func_def.decorator_list, file) {\n"
                    "                    BindingKind::ContractMember\n"
                    "                } else {\n"
                    "                    BindingKind::StructuralDefinition\n"
                    "                };",
                    "                let _ = has_override_decorator(&func_def.decorator_list, file);\n"
                    "                let kind = BindingKind::StructuralDefinition;",
                ),
            ),
            cases=("abbreviated_name::pass::override_method_contract_exempt",),
        ),
        Mutation(
            label="Python attribute writes outside __init__ not extracted as attribute declarations",
            path=SCOPES,
            edits=(
                (
                    "    let Some(init) = direct_function_definitions(&class_def.body)\n"
                    "        .into_iter()\n"
                    '        .find(|func_def| func_def.name.id == "__init__")',
                    "    let Some(init) = direct_function_definitions(&class_def.body)\n"
                    "        .into_iter()\n"
                    "        .next()",
                ),
            ),
            cases=(
                "abbreviated_name::pass::attribute_writes_outside_declarations_not_checked",
                "single_letter_name::pass::attribute_writes_outside_declarations_not_checked",
                "type_suffixed_name::pass::attribute_writes_outside_declarations_not_checked",
                "primitive_duration::pass::attribute_writes_outside_declarations_not_checked",
            ),
        ),
        Mutation(
            label="Rust trait impl associated type classified as BindingKind::ContractMember",
            path=RUST,
            edits=(
                (
                    "        SyntaxKind::FN | SyntaxKind::TYPE_ALIAS | SyntaxKind::CONST",
                    "        SyntaxKind::FN | SyntaxKind::CONST",
                ),
            ),
            cases=("abbreviated_name::pass::trait_impl_associated_type_exempt",),
        ),
        Mutation(
            label="Rust trait impl method classified as BindingKind::ContractMember",
            path=RUST,
            edits=(
                (
                    "        SyntaxKind::FN | SyntaxKind::TYPE_ALIAS | SyntaxKind::CONST",
                    "        SyntaxKind::TYPE_ALIAS | SyntaxKind::CONST",
                ),
            ),
            cases=("abbreviated_name::pass::trait_impl_method_exempt",),
        ),
        Mutation(
            label="Rust trait impl const classified as BindingKind::ContractMember",
            path=RUST,
            edits=(
                (
                    "        SyntaxKind::FN | SyntaxKind::TYPE_ALIAS | SyntaxKind::CONST",
                    "        SyntaxKind::FN | SyntaxKind::TYPE_ALIAS",
                ),
            ),
            cases=("type_suffixed_name::pass::trait_impl_const_exempt",),
        ),
        Mutation(
            label="allowed single-letter names exempt from single-letter-name",
            path=SINGLE_LETTER_NAME,
            edits=(
                (
                    "        if name.chars().count() == 1 && !allowed.contains(&*name) {",
                    "        if name.chars().count() == 1 {",
                ),
            ),
            cases=(
                "single_letter_name::pass::allowed_names",
                "single_letter_name::pass::parameter_allowed",
                "single_letter_name::pass::allowed_bindings",
                "single_letter_name::pass::rust_char_binding_allowed",
                "single_letter_name::pass::closure_parameter_allowed",
                "single_letter_name::pass::fn_parameter_allowed",
            ),
        ),
        Mutation(
            label="Rust c extended into allowed single-letter names",
            path=SINGLE_LETTER_NAME,
            edits=(
                (
                    '        extend: &[(Language::Rust, &["c"])],',
                    "        extend: &[],",
                ),
            ),
            cases=("single_letter_name::pass::rust_char_binding_allowed",),
        ),
        Mutation(
            label="Python wildcard _ ignored by extract_from_expr_target",
            path=SCOPES,
            edits=(
                (
                    '        Expr::Name(name) => {\n            if name.id.as_str() != "_" {',
                    "        Expr::Name(name) => {\n            if true {",
                ),
            ),
            cases=("single_letter_name::pass::wildcard_ignored@python",),
        ),
        Mutation(
            label="Python class definition classified as BindingKind::StructuralDefinition",
            path=SCOPES,
            edits=(
                (
                    "                self.bindings.push(Binding {\n"
                    "                    node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),\n"
                    "                    kind: BindingKind::StructuralDefinition,\n"
                    "                });",
                    "                self.bindings.push(Binding {\n"
                    "                    node: AstNode::from_span(file, span_from_ruff_range(class_def.name.range)),\n"
                    "                    kind: BindingKind::Value,\n"
                    "                });",
                ),
            ),
            cases=(
                "type_suffixed_name::pass::class_exempt",
                "primitive_duration::pass::class_exempt",
            ),
        ),
        Mutation(
            label="Python function definition classified as BindingKind::StructuralDefinition",
            path=SCOPES,
            edits=(
                (
                    "                let kind = if has_override_decorator(&func_def.decorator_list, file) {\n"
                    "                    BindingKind::ContractMember\n"
                    "                } else {\n"
                    "                    BindingKind::StructuralDefinition\n"
                    "                };",
                    "                let kind = if has_override_decorator(&func_def.decorator_list, file) {\n"
                    "                    BindingKind::ContractMember\n"
                    "                } else {\n"
                    "                    BindingKind::Value\n"
                    "                };",
                ),
            ),
            cases=(
                "type_suffixed_name::pass::method_exempt",
                "primitive_duration::pass::method_exempt",
            ),
        ),
        Mutation(
            label="Rust struct definition classified as BindingKind::StructuralDefinition",
            path=RUST,
            edits=(
                (
                    "        SyntaxKind::STRUCT\n"
                    "        | SyntaxKind::ENUM\n"
                    "        | SyntaxKind::TRAIT\n"
                    "        | SyntaxKind::TYPE_ALIAS\n"
                    "        | SyntaxKind::FN => BindingKind::StructuralDefinition,",
                    "        SyntaxKind::ENUM\n"
                    "        | SyntaxKind::TRAIT\n"
                    "        | SyntaxKind::TYPE_ALIAS\n"
                    "        | SyntaxKind::FN => BindingKind::StructuralDefinition,",
                ),
            ),
            cases=(
                "type_suffixed_name::pass::struct_exempt",
                "primitive_duration::pass::struct_exempt",
            ),
        ),
        Mutation(
            label="Rust fn definition classified as BindingKind::StructuralDefinition",
            path=RUST,
            edits=(
                (
                    "        SyntaxKind::STRUCT\n"
                    "        | SyntaxKind::ENUM\n"
                    "        | SyntaxKind::TRAIT\n"
                    "        | SyntaxKind::TYPE_ALIAS\n"
                    "        | SyntaxKind::FN => BindingKind::StructuralDefinition,",
                    "        SyntaxKind::STRUCT\n"
                    "        | SyntaxKind::ENUM\n"
                    "        | SyntaxKind::TRAIT\n"
                    "        | SyntaxKind::TYPE_ALIAS => BindingKind::StructuralDefinition,",
                ),
            ),
            cases=(
                "type_suffixed_name::pass::fn_exempt",
                "primitive_duration::pass::fn_exempt",
            ),
        ),
        Mutation(
            label="exact suffix without prefix exempt from find_suffixed_bindings",
            path=BINDINGS,
            edits=(
                (
                    "            if name.len() > suffix_lower.len() && name_lower.ends_with(suffix_lower.as_str()) {",
                    "            if name.len() >= suffix_lower.len() && name_lower.ends_with(suffix_lower.as_str()) {",
                ),
            ),
            cases=(
                "type_suffixed_name::pass::exact_suffix_exempt@python",
                "type_suffixed_name::pass::exact_suffix_exempt@rust",
                "primitive_duration::pass::exact_suffix_without_prefix_exempt@python",
                "primitive_duration::pass::exact_suffix_without_prefix_exempt@rust",
            ),
        ),
        Mutation(
            label="is_ predicate prefix exempt from type-suffixed-name",
            path=TYPE_SUFFIXED_NAME,
            edits=(
                (
                    'const PREDICATE_PREFIXES: [&str; 2] = ["is_", "has_"];',
                    'const PREDICATE_PREFIXES: [&str; 1] = ["has_"];',
                ),
            ),
            cases=(
                "type_suffixed_name::pass::is_predicate_name_exempt@python",
                "type_suffixed_name::pass::is_predicate_name_exempt@rust",
            ),
        ),
        Mutation(
            label="has_ predicate prefix exempt from type-suffixed-name",
            path=TYPE_SUFFIXED_NAME,
            edits=(
                (
                    'const PREDICATE_PREFIXES: [&str; 2] = ["is_", "has_"];',
                    'const PREDICATE_PREFIXES: [&str; 1] = ["is_"];',
                ),
            ),
            cases=(
                "type_suffixed_name::pass::has_predicate_name_exempt@python",
                "type_suffixed_name::pass::has_predicate_name_exempt@rust",
            ),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts for Batch 4 naming rules",
            reason=(
                "No exemption: full domain words without abbreviations, multi-character variable "
                "names, and variables without type or time-unit suffixes."
            ),
            cases=(
                "abbreviated_name::pass::full_domain_words_allowed@python",
                "abbreviated_name::pass::full_domain_words_allowed@rust",
                "single_letter_name::pass::descriptive_variable_names@python",
                "single_letter_name::pass::descriptive_variable_names@rust",
                "type_suffixed_name::pass::unsuffixed_variable@python",
                "type_suffixed_name::pass::unsuffixed_variable@rust",
                "primitive_duration::pass::unsuffixed_duration_variables@python",
                "primitive_duration::pass::unsuffixed_duration_variables@rust",
            ),
        ),
        Unmutated(
            label="constructs the Rust binding extractor never visits",
            reason=(
                "`rust::collect_bindings` extracts `ast::IdentPat` bindings and item declarations; "
                "it never visits `ast::WildcardPat` (`let _ = 1;`) or expression `NameRef`s (`q + 1`)."
            ),
            cases=(
                "single_letter_name::pass::wildcard_ignored@rust",
                "single_letter_name::pass::single_letter_reference_not_flagged",
            ),
        ),
    ),
)
