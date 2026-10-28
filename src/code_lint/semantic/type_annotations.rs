//! Structural pattern matching for banned type annotations (`adhoc-type-annotation`).
//!
//! Parses `"<pattern>"` and `"<pattern> => <replacement>"` entries from a `ListOption` into
//! [`TypePattern`]s and matches them against [`ast::collect_type_annotation_candidates`].

use crate::code_lint::ast::{self, AstNode, ParsedFile, TypeShape};
use std::collections::HashSet;

const DEFAULT_REPLACEMENT: &str = "a dedicated domain type";
const TUPLE_STD_NAME: &str = "tuple";
const SET_TYPING_NAME: &str = "Set";

/// Standard-library Python type constructors (in `builtins`, `typing`, `typing_extensions`, or
/// `collections.abc`) and Rust standard-prelude type constructors whose bare pattern name will not
/// match an imported symbol from a third-party or domain module.
const STD_TYPE_CONSTRUCTORS: &[&str] = &[
    "list",
    "List",
    "dict",
    "Dict",
    "set",
    SET_TYPING_NAME,
    "frozenset",
    "FrozenSet",
    TUPLE_STD_NAME,
    "Tuple",
    "Sequence",
    "MutableSequence",
    "Mapping",
    "MutableMapping",
    "AbstractSet",
    "MutableSet",
    "Collection",
    "Iterable",
    "Iterator",
    "Reversible",
    "Container",
    "AsyncIterable",
    "AsyncIterator",
    "Generator",
    "AsyncGenerator",
    "Awaitable",
    "Coroutine",
    "Callable",
    "Optional",
    "Union",
    "Annotated",
    "ClassVar",
    "Final",
    "Literal",
    "Any",
    "object",
    "BaseException",
    "Exception",
    "int",
    "float",
    "str",
    "bytes",
    "bool",
    "Option",
    "Result",
    "Vec",
    "Box",
    "Rc",
    "Arc",
    "Cow",
    "String",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "f32",
    "f64",
    "char",
];

/// A matched type annotation subnode together with its enclosing full annotation text and its
/// formatted `{replacement}` placeholder text.
pub struct BannedTypeAnnotationMatch<'a> {
    /// The matched type annotation (or nested type argument) AST node.
    pub node: AstNode<'a>,
    /// The source text of `node` (`{token}`).
    pub token: String,
    /// The source text of the full enclosing parameter, return, or attribute/field annotation
    /// (`{expression}`).
    pub expression: String,
    /// The formatted replacement text (`"`Dimensions`"` or `"a dedicated domain type"`).
    pub replacement: String,
}

/// Finds all type annotation nodes in `file` matching any entry in `banned`.
///
/// Each entry has the form `"<pattern>"` or `"<pattern> => <replacement>"`. Patterns are parsed
/// structurally (`_`, `...`, `A | B`, `Optional[A]`, `Union[A, B]`, `tuple[A, B]`, `(A, B)`,
/// `&T`, `&mut T`, and generic subscripts `Base[A, B]` / `Base<A, B>`) and matched in source
/// order against [`ast::collect_type_annotation_candidates`]. At most one match is emitted per
/// AST node span.
#[must_use]
pub fn find_banned_type_annotations<'a, S: std::hash::BuildHasher>(
    file: &'a ParsedFile,
    banned: &HashSet<String, S>,
) -> Vec<BannedTypeAnnotationMatch<'a>> {
    let mut rules: Vec<ParsedBannedRule> = banned
        .iter()
        .filter_map(|entry| parse_banned_entry(entry))
        .collect();
    if rules.is_empty() {
        return Vec::new();
    }
    rules.sort_by(|left, right| left.raw_pattern.cmp(&right.raw_pattern));

    let mut matches = Vec::new();
    for candidate in ast::collect_type_annotation_candidates(file) {
        if let Some(rule) = rules
            .iter()
            .find(|rule| matches_pattern(&rule.pattern, &candidate.shape))
        {
            let token = candidate.node.text().into_owned();
            matches.push(BannedTypeAnnotationMatch {
                node: candidate.node,
                token,
                expression: candidate.expression,
                replacement: rule.replacement.clone(),
            });
        }
    }

    matches.sort_by_key(|matched| matched.node.span());
    matches.dedup_by_key(|matched| matched.node.span());
    matches
}

/// A parsed `"<pattern> [=> <replacement>]"` configuration entry.
struct ParsedBannedRule {
    /// Original trimmed `<pattern>` string for deterministic sorting.
    raw_pattern: String,
    /// Structural type pattern.
    pattern: TypePattern,
    /// Formatted `{replacement}` text.
    replacement: String,
}

/// Structural type pattern parsed from a configuration entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypePattern {
    /// Single-slot wildcard `_`.
    Wildcard,
    /// Ellipsis `...` (matches literal `...` or trailing remaining arguments).
    Ellipsis,
    /// Python `None`.
    None,
    /// Order-independent union (`A | B`, `Optional[A]`, `Union[A, B]`).
    Union(Vec<Self>),
    /// Fixed or prefix tuple (`tuple[A, B]`, `Tuple[A, B]`, `(A, B)`).
    Tuple(Vec<Self>),
    /// Rust reference (`&T` or `&mut T`).
    Reference { mutable: bool, inner: Box<Self> },
    /// Named or generic type constructor (`Width`, `Iterator[Dimensions]`, `Option<Bound>`).
    Named {
        path: String,
        terminal: String,
        is_qualified: bool,
        arguments: Vec<Self>,
    },
}

/// Parses a `"<pattern>"` or `"<pattern> => <replacement>"` entry.
fn parse_banned_entry(entry: &str) -> Option<ParsedBannedRule> {
    let (raw_pattern, raw_replacement) = match entry.split_once("=>") {
        Some((pattern, replacement)) => (pattern.trim(), Some(replacement.trim())),
        None => (entry.trim(), None),
    };
    if raw_pattern.is_empty() {
        return None;
    }
    let pattern = parse_type_pattern(raw_pattern)?;
    let replacement = raw_replacement
        .map(|rep| rep.trim_matches('`').trim())
        .filter(|rep| !rep.is_empty())
        .map_or_else(|| DEFAULT_REPLACEMENT.to_owned(), |rep| format!("`{rep}`"));
    Some(ParsedBannedRule {
        raw_pattern: raw_pattern.to_owned(),
        pattern,
        replacement,
    })
}

/// Parses a type pattern string into a [`TypePattern`].
fn parse_type_pattern(raw: &str) -> Option<TypePattern> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let union_parts = split_top_level(trimmed, '|');
    if union_parts.len() > 1 {
        let mut branches = Vec::new();
        for part in union_parts {
            let parsed = parse_type_pattern(part)?;
            push_union_pattern_branch(&mut branches, parsed);
        }
        return Some(TypePattern::Union(branches));
    }

    match trimmed {
        "_" => return Some(TypePattern::Wildcard),
        "..." => return Some(TypePattern::Ellipsis),
        "None" => return Some(TypePattern::None),
        _ => {}
    }

    if let Some(rest) = trimmed.strip_prefix('&') {
        let rest = rest.trim_start();
        let (mutable, inner_text) = if let Some(after_mut) = rest.strip_prefix("mut")
            && after_mut.chars().next().is_some_and(char::is_whitespace)
        {
            (true, after_mut.trim_start())
        } else {
            (false, rest)
        };
        let inner = parse_type_pattern(inner_text)?;
        return Some(TypePattern::Reference {
            mutable,
            inner: Box::new(inner),
        });
    }

    if let Some(inside) = trimmed
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        && is_balanced_delimiters(inside)
    {
        let elements = parse_comma_separated_patterns(inside)?;
        return Some(TypePattern::Tuple(elements));
    }

    let SplitGeneric { base, arguments } = split_generic_base_and_args(trimmed)?;
    let base_normalized = normalize_pattern_path(base)?;
    let parsed_arguments = match arguments {
        Some(inside) => parse_comma_separated_patterns(inside)?,
        None => Vec::new(),
    };

    let PatternPathTerminal {
        terminal,
        is_qualified,
    } = split_terminal(&base_normalized);

    if let Some(std_constructor) = canonical_std_constructor(&base_normalized, &terminal) {
        match std_constructor {
            "Optional" if !parsed_arguments.is_empty() => {
                let mut branches = Vec::new();
                for argument in parsed_arguments {
                    push_union_pattern_branch(&mut branches, argument);
                }
                push_union_pattern_branch(&mut branches, TypePattern::None);
                return Some(TypePattern::Union(branches));
            }
            "Union" if !parsed_arguments.is_empty() => {
                let mut branches = Vec::new();
                for argument in parsed_arguments {
                    push_union_pattern_branch(&mut branches, argument);
                }
                return Some(TypePattern::Union(branches));
            }
            TUPLE_STD_NAME if arguments.is_some() => {
                return Some(TypePattern::Tuple(parsed_arguments));
            }
            _ => {}
        }
    }

    Some(TypePattern::Named {
        path: base_normalized,
        terminal,
        is_qualified,
        arguments: parsed_arguments,
    })
}

/// Flattens nested [`TypePattern::Union`] branches and deduplicates equal branches.
fn push_union_pattern_branch(branches: &mut Vec<TypePattern>, branch: TypePattern) {
    if let TypePattern::Union(nested) = branch {
        for item in nested {
            push_union_pattern_branch(branches, item);
        }
    } else if !branches.contains(&branch) {
        branches.push(branch);
    }
}

/// Parses a comma-separated list of [`TypePattern`]s inside `(...)`, `[...]`, or `<...>`.
// `None` means a syntax error in an element, whereas `Some(vec![])` is a valid empty `()` or `[]`.
fn parse_comma_separated_patterns(inside: &str) -> Option<Vec<TypePattern>> {
    let trimmed = inside.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for part in split_top_level(trimmed, ',') {
        let part_trimmed = part.trim();
        if part_trimmed.is_empty() {
            continue;
        }
        out.push(parse_type_pattern(part_trimmed)?);
    }
    Some(out)
}

/// Splits `text` on `delimiter` at top-level bracket/paren/angle depth `0`.
fn split_top_level(text: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '[' | '(' | '<' => depth += 1,
            ']' | ')' | '>' => depth = depth.saturating_sub(1),
            c if c == delimiter && depth == 0 => {
                parts.push(&text[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// A constructor path and its optional bracketed or angle-bracketed type argument text.
struct SplitGeneric<'a> {
    base: &'a str,
    arguments: Option<&'a str>,
}

/// Splits `Base[A, B]` or `Base<A, B>` into its base and optional type argument text.
fn split_generic_base_and_args(text: &str) -> Option<SplitGeneric<'_>> {
    let open_pos = text.find(['[', '<']);
    let Some(open_idx) = open_pos else {
        return Some(SplitGeneric {
            base: text,
            arguments: None,
        });
    };
    let open_char = text[open_idx..].chars().next()?;
    let close_char = if open_char == '[' { ']' } else { '>' };
    let rest = text.strip_suffix(close_char)?;
    let base = &rest[..open_idx];
    let inside = &rest[open_idx + open_char.len_utf8()..];
    if !is_balanced_delimiters(inside) {
        return None;
    }
    Some(SplitGeneric {
        base,
        arguments: Some(inside),
    })
}

/// Returns true if brackets, parentheses, and angle brackets in `text` never drop below depth `0`
/// and end at depth `0`.
fn is_balanced_delimiters(text: &str) -> bool {
    let mut depth = 0usize;
    for character in text.chars() {
        match character {
            '[' | '(' | '<' => depth += 1,
            ']' | ')' | '>' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    depth == 0
}

/// Strips whitespace and any leading `::` from a constructor path and validates its characters.
fn normalize_pattern_path(raw: &str) -> Option<String> {
    let compact: String = raw
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let stripped = compact.strip_prefix("::").unwrap_or(&compact);
    if stripped.is_empty() {
        return None;
    }
    let valid = stripped
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, '_' | '.' | ':'));
    valid.then(|| stripped.to_owned())
}

/// The terminal segment of a normalized pattern path and whether the path is qualified.
struct PatternPathTerminal {
    terminal: String,
    is_qualified: bool,
}

/// Extracts [`PatternPathTerminal`] from a normalized path (`"anyio.Event"` -> `"Event"`, qualified).
fn split_terminal(path: &str) -> PatternPathTerminal {
    if let Some((_, terminal)) = path.rsplit_once("::") {
        PatternPathTerminal {
            terminal: terminal.to_owned(),
            is_qualified: true,
        }
    } else if let Some((_, terminal)) = path.rsplit_once('.') {
        PatternPathTerminal {
            terminal: terminal.to_owned(),
            is_qualified: true,
        }
    } else {
        PatternPathTerminal {
            terminal: path.to_owned(),
            is_qualified: false,
        }
    }
}

/// Returns true if `pattern` structurally matches `shape`.
fn matches_pattern(pattern: &TypePattern, shape: &TypeShape) -> bool {
    match (pattern, shape) {
        (TypePattern::Wildcard, _)
        | (TypePattern::Ellipsis, TypeShape::Ellipsis)
        | (TypePattern::None, TypeShape::None) => true,
        (TypePattern::Union(pat_branches), TypeShape::Union(target_branches)) => {
            matches_union_branches(pat_branches, target_branches)
        }
        (TypePattern::Tuple(pat_args), TypeShape::Tuple(target_args)) => {
            matches_argument_list(pat_args, target_args)
        }
        (
            TypePattern::Reference {
                mutable: pat_mut,
                inner: pat_inner,
            },
            TypeShape::Reference {
                mutable: target_mut,
                inner: target_inner,
            },
        ) => pat_mut == target_mut && matches_pattern(pat_inner, target_inner),
        (
            TypePattern::Named {
                path,
                terminal,
                is_qualified,
                arguments: pat_args,
            },
            TypeShape::Named {
                resolved_path,
                terminal: target_terminal,
                arguments: target_args,
            },
        ) => {
            matches_constructor(
                path,
                terminal,
                *is_qualified,
                resolved_path.as_deref(),
                target_terminal,
            ) && matches_argument_list(pat_args, target_args)
        }
        _ => false,
    }
}

/// Returns true if `pat_branches` and `target_branches` have the same length and can be paired 1-to-1.
fn matches_union_branches(pat_branches: &[TypePattern], target_branches: &[TypeShape]) -> bool {
    if pat_branches.len() != target_branches.len() {
        return false;
    }
    let mut used = vec![false; target_branches.len()];
    pair_union_branches(pat_branches, target_branches, &mut used)
}

/// Backtracking 1-to-1 matcher for union branches.
fn pair_union_branches(
    pat_branches: &[TypePattern],
    target_branches: &[TypeShape],
    used: &mut [bool],
) -> bool {
    let Some((first_pat, rest_pats)) = pat_branches.split_first() else {
        return true;
    };
    for (idx, target) in target_branches.iter().enumerate() {
        if !used[idx] && matches_pattern(first_pat, target) {
            used[idx] = true;
            if pair_union_branches(rest_pats, target_branches, used) {
                return true;
            }
            used[idx] = false;
        }
    }
    false
}

/// Matches a pattern's type argument list against a target's type argument list, supporting a
/// trailing `...` wildcard (`tuple[A, ...]`, `tuple[Iterator[Dimensions], ...]`).
fn matches_argument_list(pat_args: &[TypePattern], target_args: &[TypeShape]) -> bool {
    if let Some((TypePattern::Ellipsis, prefix)) = pat_args.split_last() {
        return target_args.len() >= prefix.len()
            && prefix
                .iter()
                .zip(target_args)
                .all(|(pat, target)| matches_pattern(pat, target));
    }
    pat_args.len() == target_args.len()
        && pat_args
            .iter()
            .zip(target_args)
            .all(|(pat, target)| matches_pattern(pat, target))
}

/// Returns true if a [`TypePattern::Named`] constructor matches a [`TypeShape::Named`] constructor.
fn matches_constructor(
    pat_path: &str,
    pat_terminal: &str,
    pat_is_qualified: bool,
    target_resolved_path: Option<&str>,
    target_terminal: &str,
) -> bool {
    if pat_is_qualified {
        return target_resolved_path.is_some_and(|resolved| {
            resolved == pat_path
                || (pat_path.contains("::")
                    && resolved.contains("::")
                    && strip_rust_std_crate(pat_path) == strip_rust_std_crate(resolved))
                || canonical_std_constructor(pat_path, pat_terminal).is_some_and(|pat_std| {
                    canonical_std_constructor(resolved, target_terminal) == Some(pat_std)
                })
        });
    }
    if let Some(pat_std) = canonical_std_constructor(pat_path, pat_terminal) {
        return target_resolved_path.is_some_and(|resolved| {
            canonical_std_constructor(resolved, target_terminal) == Some(pat_std)
        });
    }
    pat_terminal == target_terminal
}

/// Maps a standard-library Python or Rust prelude type constructor `(path, terminal)` to its
/// canonical constructor name (`"typing.List"` -> `"list"`, `"collections.abc.Set"` ->
/// `"AbstractSet"`, `"std::option::Option"` -> `"Option"`).
fn canonical_std_constructor<'a>(path: &'a str, terminal: &'a str) -> Option<&'a str> {
    if path == terminal {
        return STD_TYPE_CONSTRUCTORS
            .contains(&terminal)
            .then(|| normalize_python_typing_alias(terminal));
    }
    if let Some(prefix) = path.strip_suffix(terminal) {
        if matches!(prefix, "typing." | "typing_extensions." | "builtins.") {
            return Some(normalize_python_typing_alias(terminal));
        }
        if prefix == "collections.abc." {
            return Some(if terminal == SET_TYPING_NAME {
                "AbstractSet"
            } else {
                terminal
            });
        }
    }
    match strip_rust_std_crate(path) {
        "option::Option" => Some("Option"),
        "result::Result" => Some("Result"),
        "vec::Vec" => Some("Vec"),
        "boxed::Box" => Some("Box"),
        "rc::Rc" => Some("Rc"),
        "sync::Arc" => Some("Arc"),
        "borrow::Cow" => Some("Cow"),
        "string::String" => Some("String"),
        _ => None,
    }
}

/// Normalizes Python `typing` builtin-collection aliases (`List`, `Dict`, `Set`, `FrozenSet`,
/// `Tuple`) to their lowercase `builtins` equivalents.
fn normalize_python_typing_alias(terminal: &str) -> &str {
    match terminal {
        "List" => "list",
        "Dict" => "dict",
        SET_TYPING_NAME => "set",
        "FrozenSet" => "frozenset",
        "Tuple" => TUPLE_STD_NAME,
        other => other,
    }
}

/// Strips a leading `std::`, `core::`, or `alloc::` crate root from a Rust path.
fn strip_rust_std_crate(path: &str) -> &str {
    path.strip_prefix("std::")
        .or_else(|| path.strip_prefix("core::"))
        .or_else(|| path.strip_prefix("alloc::"))
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Language;
    use rstest::rstest;

    #[test]
    fn empty_or_invalid_banned_entries_produce_no_matches() {
        let file = ParsedFile::new(
            "def f(x: tuple[Width, Height]) -> None: pass",
            Language::Python,
        );
        let empty = HashSet::new();
        let invalid = HashSet::from([
            String::new(),
            " => Dimensions".to_owned(),
            "tuple[Width".to_owned(),
            "tuple[Width, ] ]".to_owned(),
            "::".to_owned(),
            "bad$name".to_owned(),
        ]);
        assert!(find_banned_type_annotations(&file, &empty).is_empty());
        assert!(find_banned_type_annotations(&file, &invalid).is_empty());
    }

    #[rstest]
    #[case::explicit_replacement(
        "tuple[Width, Height] => Dimensions",
        "def f(x: tuple[Width, Height]) -> None: pass",
        Language::Python,
        "tuple[Width, Height]",
        "tuple[Width, Height]",
        "`Dimensions`"
    )]
    #[case::backticked_replacement_is_not_double_wrapped(
        "tuple[Width, Height] => `Dimensions`",
        "def f(x: Annotated[tuple[Width, Height], 'px']) -> None: pass",
        Language::Python,
        "tuple[Width, Height]",
        "Annotated[tuple[Width, Height], 'px']",
        "`Dimensions`"
    )]
    #[case::default_replacement(
        "tuple[Width, Height]",
        "def f(x: tuple[Width, Height]) -> None: pass",
        Language::Python,
        "tuple[Width, Height]",
        "tuple[Width, Height]",
        "a dedicated domain type"
    )]
    #[case::python_list_matches_typing_list(
        "list[Width] => Widths",
        "import typing\ndef f(x: typing.List[Width]) -> None: pass",
        Language::Python,
        "typing.List[Width]",
        "typing.List[Width]",
        "`Widths`"
    )]
    #[case::nested_type_argument(
        "(Width, Height) => Dimensions",
        "fn f(items: &[&mut [(Width, Height); 4]]) {}",
        Language::Rust,
        "(Width, Height)",
        "&[&mut [(Width, Height); 4]]",
        "`Dimensions`"
    )]
    #[case::rust_reference_pattern(
        "&mut (Width, Height) => Dimensions",
        "fn f(dim: (&mut (Width, Height))) {}",
        Language::Rust,
        "&mut (Width, Height)",
        "(&mut (Width, Height))",
        "`Dimensions`"
    )]
    #[case::optional_and_union_in_pattern(
        "Optional[Union[Width, Height]] => Dimension",
        "def f(dim: Height | None | Width) -> None: pass",
        Language::Python,
        "Height | None | Width",
        "Height | None | Width",
        "`Dimension`"
    )]
    #[case::qualified_std_and_rust_paths(
        "std::option::Option<sync::Mutex<i32>> => SharedCounter",
        "use std::sync::Mutex;\nfn f(c: core::option::Option<Mutex<i32>>) {}",
        Language::Rust,
        "core::option::Option<Mutex<i32>>",
        "core::option::Option<Mutex<i32>>",
        "`SharedCounter`"
    )]
    fn find_banned_type_annotations_formats_token_expression_and_replacement(
        #[case] entry: &str,
        #[case] source: &str,
        #[case] language: Language,
        #[case] expected_token: &str,
        #[case] expected_expression: &str,
        #[case] expected_replacement: &str,
    ) {
        let file = ParsedFile::new(source, language);
        let banned = HashSet::from([entry.to_owned()]);
        let matches = find_banned_type_annotations(&file, &banned);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].token, expected_token);
        assert_eq!(matches[0].expression, expected_expression);
        assert_eq!(matches[0].replacement, expected_replacement);
    }

    #[rstest]
    #[case::typing_set_does_not_match_collections_abc_set(
        "typing.Set[int]",
        "from collections.abc import Set\ndef f(x: Set[int]) -> None: pass",
        Language::Python
    )]
    #[case::rust_rc_weak_does_not_match_sync_weak(
        "std::rc::Weak<i32>",
        "use std::sync::Weak;\nfn f(x: Weak<i32>) {}",
        Language::Rust
    )]
    #[case::qualified_rust_std_does_not_match_unimported_bare(
        "std::sync::Mutex<i32>",
        "fn f(x: Mutex<i32>) {}",
        Language::Rust
    )]
    fn distinct_namespaces_do_not_collide(
        #[case] entry: &str,
        #[case] source: &str,
        #[case] language: Language,
    ) {
        let file = ParsedFile::new(source, language);
        let banned = HashSet::from([entry.to_owned()]);
        assert!(find_banned_type_annotations(&file, &banned).is_empty());
    }
}
