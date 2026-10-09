"""Exemption mutations for Batch 1: Literals & Format Placeholders."""

from __future__ import annotations

from .model import (
    AST,
    FORMAT_STRINGS,
    INLINE_TEST_ALWAYS_FALSE,
    LOGGING,
    POLICY,
    PY_LITERALS,
    QUOTE_WRAPPED,
    REPEATED_LITERAL,
    RUST,
    STRINGS,
    UNMATCHED_LOGGER,
    Cluster,
    Mutation,
    Unmutated,
)

CLUSTER = Cluster(
    rules=(
        "bare_multiline_string",
        "repeated_literal",
        "quote_wrapped_placeholder",
        "unmatched_logger_placeholder",
    ),
    mutations=(
        # --- bare-multiline-string ---
        Mutation(
            label="Python standalone string statements (docstrings) exempt from bare-multiline-string",
            path=STRINGS,
            edits=(("let is_docstring = self.docstring_expr_span == Some(span);", "let is_docstring = false;"),),
            cases=(
                "bare_multiline_string::pass::module_docstring_exempt",
                "bare_multiline_string::pass::function_docstring_exempt",
            ),
        ),
        Mutation(
            label="Python allowed dedent wrapper call exempt from bare-multiline-string",
            path=STRINGS,
            edits=(("&& self.allowed_call_depth == 0", ""),),
            cases=("bare_multiline_string::pass::inspect_cleandoc_allowed",),
        ),
        Mutation(
            label="Python single-quoted strings with line breaks exempt from bare-multiline-string",
            path=STRINGS,
            edits=(("if is_triple_quoted", "let _ = is_triple_quoted;\n        if true"),),
            cases=(
                "bare_multiline_string::pass::backslash_continuation_allowed@python",
                "bare_multiline_string::pass::fstring_multiline_interpolation_allowed",
            ),
        ),
        Mutation(
            label="Rust allowed indoc macro exempt from bare-multiline-string",
            path=RUST,
            edits=(("&& !is_enclosed_in_macro(token, &is_allowed_wrapper)", ""),),
            cases=("bare_multiline_string::pass::indoc_macro_allowed",),
        ),
        Mutation(
            label="Rust backslash line continuation exempt from bare-multiline-string",
            path=RUST,
            edits=(("lines.any(|line| !line.trim_end().ends_with('\\\\'))", "true"),),
            cases=("bare_multiline_string::pass::backslash_continuation_allowed@rust",),
        ),
        Mutation(
            label="Rust insta inline snapshot exempt from bare-multiline-string",
            path=RUST,
            edits=(("&& !is_insta_inline_snapshot(token)", ""),),
            cases=("bare_multiline_string::pass::insta_inline_snapshot_exempt",),
        ),
        Mutation(
            label="Rust doc attribute exempt from bare-multiline-string",
            path=RUST,
            edits=(("&& !is_enclosed_in_doc_attribute(token)", ""),),
            cases=("bare_multiline_string::pass::doc_attribute_exempt",),
        ),
        # --- repeated-literal ---
        Mutation(
            label="trivial integer literals (-1..=2) exempt from repeated-literal",
            path=POLICY,
            edits=(("LiteralValue::Int(value) => (-1..=2).contains(value),", "LiteralValue::Int(_) => false,"),),
            cases=("repeated_literal::pass::trivial_integers",),
        ),
        Mutation(
            label="trivial float literals (-1.0, 0.0, 1.0, 2.0) exempt from repeated-literal",
            path=POLICY,
            edits=(
                (
                    "LiteralValue::Float(bits) => [-1.0, 0.0, 1.0, 2.0].contains(&f64::from_bits(*bits)),",
                    "LiteralValue::Float(_) => false,",
                ),
            ),
            cases=("repeated_literal::pass::trivial_floats",),
        ),
        Mutation(
            label="single-character string literals exempt from repeated-literal",
            path=POLICY,
            edits=(("units < 2 || !has_alphanumeric", "!has_alphanumeric"),),
            cases=("repeated_literal::pass::single_char_strings",),
        ),
        Mutation(
            label="non-alphanumeric string literals exempt from repeated-literal",
            path=POLICY,
            edits=(("units < 2 || !has_alphanumeric", "units < 2"),),
            cases=("repeated_literal::pass::non_alphanumeric_strings",),
        ),
        Mutation(
            label="constants sharing a value are not flagged without an inline use",
            path=REPEATED_LITERAL,
            edits=(
                (
                    "LiteralRole::ConstantDefinition => group.definitions += 1,",
                    "LiteralRole::ConstantDefinition => group.inline_uses.push(occurrence.node),",
                ),
            ),
            cases=("repeated_literal::pass::constants_sharing_a_value",),
        ),
        Mutation(
            label="Python composite constant initializer not walked as inline literals",
            path=PY_LITERALS,
            edits=(("self.record_constant_rhs(&assign.value);", "walk_stmt(self, statement);"),),
            cases=("repeated_literal::pass::composite_constant_does_not_pair_with_single_use@python",),
        ),
        Mutation(
            label="Rust composite constant initializer not walked as inline literals",
            path=RUST,
            edits=(
                (
                    "if let Some((true, initializer)) = constant_initializer_expr(node) {",
                    "if let Some((true, initializer)) = None::<(bool, Option<ast::Expr>)> {",
                ),
            ),
            cases=("repeated_literal::pass::composite_constant_does_not_pair_with_single_use@rust",),
        ),
        Mutation(
            label="Python str and bytes literal values kept distinct",
            path=PY_LITERALS,
            edits=(
                (
                    "LiteralValue::Bytes(\n"
                    "                            String::from_utf8_lossy(part.as_slice()).into_owned(),\n"
                    "                        )",
                    "LiteralValue::Str(\n"
                    "                            String::from_utf8_lossy(part.as_slice()).into_owned(),\n"
                    "                        )",
                ),
            ),
            cases=("repeated_literal::pass::str_distinct_from_bytes@python",),
        ),
        Mutation(
            label="Rust str and bytes literal values kept distinct",
            path=RUST,
            edits=(
                (
                    "return Some(LiteralValue::Bytes(\n"
                    "            String::from_utf8_lossy(&decoded).into_owned(),\n"
                    "        ));",
                    "return Some(LiteralValue::Str(\n"
                    "            String::from_utf8_lossy(&decoded).into_owned(),\n"
                    "        ));",
                ),
            ),
            cases=("repeated_literal::pass::str_distinct_from_bytes@rust",),
        ),
        Mutation(
            label="number and its negation kept distinct",
            path=AST,
            edits=(
                (
                    "Self::Int(value) => value.checked_neg().map(Self::Int),",
                    "Self::Int(value) => Some(Self::Int(*value)),",
                ),
            ),
            cases=("repeated_literal::pass::number_distinct_from_negation",),
        ),
        Mutation(
            label="Python docstrings exempt from repeated-literal",
            path=PY_LITERALS,
            edits=(
                (
                    "Stmt::Expr(expr_statement) if is_standalone_string_expr(&expr_statement.value) => {}",
                    "Stmt::Expr(_) if false => {}",
                ),
            ),
            cases=("repeated_literal::pass::repeated_docstrings",),
        ),
        Mutation(
            label="Python type annotations exempt from repeated-literal",
            path=PY_LITERALS,
            edits=(
                (
                    "fn visit_annotation(&mut self, _expr: &'a Expr) {}",
                    "fn visit_annotation(&mut self, expr: &'a Expr) { self.visit_expr(expr); }",
                ),
            ),
            cases=("repeated_literal::pass::repeated_string_annotations",),
        ),
        Mutation(
            label="Python Literal[...] type arguments exempt from repeated-literal",
            path=PY_LITERALS,
            edits=(
                (
                    'if resolve_path_and_terminal_expr(&sub.value, &self.file.source).1 == "Literal" =>',
                    "if false =>",
                ),
            ),
            cases=("repeated_literal::pass::repeated_literal_type_values",),
        ),
        Mutation(
            label="Python type-name first argument in cast/TypeVar/etc. exempt from repeated-literal",
            path=PY_LITERALS,
            edits=(("if TYPE_NAME_FIRST_ARGUMENT_CALLS.contains(&terminal.as_str()) {", "if false {"),),
            cases=("repeated_literal::pass::repeated_type_names_in_cast",),
        ),
        Mutation(
            label="Python interpolated f-string literal segments exempt from repeated-literal",
            path=PY_LITERALS,
            edits=(
                (
                    "if fpart\n"
                    "                        .elements\n"
                    "                        .iter()\n"
                    "                        .any(ruff_python_ast::InterpolatedStringElement::is_interpolation)\n"
                    "                    {",
                    "if false {",
                ),
            ),
            cases=("repeated_literal::pass::repeated_interpolated_fstrings",),
        ),
        Mutation(
            label="Rust inline test code exempt from repeated-literal",
            path=AST,
            edits=INLINE_TEST_ALWAYS_FALSE,
            cases=("repeated_literal::pass::test_module_literals_are_ignored",),
        ),
        Mutation(
            label="Rust attributes exempt from repeated-literal",
            path=RUST,
            edits=(("if ast::Attr::can_cast(kind) {\n        return;\n    }", ""),),
            cases=("repeated_literal::pass::repeated_attribute_arguments",),
        ),
        Mutation(
            label="Rust formatting, assertion, and logging macros exempt from repeated-literal",
            path=RUST,
            edits=(
                (
                    "fn is_literal_exempt_macro(name: &str) -> bool {\n    LITERAL_EXEMPT_MACROS.contains(&name)\n}",
                    "fn is_literal_exempt_macro(_name: &str) -> bool {\n    false\n}",
                ),
            ),
            cases=(
                "repeated_literal::pass::repeated_format_macro_arguments",
                "repeated_literal::pass::repeated_assert_macro_arguments",
                "repeated_literal::pass::known_gap_values_inside_exempt_macros",
            ),
        ),
        Mutation(
            label="Rust negative numbers in macro token_trees skipped by repeated-literal",
            path=RUST,
            edits=(("let is_signed_number = follows_minus && is_number;", "let _ = follows_minus;\n                    let is_signed_number = false;"),),
            cases=("repeated_literal::pass::known_gap_negative_numbers_in_macros",),
        ),
        Mutation(
            label="Rust tuple field indices in macro token_trees skipped by repeated-literal",
            path=RUST,
            edits=(("let is_tuple_field = follows_dot && is_number;", "let _ = follows_dot;\n                    let is_tuple_field = false;"),),
            cases=("repeated_literal::pass::tuple_field_positions_in_macros_are_not_literals",),
        ),
        # --- quote-wrapped-placeholder ---
        Mutation(
            label="f-string placeholder with conversion (!r, !s, !a) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "PythonFormatStyle::FString => (placeholder.conversion.is_none()\n"
                    "            && placeholder.format_spec.is_none()",
                    "PythonFormatStyle::FString => (placeholder.format_spec.is_none()",
                ),
            ),
            cases=(
                "quote_wrapped_placeholder::pass::fstring_already_has_repr_inside_quotes",
                "quote_wrapped_placeholder::pass::fstring_explicit_str_or_ascii_conversion",
            ),
        ),
        Mutation(
            label="f-string placeholder with format specifier (:...) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "&& placeholder.format_spec.is_none()\n            && !placeholder.is_self_documenting",
                    "&& !placeholder.is_self_documenting",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::fstring_with_format_specifier",),
        ),
        Mutation(
            label="f-string self-documenting debug placeholder ({x=}) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(("&& !placeholder.is_self_documenting", ""),),
            cases=("quote_wrapped_placeholder::pass::fstring_with_debug_equals",),
        ),
        Mutation(
            label="str.format non-identifier brace contents exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(("&& extract_valid_field_root(field).is_some()", ""),),
            cases=(
                "quote_wrapped_placeholder::pass::non_identifier_braces_in_str_format",
                "quote_wrapped_placeholder::pass::spaced_non_identifier_braces_in_str_format",
            ),
        ),
        Mutation(
            label="zero-argument logger call not treated as printf format string",
            path=FORMAT_STRINGS,
            edits=(
                (
                    ".filter(|lc| lc.uses_printf && lc.has_trailing_positional_args)",
                    ".filter(|lc| lc.uses_printf)",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::zero_arg_logger_call",),
        ),
        Mutation(
            label="raw f-strings skipped by collect_format_strings",
            path=FORMAT_STRINGS,
            edits=(("if !fstring.flags.prefix().is_raw() {", "if true {"),),
            cases=("quote_wrapped_placeholder::pass::raw_fstring",),
        ),
        Mutation(
            label="raw str.format / printf strings skipped by collect_format_strings",
            path=FORMAT_STRINGS,
            edits=(("if !part.flags.prefix().is_raw() {", "if true {"),),
            cases=("quote_wrapped_placeholder::pass::raw_str_format",),
        ),
        Mutation(
            label="printf non-%s conversions (%r, %d, %f) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    'if placeholder.conversion.as_deref() != Some("s") || placeholder.format_spec.is_some() {',
                    "if placeholder.format_spec.is_some() {",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::printf_non_s_conversion",),
        ),
        Mutation(
            label="printf %s with format specifier (%.8s) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    'if placeholder.conversion.as_deref() != Some("s") || placeholder.format_spec.is_some() {',
                    'if placeholder.conversion.as_deref() != Some("s") {',
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::printf_s_with_format_specifier",),
        ),
        Mutation(
            label="printf escaped %% skipped by split_printf_fields",
            path=FORMAT_STRINGS,
            edits=(
                (
                    "if bytes.get(index + 1) == Some(&b'%') {\n            index += 2;\n            continue;\n        }",
                    "if bytes.get(index + 1) == Some(&b'%') {\n"
                    "            if let Some((end, placeholder)) = parse_printf_field(content, index + 1) {\n"
                    "                literals.push(content[literal_start..index].to_owned());\n"
                    "                placeholders.push(placeholder);\n"
                    "                index = end;\n"
                    "                literal_start = index;\n"
                    "                continue;\n"
                    "            }\n"
                    "        }",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::printf_escaped_percent",),
        ),
        Mutation(
            label="str.format escaped {{ and }} skipped by split_brace_fields",
            path=FORMAT_STRINGS,
            edits=(
                (
                    "if is_escaped {\n            index += 2;\n            continue;\n        }",
                    "if is_escaped && current == b'{' && let Some(close) = content[index + 2..].find(\"}}\") {\n"
                    "            literals.push(content[literal_start..index].to_owned());\n"
                    "            placeholders.push(brace_placeholder(&content[index + 1..=index + 2 + close]));\n"
                    "            index = index + 2 + close + 2;\n"
                    "            literal_start = index;\n"
                    "            continue;\n"
                    "        }",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::str_format_escaped_braces",),
        ),
        Mutation(
            label="str.format placeholder with conversion (!r) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "PythonFormatStyle::StrFormat => (placeholder.conversion.is_none()\n"
                    "            && placeholder.format_spec.is_none()",
                    "PythonFormatStyle::StrFormat => (placeholder.format_spec.is_none()",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::str_format_with_conversion",),
        ),
        Mutation(
            label="str.format placeholder with format specifier (:...) exempt from quote-wrapped-placeholder",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "PythonFormatStyle::StrFormat => (placeholder.conversion.is_none()\n"
                    "            && placeholder.format_spec.is_none()",
                    "PythonFormatStyle::StrFormat => (placeholder.conversion.is_none()",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::str_format_with_format_specifier",),
        ),
        Mutation(
            label="left prose boundary requires space or '(' before opening quote",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "if !starts_at_beginning && !preceded_by_space_or_paren {\n        return false;\n    }",
                    "let _ = (starts_at_beginning, preceded_by_space_or_paren);",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::attached_colon_before_quote",),
        ),
        Mutation(
            label="left prose boundary rejects '=' key-value prefix before opening quote",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "if !starts_at_beginning && !preceded_by_space_or_paren {\n        return false;\n    }",
                    "let _ = (starts_at_beginning, preceded_by_space_or_paren);",
                ),
                (
                    "last_char.is_ascii_alphanumeric() || matches!(last_char, ':' | ',' | '(' | '.' | '!' | '?')",
                    "let _ = last_char;\n    true",
                ),
            ),
            cases=(
                "quote_wrapped_placeholder::pass::html_attribute_quotes",
                "quote_wrapped_placeholder::pass::key_equals_quoted_placeholder",
                "quote_wrapped_placeholder::pass::cli_flag_attached_equals",
                "quote_wrapped_placeholder::pass::concatenated_flag_prefix_in_str_format",
                "quote_wrapped_placeholder::pass::concatenated_flag_prefix_in_fstring",
            ),
        ),
        Mutation(
            label="left prose boundary rejects unclosed markdown backtick span",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "if backtick_count % 2 != 0 || has_unclosed_structured_delimiter(structure_prefix_before_quote) {",
                    "let _ = backtick_count;\n"
                    "    if has_unclosed_structured_delimiter(structure_prefix_before_quote) {",
                ),
            ),
            cases=(
                "quote_wrapped_placeholder::pass::placeholder_inside_markdown_backticks",
                "quote_wrapped_placeholder::pass::concatenated_backtick_prefix_in_logger",
                "quote_wrapped_placeholder::pass::concatenated_backtick_prefix_in_fstring",
            ),
        ),
        Mutation(
            label="left prose boundary rejects unclosed '{' / '[' structured delimiter",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "if backtick_count % 2 != 0 || has_unclosed_structured_delimiter(structure_prefix_before_quote) {",
                    "let _ = structure_prefix_before_quote;\n    if backtick_count % 2 != 0 {",
                ),
            ),
            cases=(
                "quote_wrapped_placeholder::pass::escaped_braces_json_in_fstring",
                "quote_wrapped_placeholder::pass::escaped_braces_json_in_str_format",
            ),
        ),
        Mutation(
            label="right prose boundary rejects identifier character after punctuation ('.py')",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "second_char.is_ascii_whitespace()\n"
                    "                || is_prose_punctuation(second_char)\n"
                    "                || matches!(second_char, '\\'' | '\"')",
                    "let _ = second_char;\n            true",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::file_extension_after_closing_quote",),
        ),
        Mutation(
            label="right prose boundary rejects punctuation immediately followed by another placeholder (':{port}')",
            path=QUOTE_WRAPPED,
            edits=((".map_or(!followed_by_placeholder,", ".map_or(true,"),),
            cases=("quote_wrapped_placeholder::pass::colon_before_placeholder_after_closing_quote",),
        ),
        Mutation(
            label="mismatched opening and closing quotes rejected",
            path=QUOTE_WRAPPED,
            edits=(
                (
                    "else if after.as_bytes().first() == Some(&b'\\\\') && after.as_bytes().get(1) == Some(&quote) {",
                    "else if after.as_bytes().first() == Some(&b'\\\\') && matches!(after.as_bytes().get(1), Some(b'\\'' | b'\"')) {",
                ),
            ),
            cases=("quote_wrapped_placeholder::pass::mismatched_quotes",),
        ),
        # --- unmatched-logger-placeholder ---
        Mutation(
            label="positional empty '{}' root excluded by is_python_identifier",
            path=FORMAT_STRINGS,
            edits=(
                (
                    "let Some(first) = characters.next() else {\n        return false;\n    };",
                    "let Some(first) = characters.next() else {\n        return true;\n    };",
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::positional_empty_braces_with_positional_arg",),
        ),
        Mutation(
            label="digit-starting field roots ('0', '0.id', '0[sku]') excluded by is_python_identifier",
            path=FORMAT_STRINGS,
            edits=(
                (
                    "let valid_start = first == '_' || first.is_alphabetic();\n"
                    "    valid_start\n"
                    "        && characters.all(|character| character == '_' || character.is_alphanumeric())\n"
                    "        && !first.is_ascii_digit()",
                    "let _ = first;\n"
                    "    characters.all(|character| character == '_' || character.is_alphanumeric())",
                ),
            ),
            cases=(
                "unmatched_logger_placeholder::pass::positional_numbered_braces_with_positional_arg",
                "unmatched_logger_placeholder::pass::positional_compound_attribute",
                "unmatched_logger_placeholder::pass::positional_compound_subscript",
            ),
        ),
        Mutation(
            label="conversion flag after '!' stripped before parsing field name",
            path=FORMAT_STRINGS,
            edits=(
                (
                    'Some((before, "r" | "s" | "a")) => Some(before),',
                    "Some((_before, after)) => Some(after),",
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::positional_with_conversion",),
        ),
        Mutation(
            label="named placeholder matched by keyword argument exempt from unmatched-logger-placeholder",
            path=UNMATCHED_LOGGER,
            edits=((".find(|root| !call.keyword_names.contains(root))?;", ".next()?;"),),
            cases=(
                "unmatched_logger_placeholder::pass::named_placeholder_with_matching_keyword_arg",
                "unmatched_logger_placeholder::pass::named_compound_attribute_with_matching_keyword_arg",
                "unmatched_logger_placeholder::pass::named_compound_subscript_with_matching_keyword_arg",
            ),
        ),
        Mutation(
            label="logger calls without trailing positional arguments exempt from unmatched-logger-placeholder",
            path=UNMATCHED_LOGGER,
            edits=(
                (
                    ".filter(|call| call.has_trailing_positional_args && !call.has_keyword_splat)",
                    ".filter(|call| !call.has_keyword_splat)",
                ),
            ),
            cases=(
                "unmatched_logger_placeholder::pass::zero_format_args_with_literal_braces",
                "unmatched_logger_placeholder::pass::zero_positional_format_args_with_structlog_kwargs",
                "unmatched_logger_placeholder::pass::zero_positional_format_args_with_exc_info_kwarg",
            ),
        ),
        Mutation(
            label="escaped '{{' and '}}' skipped by named_format_field_roots",
            path=FORMAT_STRINGS,
            edits=(
                (
                    'if rest.starts_with("{{") || rest.starts_with("}}") {\n'
                    "            cursor += 2;\n"
                    "        } else if rest.starts_with('}') {\n"
                    "            return None;\n"
                    "        }",
                    'if rest.starts_with("{{") || rest.starts_with(\'}\') {\n            cursor += 1;\n        }',
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::escaped_double_braces_with_positional_arg",),
        ),
        Mutation(
            label="logger calls with **kwargs splat exempt from unmatched-logger-placeholder",
            path=UNMATCHED_LOGGER,
            edits=(
                (
                    ".filter(|call| call.has_trailing_positional_args && !call.has_keyword_splat)",
                    ".filter(|call| call.has_trailing_positional_args)",
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::dictionary_splat_kwargs_exempt",),
        ),
        Mutation(
            label="non-identifier braces (JSON/set literals) rejected by extract_valid_field_root",
            path=FORMAT_STRINGS,
            edits=(
                ("if !valid_root {\n        return None;\n    }", "let _ = valid_root;"),
                (
                    ".filter(|root| is_python_identifier(root))",
                    ".filter(|root| !root.is_empty() && !root.chars().all(|c| c.is_ascii_digit()))",
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::non_identifier_braces_json_or_set_with_positional_arg",),
        ),
        Mutation(
            label="unclosed '{' brace aborts named_format_field_roots",
            path=FORMAT_STRINGS,
            edits=(
                (
                    "let (delimiter_index, delimiter_char) = delimiter?;",
                    "let (delimiter_index, delimiter_char) = delimiter.unwrap_or((message.len(), '}'));",
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::malformed_unclosed_brace_ignored",),
        ),
        Mutation(
            label="logger.log inspects second positional argument as message",
            path=LOGGING,
            edits=(('let message_index = usize::from(method == "log");', "let message_index = 0;"),),
            cases=("unmatched_logger_placeholder::pass::logger_log_level_first_arg_with_valid_message",),
        ),
        Mutation(
            label="non-logger receivers ignored by extract_logger_call",
            path=LOGGING,
            edits=(("if !is_logger_receiver(&function.value) {\n        return None;\n    }", ""),),
            cases=("unmatched_logger_placeholder::pass::non_logger_call_ignored",),
        ),
        Mutation(
            label="decoded string value used for logger message so \\N{NAME} is not a format field",
            path=LOGGING,
            edits=(
                (
                    "message: static_string_text(message_expr),",
                    "message: Some(file.source[usize::from(message_expr.range().start())..usize::from(message_expr.range().end())].to_owned()),",
                ),
            ),
            cases=("unmatched_logger_placeholder::pass::unicode_named_character_escape_not_flagged",),
        ),
    ),
    unmutated=(
        Unmutated(
            label="adjacent single-line strings contain no newline in any individual string token",
            reason="each string literal part in the implicit concatenation is single-line",
            cases=("bare_multiline_string::pass::implicit_adjacent_concat_allowed",),
        ),
        Unmutated(
            label="distinct literal values or non-literal AST nodes in repeated-literal",
            reason="baseline single occurrence, raw-vs-decoded escape value difference, and Rust NameRef/Abi tokens",
            cases=(
                "repeated_literal::pass::single_occurrence",
                "repeated_literal::pass::raw_string_with_backslash_distinct_from_plain",
                "repeated_literal::pass::tuple_field_positions_are_not_literals",
                "repeated_literal::pass::repeated_extern_abi",
            ),
        ),
        Unmutated(
            label="unquoted placeholders, unformatted strings, and byte strings in quote-wrapped-placeholder",
            reason="no surrounding quote pair or not a formatted string literal",
            cases=(
                "quote_wrapped_placeholder::pass::fstring_repr_conversion_allowed",
                "quote_wrapped_placeholder::pass::fstring_backticks_allowed",
                "quote_wrapped_placeholder::pass::docstring_with_placeholder_syntax",
                "quote_wrapped_placeholder::pass::plain_unformatted_string",
                "quote_wrapped_placeholder::pass::byte_printf_string",
            ),
        ),
        Unmutated(
            label="f-string logger message and positional format spec without nested braces",
            reason="static_string_text only inspects Expr::StringLiteral; '{:.2f}' has an empty root and no nested '{...}' in its spec",
            cases=(
                "unmatched_logger_placeholder::pass::f_string_message_not_flagged",
                "unmatched_logger_placeholder::pass::positional_with_format_spec",
            ),
        ),
    ),
)
