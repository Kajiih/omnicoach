//! Jujutsu revset expression parser and syntactic boundedness analysis.

use std::iter::Peekable;
use std::num::NonZeroUsize;
use std::vec::IntoIter;

const COUNT_CAPPED_FUNCTIONS: &[&str] = &["latest", "exactly"];

const DEPTH_BOUNDED_TRAVERSAL_FUNCTIONS: &[&str] = &["ancestors", "descendants", "first_ancestors"];

const UNBOUNDED_REVSET_FUNCTIONS: &[&str] = &[
    "all",
    "visible",
    "hidden",
    "immutable",
    "visible_heads",
    "mine",
    "author",
    "author_name",
    "author_email",
    "author_date",
    "committer",
    "committer_name",
    "committer_email",
    "committer_date",
    "description",
    "subject",
    "files",
    "diff_lines",
    "diff_lines_added",
    "diff_lines_removed",
    "conflicts",
    "divergent",
    "merges",
    "forks",
    "signed",
    "empty",
    "bookmarks",
    "remote_bookmarks",
    "tracked_remote_bookmarks",
    "untracked_remote_bookmarks",
    "tags",
    "remote_tags",
    "tracked_remote_tags",
    "untracked_remote_tags",
];

const UNBOUNDED_RANGE_LOWER_BOUNDS: &[&str] = &["root", "none"];

/// Returns `true` if the Jujutsu revset expression `input` is syntactically bounded without
/// requiring `-n` / `--limit`.
///
/// A revset is bounded when it resolves to point revisions, local parent/child steps (`@-`,
/// `@+`), two-sided ranges (`x..y`, `x::y`) whose lower bound is not `root()` or `none()`,
/// count-capped functions (`latest`, `exactly`), depth-bounded traversals (`ancestors(x, depth)`),
/// `reachable(srcs, domain)` over a bounded domain, or intersections (`&`) and differences (`~`)
/// constrained by a bounded set such as `mutable()`.
#[must_use]
pub fn is_bounded_jj_revset(input: &str) -> bool {
    parse_revset(input).is_some_and(|expression| expression.is_bounded())
}

/// Formats `input` intersected with `mutable()`, wrapping `input` in parentheses when it
/// contains a top-level union (`|`) so `& mutable()` constrains every union branch.
#[must_use]
pub fn intersect_revset_with_mutable(input: &str) -> String {
    let trimmed = input.trim();
    if matches!(parse_revset(trimmed), Some(RevsetExpr::Union(_, _))) {
        format!("({trimmed}) & mutable()")
    } else {
        format!("{trimmed} & mutable()")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RevsetExpr<'a> {
    Symbol,
    Step(Box<Self>),
    OpenRange,
    ClosedRange { from: Box<Self>, to: Box<Self> },
    Negate(Box<Self>),
    Intersection(Box<Self>, Box<Self>),
    Difference(Box<Self>, Box<Self>),
    Union(Box<Self>, Box<Self>),
    Call { name: &'a str, arguments: Vec<Self> },
}

impl RevsetExpr<'_> {
    fn is_bounded(&self) -> bool {
        match self {
            Self::Symbol => true,
            Self::Step(inner) => inner.is_bounded(),
            Self::OpenRange | Self::Negate(_) => false,
            Self::ClosedRange { from, to } => {
                !from.is_unbounded_range_origin() && from.is_bounded() && to.is_bounded()
            }
            Self::Intersection(left, right) => left.is_bounded() || right.is_bounded(),
            Self::Difference(left, _) => left.is_bounded(),
            Self::Union(left, right) => left.is_bounded() && right.is_bounded(),
            Self::Call { name, arguments } => is_call_bounded(name, arguments),
        }
    }

    fn is_unbounded_range_origin(&self) -> bool {
        match self {
            Self::Call { name, arguments } => {
                arguments.is_empty() && UNBOUNDED_RANGE_LOWER_BOUNDS.contains(name)
            }
            Self::Union(left, right) => {
                left.is_unbounded_range_origin() || right.is_unbounded_range_origin()
            }
            _ => false,
        }
    }
}

fn is_call_bounded(name: &str, arguments: &[RevsetExpr<'_>]) -> bool {
    if COUNT_CAPPED_FUNCTIONS.contains(&name) {
        return true;
    }
    if UNBOUNDED_REVSET_FUNCTIONS.contains(&name) {
        return false;
    }
    if DEPTH_BOUNDED_TRAVERSAL_FUNCTIONS.contains(&name) {
        return arguments.len() >= 2 && arguments[0].is_bounded();
    }
    if name == "reachable" {
        return arguments.get(1).is_some_and(RevsetExpr::is_bounded);
    }
    arguments.iter().all(RevsetExpr::is_bounded)
}

fn parse_revset(input: &str) -> Option<RevsetExpr<'_>> {
    let mut tokens = Vec::new();
    if !tokenize_into(input, &mut tokens) {
        return None;
    }
    let mut parser = RevsetParser {
        tokens: tokens.into_iter().peekable(),
    };
    let expression = parser.parse_union()?;
    parser.tokens.peek().is_none().then_some(expression)
}

struct RevsetParser<'a> {
    tokens: Peekable<IntoIter<Token<'a>>>,
}

impl<'a> RevsetParser<'a> {
    fn parse_union(&mut self) -> Option<RevsetExpr<'a>> {
        let mut left = self.parse_intersection_or_difference()?;
        while self.consume_if(Token::Pipe) {
            let right = self.parse_intersection_or_difference()?;
            left = RevsetExpr::Union(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_intersection_or_difference(&mut self) -> Option<RevsetExpr<'a>> {
        let mut left = self.parse_prefix()?;
        loop {
            if self.consume_if(Token::Ampersand) {
                let right = self.parse_prefix()?;
                left = RevsetExpr::Intersection(Box::new(left), Box::new(right));
            } else if self.consume_if(Token::Tilde) {
                let right = self.parse_prefix()?;
                left = RevsetExpr::Difference(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Some(left)
    }

    fn parse_prefix(&mut self) -> Option<RevsetExpr<'a>> {
        if self.consume_if(Token::Tilde) {
            let inner = self.parse_prefix()?;
            return Some(RevsetExpr::Negate(Box::new(inner)));
        }
        self.parse_range()
    }

    fn parse_range(&mut self) -> Option<RevsetExpr<'a>> {
        if self.consume_if(Token::RangeOp) {
            if self.can_start_postfix() {
                let _upper = self.parse_postfix()?;
            }
            return Some(RevsetExpr::OpenRange);
        }
        let lower = self.parse_postfix()?;
        if self.consume_if(Token::RangeOp) {
            if self.can_start_postfix() {
                let upper = self.parse_postfix()?;
                return Some(RevsetExpr::ClosedRange {
                    from: Box::new(lower),
                    to: Box::new(upper),
                });
            }
            return Some(RevsetExpr::OpenRange);
        }
        Some(lower)
    }

    fn parse_postfix(&mut self) -> Option<RevsetExpr<'a>> {
        let mut expression = self.parse_primary()?;
        while self.consume_if(Token::Step) {
            expression = RevsetExpr::Step(Box::new(expression));
        }
        Some(expression)
    }

    fn parse_primary(&mut self) -> Option<RevsetExpr<'a>> {
        let token = *self.tokens.peek()?;
        match token {
            Token::LParen => {
                self.tokens.next();
                let inner = self.parse_union()?;
                self.consume_if(Token::RParen).then_some(inner)
            }
            Token::Atom(name) => {
                self.tokens.next();
                if self.consume_if(Token::LParen) {
                    let mut arguments = Vec::new();
                    self.parse_call_arguments(&mut arguments)
                        .then_some(RevsetExpr::Call { name, arguments })
                } else {
                    Some(RevsetExpr::Symbol)
                }
            }
            _ => None,
        }
    }

    fn parse_call_arguments(&mut self, arguments: &mut Vec<RevsetExpr<'a>>) -> bool {
        if self.consume_if(Token::RParen) {
            return true;
        }
        loop {
            let Some(argument) = self.parse_union() else {
                return false;
            };
            arguments.push(argument);
            if self.consume_if(Token::Comma) {
                if self.consume_if(Token::RParen) {
                    return true;
                }
            } else {
                return self.consume_if(Token::RParen);
            }
        }
    }

    fn can_start_postfix(&mut self) -> bool {
        matches!(self.tokens.peek(), Some(Token::LParen | Token::Atom(_)))
    }

    fn consume_if(&mut self, expected: Token<'a>) -> bool {
        self.tokens.next_if_eq(&expected).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    LParen,
    RParen,
    Comma,
    Pipe,
    Ampersand,
    Tilde,
    Step,
    RangeOp,
    Atom(&'a str),
}

fn tokenize_into<'a>(input: &'a str, tokens: &mut Vec<Token<'a>>) -> bool {
    let mut rest = input.trim_ascii_start();
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("..").or_else(|| rest.strip_prefix("::")) {
            tokens.push(Token::RangeOp);
            rest = tail;
        } else if let Some(token) = single_byte_token(rest.as_bytes()[0]) {
            tokens.push(token);
            rest = &rest[1..];
        } else {
            let Some(len) = atom_len(rest) else {
                return false;
            };
            let (atom, tail) = rest.split_at(len.get());
            tokens.push(Token::Atom(atom));
            rest = tail;
        }
        rest = rest.trim_ascii_start();
    }
    true
}

const fn single_byte_token(byte: u8) -> Option<Token<'static>> {
    match byte {
        b'(' => Some(Token::LParen),
        b')' => Some(Token::RParen),
        b',' => Some(Token::Comma),
        b'|' => Some(Token::Pipe),
        b'&' => Some(Token::Ampersand),
        b'~' => Some(Token::Tilde),
        b'+' | b'-' => Some(Token::Step),
        _ => None,
    }
}

fn atom_len(input: &str) -> Option<NonZeroUsize> {
    let mut rest = input;
    while !rest.is_empty() && !is_atom_boundary(rest) {
        let mut chars = rest.chars();
        let character = chars.next()?;
        rest = if character == '"' || character == '\'' {
            skip_quoted(chars.as_str(), character)?
        } else {
            chars.as_str()
        };
    }
    NonZeroUsize::new(input.len() - rest.len())
}

fn skip_quoted(after_open_quote: &str, quote: char) -> Option<&str> {
    let mut chars = after_open_quote.chars();
    while let Some(character) = chars.next() {
        if character == '\\' && quote == '"' {
            chars.next()?;
            continue;
        }
        if character == quote {
            return Some(chars.as_str());
        }
    }
    None
}

fn is_atom_boundary(rest: &str) -> bool {
    let byte = rest.as_bytes()[0];
    byte.is_ascii_whitespace()
        || matches!(byte, b'(' | b')' | b',' | b'|' | b'&' | b'~')
        || rest.starts_with("..")
        || rest.starts_with("::")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::working_copy("@")]
    #[case::parent_step("@-")]
    #[case::grandparent_step("@--")]
    #[case::child_step("main+")]
    #[case::hyphenated_bookmark("feature-branch_1")]
    #[case::hyphenated_bookmark_parent("feature-branch-")]
    #[case::remote_bookmark_symbol("main@origin")]
    #[case::dotted_tag_symbol("v0.4.0")]
    #[case::single_quoted_open_range_symbol("'a..'")]
    #[case::single_quoted_trailing_backslash(r"'a\'")]
    #[case::double_quoted_escaped_open_range_symbol(r#""a\"..""#)]
    #[case::union_of_points("@ | main | kpqxywon")]
    #[case::closed_range("main..@")]
    #[case::closed_dag_range("main::@")]
    #[case::mutable_function("mutable()")]
    #[case::trunk_function("trunk()")]
    #[case::root_singleton("root()")]
    #[case::root_child("root()+")]
    #[case::ancestors_with_depth("ancestors(@, 5)")]
    #[case::descendants_with_depth("descendants(main, 3)")]
    #[case::first_ancestors_with_depth("first_ancestors(@, 2)")]
    #[case::latest_over_all("latest(all(), 5)")]
    #[case::exactly_over_mine("exactly(mine(), 1)")]
    #[case::reachable_in_mutable("reachable(@, mutable())")]
    #[case::heads_of_closed_range("heads(main..@)")]
    #[case::coalesce_of_points("coalesce(@-, main,)")]
    #[case::open_ancestors_intersected_with_mutable("::@ & mutable()")]
    #[case::open_descendants_intersected_with_mutable("main.. & mutable()")]
    #[case::bare_dag_range_intersected_with_mutable(":: & mutable()")]
    #[case::mutable_intersected_with_mine("mutable() & mine()")]
    #[case::parenthesized_union_intersected_with_mutable(
        "(mine() | author(\"alice\")) & mutable()"
    )]
    #[case::difference_from_mutable("mutable() ~ ::main")]
    #[case::pattern_with_colon_in_mutable("description(glob:\"fix..*\") & mutable()")]
    #[case::unquoted_glob_with_hyphen_wildcard("description(glob:fix-*) & mutable()")]
    #[case::unquoted_path_with_hyphen_wildcard("files(src/foo-*) & mutable()")]
    fn bounded_revsets_return_true(#[case] revset: &str) {
        assert!(is_bounded_jj_revset(revset));
    }

    #[rstest]
    #[case::empty_string("")]
    #[case::unclosed_paren("(@")]
    #[case::unclosed_quote("\"unterminated")]
    #[case::unclosed_quote_trailing_backslash("\"unterminated\\")]
    #[case::trailing_tokens("@ @")]
    #[case::bare_dag_range("::")]
    #[case::bare_range("..")]
    #[case::open_ancestors("::@")]
    #[case::open_predecessors("..@")]
    #[case::open_descendants("main::")]
    #[case::open_range_from_base("main..")]
    #[case::closed_range_from_root("root()..@")]
    #[case::closed_dag_range_from_root("root()::@")]
    #[case::closed_dag_range_from_union_with_root("(root() | main)::@")]
    #[case::closed_range_from_none("none()..@")]
    #[case::closed_range_with_unbounded_lower("(::main)..@")]
    #[case::closed_range_with_unbounded_upper("main..(all())")]
    #[case::prefix_negation("~@")]
    #[case::prefix_negation_of_mutable("~mutable()")]
    #[case::all_function("all()")]
    #[case::mine_function("mine()")]
    #[case::author_function("author(\"alice\")")]
    #[case::description_function("description(\"fix\")")]
    #[case::files_function("files(\"src\")")]
    #[case::conflicts_function("conflicts()")]
    #[case::bookmarks_function("bookmarks()")]
    #[case::remote_bookmarks_function("remote_bookmarks(remote=\"origin\")")]
    #[case::tags_function("tags()")]
    #[case::one_arg_ancestors("ancestors(@)")]
    #[case::two_arg_ancestors_over_unbounded("ancestors(all(), 5)")]
    #[case::one_arg_descendants("descendants(main)")]
    #[case::one_arg_first_ancestors("first_ancestors(@)")]
    #[case::unbounded_step("all()-")]
    #[case::heads_of_all("heads(all())")]
    #[case::reachable_in_unbounded_domain("reachable(@, all())")]
    #[case::reachable_missing_domain("reachable(@)")]
    #[case::intersection_of_two_unbounded("mine() & author(\"alice\")")]
    #[case::difference_from_unbounded("all() ~ mutable()")]
    #[case::union_with_unbounded_left("::@ | @")]
    #[case::union_with_unbounded_right("@ | mine()")]
    fn unbounded_or_malformed_revsets_return_false(#[case] revset: &str) {
        assert!(!is_bounded_jj_revset(revset));
    }

    #[rstest]
    #[case::simple_range("::@", "::@ & mutable()")]
    #[case::top_level_union("mine() | conflicts()", "(mine() | conflicts()) & mutable()")]
    fn format_mutable_intersection_wraps_top_level_unions(
        #[case] input: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(intersect_revset_with_mutable(input), expected);
    }
}
