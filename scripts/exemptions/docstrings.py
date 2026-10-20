"""Exemption mutations for Batch D: docstring rules."""

from __future__ import annotations

from .model import (
    CLASSES,
    DOCSTRINGS,
    FUNCTIONS,
    UNEXPLAINED_OPTIONAL_RETURN,
    VERB_LED_PROPERTY_DOCSTRING,
    Cluster,
    Mutation,
    Unmutated,
)

VERB_LED_RETURNS_SECTION = "src/code_lint/rules/verb_led_returns_section.rs"
UNDOCUMENTED_PUBLIC_ATTRIBUTE = "src/code_lint/rules/undocumented_public_attribute.rs"
ANNOTATED_TYPE_IN_DOCSTRING = "src/code_lint/rules/annotated_type_in_docstring.rs"
UNEXPLAINED_OPTIONAL_ARGUMENT = "src/code_lint/rules/unexplained_optional_argument.rs"
DEFERRAL_DOCSTRING = "src/code_lint/rules/deferral_docstring.rs"
COPIED_BASE_DOCSTRING = "src/code_lint/rules/copied_base_docstring.rs"
PARTIAL_OVERRIDE_DOCSTRING = "src/code_lint/rules/partial_override_docstring.rs"
UNDOCUMENTED_NONTRIVIAL_FUNCTION = (
    "src/code_lint/rules/undocumented_nontrivial_function.rs"
)
UNEXPLAINED_PARAMETER = "src/code_lint/rules/unexplained_parameter.rs"

CHECK_EVERY_FUNCTION = (
    (
        "        for method in class.methods() {",
        "        for method in ast::python::extract_function_signatures(file) {",
    ),
)
DROP_OVERRIDE_CHECK = (
    ("            if !method.has_override_decorator() {", "            if false {"),
)

CLUSTER = Cluster(
    rules=(
        "verb_led_returns_section",
        "verb_led_property_docstring",
        "undocumented_public_attribute",
        "annotated_type_in_docstring",
        "unexplained_optional_argument",
        "unexplained_optional_return",
        "deferral_docstring",
        "copied_base_docstring",
        "partial_override_docstring",
        "undocumented_nontrivial_function",
        "unexplained_parameter",
    ),
    mutations=(
        Mutation(
            label="bare Return/Yield/Get + noun read as a noun phrase (verb-led-returns-section, verb-led-property-docstring)",
            path=DOCSTRINGS,
            edits=(
                (
                    "    let is_noun_phrase =\n"
                    "        noun_stems.contains(&lowercase.as_str()) && noun_followers.contains(&next_word.as_str());",
                    "    let _ = (noun_stems, noun_followers, next_word);\n    let is_noun_phrase = false;",
                ),
            ),
            cases=(
                "verb_led_returns_section::pass::return_noun_phrase_exempt",
                "verb_led_property_docstring::pass::return_noun_phrase_exempt",
            ),
        ),
        Mutation(
            label="whole-word match of the opening verb (verb-led-returns-section, verb-led-property-docstring)",
            path=DOCSTRINGS,
            edits=(
                (
                    "    if !verbs.contains(&lowercase.as_str()) {",
                    "    if !verbs.iter().any(|verb| lowercase.starts_with(verb)) {",
                ),
            ),
            cases=(
                "verb_led_returns_section::pass::longer_word_starting_with_verb_exempt",
                "verb_led_property_docstring::pass::longer_word_starting_with_verb_exempt",
            ),
        ),
        Mutation(
            label="only the first body line is read (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "let Some(opening_line) = section.body.first() else {",
                    "let Some(opening_line) = section.body.last() else {",
                ),
            ),
            cases=("verb_led_returns_section::pass::only_first_body_line_checked",),
        ),
        Mutation(
            label="only the first word of the opening line is read (verb-led-returns-section, verb-led-property-docstring)",
            path=DOCSTRINGS,
            edits=(
                (
                    "    let word = leading_word(text);",
                    "    let word = text\n"
                    "        .split_whitespace()\n"
                    "        .map(leading_word)\n"
                    "        .find(|candidate| verbs.contains(&candidate.to_ascii_lowercase().as_str()))\n"
                    "        .unwrap_or_default();",
                ),
            ),
            cases=(
                "verb_led_returns_section::pass::only_first_word_checked",
                "verb_led_property_docstring::pass::only_first_word_checked",
            ),
        ),
        Mutation(
            label="only Returns/Yields sections are read (verb-led-returns-section)",
            path=VERB_LED_RETURNS_SECTION,
            edits=(
                (
                    "                DocstringSectionKind::Returns | DocstringSectionKind::Yields\n",
                    "                _\n",
                ),
            ),
            cases=("verb_led_returns_section::pass::other_sections_not_checked",),
        ),
        Mutation(
            label="only @property getters are checked (verb-led-property-docstring)",
            path=VERB_LED_PROPERTY_DOCSTRING,
            edits=(
                ("        if !signature.is_property_getter() {", "        if false {"),
            ),
            cases=(
                "verb_led_property_docstring::pass::regular_method_with_returns_exempt",
                "verb_led_property_docstring::pass::property_accessors_exempt",
            ),
        ),
        Mutation(
            label="property setters and deleters are not property getters (verb-led-property-docstring)",
            path=FUNCTIONS,
            edits=(
                (
                    "            .any(|decorator| decorator.matches_any(PROPERTY_DECORATOR_PATHS))",
                    "            .any(|_| true)",
                ),
            ),
            cases=("verb_led_property_docstring::pass::property_accessors_exempt",),
        ),
        Mutation(
            label="summary-only class docstring (undocumented-public-attribute)",
            path=UNDOCUMENTED_PUBLIC_ATTRIBUTE,
            edits=(
                (
                    "docstring.non_blank_line_count() <= 1 ||",
                    "docstring.non_blank_line_count() < 1 ||",
                ),
            ),
            cases=(
                "undocumented_public_attribute::pass::summary_only_docstring_exempt",
            ),
        ),
        Mutation(
            label="`_`-prefixed names (undocumented-public-attribute)",
            path=UNDOCUMENTED_PUBLIC_ATTRIBUTE,
            edits=(("name.starts_with('_')", "name.starts_with('#')"),),
            cases=(
                "undocumented_public_attribute::pass::underscore_prefixed_names_exempt",
            ),
        ),
        Mutation(
            label="names bound by `def` in the class body (undocumented-public-attribute)",
            path=UNDOCUMENTED_PUBLIC_ATTRIBUTE,
            edits=(
                ("|| class.defines_function(name)", '|| class.defines_function("")'),
            ),
            cases=("undocumented_public_attribute::pass::names_defined_by_def_exempt",),
        ),
        Mutation(
            label="Enum classes (undocumented-public-attribute)",
            path=UNDOCUMENTED_PUBLIC_ATTRIBUTE,
            edits=(("|| class.is_enum() {", "|| class.name.is_empty() {"),),
            cases=("undocumented_public_attribute::pass::enum_members_exempt",),
        ),
        Mutation(
            label="names in `allowed` (undocumented-public-attribute)",
            path=UNDOCUMENTED_PUBLIC_ATTRIBUTE,
            edits=(("|| allowed.contains(name);", '|| allowed.contains("");'),),
            cases=("undocumented_public_attribute::pass::allowed_names_exempt",),
        ),
        Mutation(
            label="`dataclasses.InitVar` pseudo-fields (undocumented-public-attribute)",
            path=UNDOCUMENTED_PUBLIC_ATTRIBUTE,
            edits=(
                (
                    "|| definition.is_init_var\n",
                    "|| (definition.is_init_var && name.is_empty())\n",
                ),
            ),
            cases=(
                "undocumented_public_attribute::pass::init_var_pseudo_field_exempt",
            ),
        ),
        Mutation(
            label="initializer nested scopes not entered (PythonClassInfo::attribute_definitions)",
            path=CLASSES,
            edits=(
                (
                    "                Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,\n"
                    "                Stmt::Assign(assign) => {\n"
                    "                    self.out\n",
                    "                Stmt::Assign(assign) => {\n"
                    "                    self.out\n",
                ),
            ),
            cases=(
                "undocumented_public_attribute::pass::nested_scope_assignments_exempt",
            ),
        ),
        Mutation(
            label="only initializer methods read (PythonClassInfo::attribute_definitions)",
            path=CLASSES,
            edits=(
                (
                    "if INITIALIZER_METHODS.contains(&function_def.name.id.as_str())",
                    "if !INITIALIZER_METHODS.is_empty()",
                ),
            ),
            cases=(
                "undocumented_public_attribute::pass::other_method_assignments_exempt",
            ),
        ),
        Mutation(
            label="unannotated parameters (annotated-type-in-docstring)",
            path=ANNOTATED_TYPE_IN_DOCSTRING,
            edits=(
                (
                    ".filter(|parameter| parameter.type_node.is_some())",
                    ".filter(|parameter| !parameter.name.is_empty())",
                ),
            ),
            cases=("annotated_type_in_docstring::pass::unannotated_parameter_exempt",),
        ),
        Mutation(
            label="entries naming no parameter (annotated-type-in-docstring)",
            path=ANNOTATED_TYPE_IN_DOCSTRING,
            edits=(
                (
                    ".filter(|entry| annotated.contains(entry.bare_name()))",
                    ".filter(|entry| !annotated.is_empty() || entry.name.is_empty())",
                ),
            ),
            cases=(
                "annotated_type_in_docstring::pass::entry_naming_no_parameter_exempt",
            ),
        ),
        Mutation(
            label="`(optional)` alone has no type (annotated-type-in-docstring)",
            path=DOCSTRINGS,
            edits=(
                (
                    "(lowercase != OPTIONAL_MARKER && !is_default_note)",
                    "(!lowercase.is_empty() && !is_default_note)",
                ),
            ),
            cases=(
                "annotated_type_in_docstring::pass::parenthetical_without_type_exempt",
            ),
        ),
        Mutation(
            label="`(default …)` alone has no type (annotated-type-in-docstring)",
            path=DOCSTRINGS,
            edits=(
                (
                    "(lowercase != OPTIONAL_MARKER && !is_default_note)",
                    "(lowercase != OPTIONAL_MARKER && (!is_default_note || !lowercase.is_empty()))",
                ),
            ),
            cases=(
                "annotated_type_in_docstring::pass::parenthetical_without_type_exempt",
            ),
        ),
        Mutation(
            label="unannotated attributes (annotated-type-in-docstring)",
            path=ANNOTATED_TYPE_IN_DOCSTRING,
            edits=(
                (
                    ".filter(|definition| definition.annotation.is_some())",
                    ".filter(|definition| !definition.name.is_empty())",
                ),
            ),
            cases=("annotated_type_in_docstring::pass::unannotated_attribute_exempt",),
        ),
        Mutation(
            label="Returns/Yields prefix must equal the annotation (annotated-type-in-docstring)",
            path=ANNOTATED_TYPE_IN_DOCSTRING,
            edits=(
                (
                    "without_whitespace(annotation) == prefix_text",
                    "!without_whitespace(annotation).is_empty() || prefix_text.is_empty()",
                ),
            ),
            cases=(
                "annotated_type_in_docstring::pass::differing_returns_prefix_exempt",
            ),
        ),
        Mutation(
            label="Returns/Yields without a return annotation (annotated-type-in-docstring)",
            path=ANNOTATED_TYPE_IN_DOCSTRING,
            edits=(
                (
                    "let repeats_annotation = annotations\n",
                    "let repeats_annotation = annotations.is_empty()\n        || annotations\n",
                ),
            ),
            cases=(
                "annotated_type_in_docstring::pass::returns_without_annotation_exempt",
            ),
        ),
        Mutation(
            label="class Args entries read `__init__` annotations only (annotated-type-in-docstring)",
            path=ANNOTATED_TYPE_IN_DOCSTRING,
            edits=(
                (
                    "|constructor| annotated_parameter_names(&constructor),",
                    "|_| annotated_attributes.clone(),",
                ),
            ),
            cases=(
                "annotated_type_in_docstring::pass::unannotated_constructor_parameter_exempt",
            ),
        ),
        Mutation(
            label="nullability marker only at a clause end (unexplained-optional-argument, unexplained-optional-return)",
            path=DOCSTRINGS,
            edits=(
                (
                    "            if !is_clause_end(after.strip_prefix(')').unwrap_or(after)) {",
                    "            if false {",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::none_not_at_clause_end_exempt",
                "unexplained_optional_return::pass::none_not_at_clause_end_exempt",
            ),
        ),
        Mutation(
            label="explained by `None` outside the marker (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(("    none_words(text).next().is_some()\n", "    false\n"),),
            cases=("unexplained_optional_argument::pass::explained_by_none_exempt",),
        ),
        Mutation(
            label="explained by an omission phrase (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(
                (
                    "            .any(|phrase| lowercase.contains(phrase))",
                    "            .any(|_| false)",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::explained_by_omission_phrase_exempt",
            ),
        ),
        Mutation(
            label="`None` is case-sensitive (unexplained-optional-argument, unexplained-optional-return)",
            path=DOCSTRINGS,
            edits=(
                (
                    "    text.match_indices(NONE).filter_map(",
                    "    text.to_ascii_lowercase()\n"
                    '        .match_indices("none")\n'
                    "        .map(|(start, _)| (start, ()))\n"
                    "        .collect::<Vec<_>>()\n"
                    "        .into_iter()\n"
                    "        .filter_map(",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::lowercase_none_exempt",
                "unexplained_optional_return::pass::lowercase_none_exempt",
            ),
        ),
        Mutation(
            label="explained by `None` outside the marker (unexplained-optional-return)",
            path=UNEXPLAINED_OPTIONAL_RETURN,
            edits=(
                (
                    "    none_words(&unmarked)\n        .next()\n        .is_none()\n",
                    "    true\n",
                ),
            ),
            cases=("unexplained_optional_return::pass::explained_by_none_exempt",),
        ),
        Mutation(
            label="Napoleon nullable type prefix stripped before matching (unexplained-optional-return)",
            path=DOCSTRINGS,
            edits=(
                (
                    "            .map_or(opening_line.text.as_str(), |typed| typed.description);",
                    "            .map_or(opening_line.text.as_str(), |typed| typed.type_text);",
                ),
            ),
            cases=(
                "unexplained_optional_return::pass::napoleon_nullable_type_prefix_exempt",
            ),
        ),
        Mutation(
            label="only Returns/Yields sections are read (unexplained-optional-return)",
            path=UNEXPLAINED_OPTIONAL_RETURN,
            edits=(
                (
                    "                DocstringSectionKind::Returns | DocstringSectionKind::Yields\n",
                    "                _\n",
                ),
            ),
            cases=("unexplained_optional_return::pass::other_sections_not_checked",),
        ),
        Mutation(
            label="a lone `optional` must start a clause (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(
                (
                    "        } else if is_clause_start(before) && is_clause_end(after) {",
                    "        } else if is_clause_end(after) {",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::optional_not_first_word_exempt",
            ),
        ),
        Mutation(
            label="leading `Optional` must not run into a letter (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(
                ("    if after.starts_with(char::is_alphabetic) {", "    if false {"),
            ),
            cases=("unexplained_optional_argument::pass::optionally_exempt",),
        ),
        Mutation(
            label="only a type slot that is exactly `optional` (unexplained-optional-argument)",
            path=DOCSTRINGS,
            edits=(
                (
                    ".filter(|slot| slot.eq_ignore_ascii_case(OPTIONAL_MARKER))",
                    ".filter(|slot| slot.to_ascii_lowercase().contains(OPTIONAL_MARKER))",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::napoleon_optional_type_slot_exempt",
            ),
        ),
        Mutation(
            label="only Args and Attributes sections are read (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(("            _ => continue,", "            _ => parameters,"),),
            cases=("unexplained_optional_argument::pass::other_sections_not_checked",),
        ),
        Mutation(
            label="omission marker with a non-None default (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(
                (
                    "    let type_slot_marker = if has_value_default {",
                    "    let type_slot_marker = if false {",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::omission_marker_with_value_default_exempt",
            ),
        ),
        Mutation(
            label="class Args entries matched with `__init__` (unexplained-optional-argument)",
            path=UNEXPLAINED_OPTIONAL_ARGUMENT,
            edits=(
                (
                    "        let constructor_parameters = class\n"
                    "            .constructor()",
                    "        let constructor_parameters = class\n"
                    "            .constructor()\n"
                    "            .filter(|_| false)",
                ),
            ),
            cases=(
                "unexplained_optional_argument::pass::constructor_value_default_exempt",
            ),
        ),
        Mutation(
            label="whole-docstring phrase match (deferral-docstring E7)",
            path=DEFERRAL_DOCSTRING,
            edits=(
                (
                    "        None => text == phrase,",
                    "        None => text.starts_with(phrase),",
                ),
            ),
            cases=("deferral_docstring::pass::phrase_followed_by_text_exempt",),
        ),
        Mutation(
            label="`{base}` matches only bases and in-file ancestors (deferral-docstring E8)",
            path=DEFERRAL_DOCSTRING,
            edits=(
                (
                    ".is_some_and(|reference| base_references.iter().any(|base| base == reference)),",
                    ".is_some_and(|reference| !reference.contains(' ')),",
                ),
            ),
            cases=("deferral_docstring::pass::reference_to_non_base_exempt",),
        ),
        Mutation(
            label="only methods directly in a class body (deferral-docstring E9)",
            path=DEFERRAL_DOCSTRING,
            edits=CHECK_EVERY_FUNCTION,
            cases=("deferral_docstring::pass::module_level_function_exempt",),
        ),
        Mutation(
            label="only @override methods (deferral-docstring E10)",
            path=DEFERRAL_DOCSTRING,
            edits=DROP_OVERRIDE_CHECK,
            cases=("deferral_docstring::pass::undecorated_override_exempt",),
        ),
        Mutation(
            label="no match when the override extends the inherited text (copied-base-docstring E1)",
            path=COPIED_BASE_DOCSTRING,
            edits=(
                (
                    "let is_copy = docstring.normalized() == inherited.normalized()",
                    "let is_copy = docstring.normalized().starts_with(inherited.normalized().as_str())",
                ),
            ),
            cases=("copied_base_docstring::pass::extension_of_inherited_exempt",),
        ),
        Mutation(
            label="whole override compared with the inherited summary (copied-base-docstring E2)",
            path=COPIED_BASE_DOCSTRING,
            edits=(
                (
                    "|| docstring.copies_summary_of(&inherited);",
                    "|| crate::code_lint::ast::python::normalize_docstring_text(&docstring.summary_paragraph())\n"
                    "                    == crate::code_lint::ast::python::normalize_docstring_text(&inherited.summary_paragraph());",
                ),
            ),
            cases=("copied_base_docstring::pass::same_summary_different_body_exempt",),
        ),
        Mutation(
            label="no token-overlap match (copied-base-docstring E3)",
            path=COPIED_BASE_DOCSTRING,
            edits=(
                (
                    "|| docstring.copies_summary_of(&inherited);",
                    "|| docstring.copies_summary_of(&inherited)\n"
                    "                || {\n"
                    "                    let words = |text: &str| text.split_whitespace().map(str::to_owned)\n"
                    "                        .collect::<std::collections::HashSet<_>>();\n"
                    "                    let (own, base) = (words(&docstring.normalized()), words(&inherited.normalized()));\n"
                    "                    own.intersection(&base).count() * 5 >= own.union(&base).count() * 4\n"
                    "                };",
                ),
            ),
            cases=("copied_base_docstring::pass::adapted_copy_exempt",),
        ),
        Mutation(
            label="no match when the override is a word prefix of the inherited text (copied-base-docstring E4)",
            path=COPIED_BASE_DOCSTRING,
            edits=(
                (
                    "let is_copy = docstring.normalized() == inherited.normalized()",
                    "let is_copy = inherited.normalized().starts_with(docstring.normalized().as_str())",
                ),
            ),
            cases=("copied_base_docstring::pass::word_prefix_of_inherited_exempt",),
        ),
        Mutation(
            label="a base binds to the last earlier class of that name (copied-base-docstring D10)",
            path=CLASSES,
            edits=(
                (
                    "                .rposition(|candidate| candidate.name == name)",
                    "                .position(|candidate| candidate.name == name)",
                ),
            ),
            cases=(
                "copied_base_docstring::pass::base_bound_to_last_earlier_definition",
            ),
        ),
        Mutation(
            label="bases resolve only to classes defined before the subclass (override docstrings E6)",
            path=CLASSES,
            edits=(("            classes[..index]\n", "            classes\n"),),
            cases=(
                "copied_base_docstring::pass::base_defined_after_subclass_exempt",
                "partial_override_docstring::pass::base_defined_after_subclass_exempt",
            ),
        ),
        Mutation(
            label="an undocumented nearer ancestor is skipped (override docstrings D9)",
            path=CLASSES,
            edits=(
                (
                    "    local_ancestors(classes, index)\n        .into_iter()\n",
                    "    local_ancestors(classes, index)\n"
                    "        .into_iter()\n"
                    "        .filter(|&ancestor| classes[ancestor].methods().iter().any(|method| method.name == name))\n"
                    "        .take(1)\n",
                ),
            ),
            cases=(
                "copied_base_docstring::fail::undocumented_nearer_ancestor_skipped",
                "partial_override_docstring::fail::undocumented_nearer_ancestor_skipped",
            ),
        ),
        Mutation(
            label="only methods directly in a class body (copied-base-docstring E9)",
            path=COPIED_BASE_DOCSTRING,
            edits=CHECK_EVERY_FUNCTION,
            cases=("copied_base_docstring::pass::module_level_function_exempt",),
        ),
        Mutation(
            label="only @override methods (copied-base-docstring E10)",
            path=COPIED_BASE_DOCSTRING,
            edits=DROP_OVERRIDE_CHECK,
            cases=("copied_base_docstring::pass::undecorated_override_exempt",),
        ),
        Mutation(
            label="only methods directly in a class body (partial-override-docstring E9)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=CHECK_EVERY_FUNCTION,
            cases=("partial_override_docstring::pass::module_level_function_exempt",),
        ),
        Mutation(
            label="only @override methods (partial-override-docstring E10)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=DROP_OVERRIDE_CHECK,
            cases=("partial_override_docstring::pass::undecorated_override_exempt",),
        ),
        Mutation(
            label="only contract sections are required (partial-override-docstring E11)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=(
                (
                    '    (DocstringSectionKind::Raises, "Raises"),\n];',
                    '    (DocstringSectionKind::Raises, "Raises"),\n'
                    '    (DocstringSectionKind::Attributes, "Attributes"),\n'
                    '    (DocstringSectionKind::Other, "Other"),\n];',
                ),
            ),
            cases=("partial_override_docstring::pass::non_contract_sections_exempt",),
        ),
        Mutation(
            label="section kinds compared, not header text (partial-override-docstring E12)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=(
                (
                    "= docstring\n        .sections()\n        .iter()\n",
                    "= docstring\n"
                    "        .sections()\n"
                    "        .iter()\n"
                    "        .filter(|section| CONTRACT_SECTIONS.iter().any(|(_, header)| *header == section.header))\n",
                ),
            ),
            cases=(
                "partial_override_docstring::pass::other_header_of_same_kind_exempt",
            ),
        ),
        Mutation(
            label="Returns and Yields sections satisfy each other (partial-override-docstring E13)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=(
                (
                    "    if docstring.describes_result() {",
                    "    if docstring.has_result_verb_summary() {",
                ),
            ),
            cases=("partial_override_docstring::pass::yields_for_returns_exempt",),
        ),
        Mutation(
            label="a Returns/Yields summary documents the result (partial-override-docstring E14)",
            path=DOCSTRINGS,
            edits=(
                (
                    "        self.has_result_section() || self.has_result_verb_summary()",
                    "        self.has_result_section()",
                ),
            ),
            cases=("partial_override_docstring::pass::result_verb_summary_exempt",),
        ),
        Mutation(
            label="a copy of the inherited summary is left to copied-base-docstring (partial-override-docstring E15)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=(
                (
                    "            if docstring.copies_summary_of(&inherited) {",
                    "            if false && docstring.copies_summary_of(&inherited) {",
                ),
            ),
            cases=("partial_override_docstring::pass::summary_copy_exempt",),
        ),
        Mutation(
            label="a NotImplementedError-only Raises section is not required (partial-override-docstring E16)",
            path=PARTIAL_OVERRIDE_DOCSTRING,
            edits=(
                (
                    "        .filter(|section| !is_abstract_raises(section))",
                    "        .filter(|_| true)",
                ),
            ),
            cases=("partial_override_docstring::pass::abstract_raises_exempt",),
        ),
        Mutation(
            label="nested functions are not checked (undocumented-nontrivial-function)",
            path=UNDOCUMENTED_NONTRIVIAL_FUNCTION,
            edits=(("                && !signature.is_nested()\n", ""),),
            cases=("undocumented_nontrivial_function::pass::nested_function_exempt",),
        ),
        Mutation(
            label="constructors of a documented class (undocumented-nontrivial-function)",
            path=UNDOCUMENTED_NONTRIVIAL_FUNCTION,
            edits=(
                (
                    "                && !ast::python::is_documented_by_enclosing_class(signature)\n",
                    "",
                ),
            ),
            cases=(
                "undocumented_nontrivial_function::pass::constructor_of_documented_class_exempt",
            ),
        ),
        Mutation(
            label="data-model dunders (undocumented-nontrivial-function)",
            path=FUNCTIONS,
            edits=(
                (
                    "        is_data_model_dunder(&self.name)\n"
                    "            || extract_decorators_from_slice(&self.definition.decorator_list",
                    "        extract_decorators_from_slice(&self.definition.decorator_list",
                ),
            ),
            cases=("undocumented_nontrivial_function::pass::data_model_dunder_exempt",),
        ),
        Mutation(
            label="@override methods (undocumented-nontrivial-function)",
            path=FUNCTIONS,
            edits=(
                (
                    "                    decorator.matches_any(OVERRIDE_DECORATOR_PATHS)\n"
                    '                        || decorator.is_attribute_named(&[SETTER_ATTRIBUTE, "deleter"])',
                    '                    decorator.is_attribute_named(&[SETTER_ATTRIBUTE, "deleter"])',
                ),
            ),
            cases=("undocumented_nontrivial_function::pass::override_method_exempt",),
        ),
        Mutation(
            label="property setters and deleters (undocumented-nontrivial-function)",
            path=FUNCTIONS,
            edits=(
                (
                    '                        || decorator.is_attribute_named(&[SETTER_ATTRIBUTE, "deleter"])',
                    "",
                ),
            ),
            cases=(
                "undocumented_nontrivial_function::pass::property_accessors_exempt",
            ),
        ),
        Mutation(
            label="undocumented functions left to undocumented-nontrivial-function (unexplained-parameter)",
            path=UNEXPLAINED_PARAMETER,
            edits=(
                (
                    "            if own_docstring.is_none() && class_docstring.is_none() {\n"
                    "                return None;\n"
                    "            }\n",
                    "",
                ),
            ),
            cases=("unexplained_parameter::pass::undocumented_function_exempt",),
        ),
        Mutation(
            label="nested functions are not checked (unexplained-parameter)",
            path=UNEXPLAINED_PARAMETER,
            edits=(
                (
                    "            !signature.is_nested()\n                && ",
                    "            ",
                ),
            ),
            cases=("unexplained_parameter::pass::nested_function_exempt",),
        ),
        Mutation(
            label="@override and exempt methods (unexplained-parameter)",
            path=UNEXPLAINED_PARAMETER,
            edits=(
                (
                    "                && !signature.is_documented_elsewhere()\n",
                    "\n",
                ),
            ),
            cases=("unexplained_parameter::pass::override_method_exempt",),
        ),
        Mutation(
            label="`_`-prefixed parameters (unexplained-parameter)",
            path=UNEXPLAINED_PARAMETER,
            edits=(
                (
                    "                    !parameter.name.starts_with('_')\n"
                    "                        && is_non_obvious(parameter)",
                    "                    is_non_obvious(parameter)",
                ),
            ),
            cases=(
                "unexplained_parameter::pass::underscore_prefixed_parameter_exempt",
            ),
        ),
        Mutation(
            label="unchanged constructor parameter documented in class Attributes (unexplained-parameter)",
            path=UNEXPLAINED_PARAMETER,
            edits=(
                (
                    "                        && !documented_unchanged_attrs.contains(&parameter.name)",
                    "",
                ),
            ),
            cases=(
                "unexplained_parameter::pass::unchanged_constructor_attribute_exempt",
            ),
        ),
        Mutation(
            label="class Args section satisfies constructor (unexplained-parameter)",
            path=UNEXPLAINED_PARAMETER,
            edits=(
                (
                    "                || class_docstring.is_some_and(|doc| doc.has_section(DocstringSectionKind::Args))",
                    "",
                ),
            ),
            cases=(
                "unexplained_parameter::pass::class_args_section_satisfies_constructor_exempt",
            ),
        ),
    ),
    unmutated=(
        Unmutated(
            label="positive layouts (verb-led-returns-section)",
            reason="No exemption: sections that open with a noun phrase.",
            cases=("verb_led_returns_section::pass::noun_phrase_sections",),
        ),
        Unmutated(
            label="constructs the rule never visits (verb-led-returns-section)",
            reason="The summary line is not a section: the rule only reads `PythonDocstring::sections()`.",
            cases=("verb_led_returns_section::pass::summary_line_not_checked",),
        ),
        Unmutated(
            label="positive layouts (verb-led-property-docstring)",
            reason="No exemption: property docstring that opens with a noun phrase, and an undocumented property.",
            cases=(
                "verb_led_property_docstring::pass::noun_phrase_property_docstring",
                "verb_led_property_docstring::pass::undocumented_property_exempt",
            ),
        ),
        Unmutated(
            label="positive layouts (undocumented-public-attribute)",
            reason="No exemption: Google layout with every attribute, stored arguments included, listed.",
            cases=("undocumented_public_attribute::pass::every_attribute_listed",),
        ),
        Unmutated(
            label="positive layouts (annotated-type-in-docstring)",
            reason="No exemption: untyped entries and sections under annotations.",
            cases=(
                "annotated_type_in_docstring::pass::untyped_entries_under_annotations",
            ),
        ),
        Unmutated(
            label="constructs the rule never visits (annotated-type-in-docstring)",
            reason="A continuation line is not an entry: `parse_google_sections` only splits entry lines.",
            cases=(
                "annotated_type_in_docstring::pass::parenthetical_in_continuation_line",
            ),
        ),
        Unmutated(
            label="positive layouts (unexplained-optional-argument)",
            reason="No exemption: unmarked descriptions (`if any`, `Defaults to 3.`, `An optional …`).",
            cases=("unexplained_optional_argument::pass::unmarked_descriptions",),
        ),
        Unmutated(
            label="constructs the rule never visits (unexplained-optional-argument)",
            reason=(
                "Module docstrings are not read, and a wrapped `Format: …` line is a continuation:"
                " the shared parser owns entry splitting, so no rule-side edit disables it."
            ),
            cases=(
                "unexplained_optional_argument::pass::module_docstring_not_checked",
                "unexplained_optional_argument::pass::continuation_line_with_colon_not_an_entry",
            ),
        ),
        Unmutated(
            label="positive layouts (unexplained-optional-return)",
            reason="No exemption: `Returns:` section without a nullability marker.",
            cases=("unexplained_optional_return::pass::unmarked_returns_section",),
        ),
        Unmutated(
            label="positive layouts (override docstrings)",
            reason="No exemption: no docstring, a docstring stating a difference over a base without contract sections, a complete docstring.",
            cases=(
                "deferral_docstring::pass::no_docstring",
                "deferral_docstring::pass::difference_over_unsectioned_base",
                "copied_base_docstring::pass::no_docstring",
                "copied_base_docstring::pass::difference_over_unsectioned_base",
                "partial_override_docstring::pass::no_docstring",
                "partial_override_docstring::pass::difference_over_unsectioned_base",
                "partial_override_docstring::pass::complete_override_docstring",
            ),
        ),
        Unmutated(
            label="no in-file inherited docstring (override docstrings E5)",
            reason=(
                "Nothing to compare: no in-file ancestor documents the method, and no small edit makes the lookup"
                " invent a docstring. Skipping the undocumented nearer ancestor is mutated through the"
                " `undocumented_nearer_ancestor_skipped` fail cases."
            ),
            cases=(
                "copied_base_docstring::pass::undocumented_ancestor_exempt",
                "partial_override_docstring::pass::undocumented_ancestor_exempt",
            ),
        ),
        Unmutated(
            label="positive layouts (undocumented-nontrivial-function)",
            reason="No exemption: a documented function, and one just below both thresholds.",
            cases=(
                "undocumented_nontrivial_function::pass::documented_function",
                "undocumented_nontrivial_function::pass::below_both_thresholds",
            ),
        ),
        Unmutated(
            label="positive layouts (unexplained-parameter)",
            reason="No exemption: a function with an `Args:` section, a function with ordinary parameters, and a function below the complexity gate.",
            cases=(
                "unexplained_parameter::pass::args_section_present_exempt",
                "unexplained_parameter::pass::ordinary_parameters_exempt",
                "unexplained_parameter::pass::below_complexity_gate_exempt",
            ),
        ),
    ),
)
