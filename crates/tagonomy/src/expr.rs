//! Boolean tag filter expressions, infix string parser, and static query checks.

use std::ops::{BitAnd, BitOr, Not, Range};

use fixedbitset::FixedBitSet;
use thiserror::Error;

use crate::tag::TagGroup;
use crate::tag_set::TagSet;
use crate::vocabulary::{TagId, TagLookupError, Vocabulary};

/// A boolean filter expression over a [`TagSet`], combining tags with `AND`, `OR`, and `NOT`.
///
/// Build expressions from user input with [`Expr::parse`], or in Rust with `&`, `|`, `!`,
/// [`Expr::all`], and [`Expr::any`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    /// Matches every [`TagSet`].
    All,
    /// Matches no [`TagSet`].
    None,
    /// Matches items that carry this tag directly or via implication (for example, `os` matches an
    /// item tagged `ebpf`).
    Tag(TagId),
    /// Matches only items that list this tag directly, not merely by implication (for example,
    /// `ExplicitTag(os)` matches `[os]` but not `[ebpf]`).
    ExplicitTag(TagId),
    /// Negation (`!expr`): matches when the inner expression does not.
    Not(Box<Self>),
    /// Logical `AND`: matches when every child matches (matches everything when empty).
    And(Vec<Self>),
    /// Logical `OR`: matches when at least one child matches (matches nothing when empty).
    Or(Vec<Self>),
}

impl Expr {
    /// Combines `expressions` with `AND`, flattening nested `And`s ([`Self::All`] when empty).
    #[must_use]
    pub fn all(expressions: impl IntoIterator<Item = Self>) -> Self {
        let mut flat = Vec::new();
        for expression in expressions {
            match expression {
                Self::All => {}
                Self::And(children) => flat.extend(children),
                other => flat.push(other),
            }
        }
        match flat.len() {
            0 => Self::All,
            1 => flat.pop().unwrap_or(Self::All),
            _ => Self::And(flat),
        }
    }

    /// Combines `expressions` with `OR`, flattening nested `Or`s ([`Self::None`] when empty).
    #[must_use]
    pub fn any(expressions: impl IntoIterator<Item = Self>) -> Self {
        let mut flat = Vec::new();
        for expression in expressions {
            match expression {
                Self::None => {}
                Self::Or(children) => flat.extend(children),
                other => flat.push(other),
            }
        }
        match flat.len() {
            0 => Self::None,
            1 => flat.pop().unwrap_or(Self::None),
            _ => Self::Or(flat),
        }
    }

    /// Combines `self` and `right` with `AND`.
    #[must_use]
    pub fn and(self, right: Self) -> Self {
        Self::all([self, right])
    }

    /// Combines `self` and `right` with `OR`.
    #[must_use]
    pub fn or(self, right: Self) -> Self {
        Self::any([self, right])
    }

    /// Evaluates whether `tags` matches this expression.
    #[must_use]
    pub fn matches(&self, tags: &TagSet) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Tag(id) => tags.carries(*id),
            Self::ExplicitTag(id) => tags.carries_explicit(*id),
            Self::Not(inner) => !inner.matches(tags),
            Self::And(children) => children.iter().all(|child| child.matches(tags)),
            Self::Or(children) => children.iter().any(|child| child.matches(tags)),
        }
    }

    /// Parses a boolean tag query such as `"(linux | kernel) & !slow"` against `vocabulary`.
    ///
    /// Supported operators, from highest to lowest precedence:
    ///
    /// 1. Parentheses: `( ... )`
    /// 2. Negation: `!` or `not`
    /// 3. `AND`: `&`, `&&`, or `and`
    /// 4. `OR`: `|`, `||`, or `or`
    ///
    /// Keyword operators (`not`, `and`, `or`) are matched ASCII case-insensitively. Any other run
    /// of non-whitespace characters without `(`, `)`, `!`, `&`, or `|` is read as a tag label or
    /// alias (so `kebab-case` and `path/style` tags work without quotes).
    ///
    /// # Errors
    ///
    /// Returns [`ExprParseError`] with the UTF-8 byte span of the invalid token or end of input.
    pub fn parse(vocabulary: &Vocabulary, input: &str) -> Result<Self, ExprParseError> {
        let tokens = tokenize(input);
        if tokens.is_empty() {
            return Err(ExprParseError {
                span: 0..input.len(),
                kind: ExprParseErrorKind::Empty,
            });
        }
        let mut parser = Parser {
            remaining: &tokens,
            vocabulary,
            end_offset: input.len(),
        };
        let expression = parser.parse_or()?;
        if let Some((trailing, _)) = parser.remaining.split_first() {
            return Err(ExprParseError {
                span: trailing.span.clone(),
                kind: ExprParseErrorKind::UnexpectedToken {
                    token: trailing.text.to_owned(),
                },
            });
        }
        Ok(expression)
    }

    /// Checks `self` for queries that can never match, always match, or repeat an already-implied
    /// tag under `vocabulary`.
    ///
    /// Negations are pushed inward first (for example, `!(fast & slow)` becomes `!fast | !slow`)
    /// so negated expressions receive the same warnings as positive ones.
    #[must_use]
    pub fn lint(&self, vocabulary: &Vocabulary) -> Vec<ExprWarning> {
        match to_nnf(self, false) {
            Self::All => vec![ExprWarning::Tautology(TautologyKind::ConstantAll)],
            Self::None => vec![ExprWarning::Contradiction(ContradictionKind::ConstantNone)],
            normalized => {
                let mut warnings = Vec::new();
                lint_nnf(&normalized, vocabulary, &mut warnings);
                warnings
            }
        }
    }
}

impl BitAnd for Expr {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        self.and(rhs)
    }
}

impl BitOr for Expr {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.or(rhs)
    }
}

impl Not for Expr {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            Self::All => Self::None,
            Self::None => Self::All,
            other => Self::Not(Box::new(other)),
        }
    }
}

/// Syntax or label-lookup failure when parsing an [`Expr`] string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind}")]
pub struct ExprParseError {
    /// UTF-8 byte span in the input string.
    pub span: Range<usize>,
    /// Specific parse failure reason.
    pub kind: ExprParseErrorKind,
}

/// Specific reason an [`Expr::parse`] call rejected its input.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExprParseErrorKind {
    /// The input string contained no tokens.
    #[error("expression is empty")]
    Empty,
    /// An operator or closing parenthesis appeared where an operand or operator was expected.
    #[error("unexpected token `{token}`")]
    UnexpectedToken {
        /// The offending token text.
        token: String,
    },
    /// The input ended right after an operator or opening parenthesis.
    #[error("unexpected end of expression")]
    UnexpectedEnd,
    /// An opening `(` had no matching `)`.
    #[error("unclosed `(`")]
    UnclosedParen,
    /// A bare identifier named a [`TagGroup`] instead of a tag.
    #[error("`{label}` names group `{}`, not a tag", group.name)]
    Group {
        /// The identifier as written.
        label: String,
        /// The matched tag group.
        group: &'static TagGroup,
    },
    /// A bare identifier matched no canonical tag or alias in the vocabulary.
    #[error("unknown tag `{label}`")]
    UnknownTag {
        /// The unrecognized identifier as written.
        label: String,
    },
}

/// A query-hygiene warning reported by [`Expr::lint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprWarning {
    /// An `AND` clause (or [`Expr::None`]) can never match any valid [`TagSet`] (for example,
    /// `ebpf & !os` or `fast & slow`).
    Contradiction(ContradictionKind),
    /// An `OR` clause (or [`Expr::All`]) matches every valid [`TagSet`] (for example,
    /// `os | !ebpf` or `!fast | !slow`).
    Tautology(TautologyKind),
    /// A tag in an `AND` or `OR` clause is already covered by another tag in the same clause (for
    /// example, `os` in `linux & os`, or `linux` in `os | linux`).
    Redundant {
        /// The redundant tag.
        redundant: TagId,
        /// The other tag in the clause that already covers it.
        covered_by: TagId,
    },
}

/// Why an `AND` clause in an [`Expr`] can never match any valid [`TagSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContradictionKind {
    /// [`Expr::None`] (or `!Expr::All`) appeared in an `AND` clause or as the whole query.
    ConstantNone,
    /// A required tag implies a negated tag in the same `AND` clause (for example, `ebpf & !os` or
    /// `linux & !linux`).
    ImpliesNegated {
        /// The required tag.
        positive: TagId,
        /// The negated implied tag.
        negated: TagId,
    },
    /// Requiring both a narrower tag (`ebpf`) and an explicit broader tag (`ExplicitTag(os)`) can
    /// never match, because [`TagSet`] rejects listing a broader tag alongside a narrower one that
    /// already implies it.
    Antichain {
        /// The strictly narrower tag.
        narrower: TagId,
        /// The broader tag required explicitly.
        explicit_broader: TagId,
    },
    /// Required tags in an `AND` clause imply two conflicting tags from an exclusive group (for
    /// example, `fast & slow`, or `ebpf & windows` when `ebpf` implies `linux`).
    ExclusiveGroup {
        /// The exclusive group.
        group: &'static TagGroup,
        /// Two conflicting tags in `group`.
        conflicting: [TagId; 2],
    },
}

impl ContradictionKind {
    const fn into_tautology(self) -> TautologyKind {
        match self {
            Self::ConstantNone => TautologyKind::ConstantAll,
            Self::ImpliesNegated { positive, negated } => TautologyKind::ImpliesNegated {
                positive: negated,
                negated: positive,
            },
            Self::Antichain {
                narrower,
                explicit_broader,
            } => TautologyKind::Antichain {
                narrower,
                explicit_broader,
            },
            Self::ExclusiveGroup { group, conflicting } => {
                TautologyKind::ExclusiveGroup { group, conflicting }
            }
        }
    }
}

/// Why an `OR` clause in an [`Expr`] matches every valid [`TagSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TautologyKind {
    /// [`Expr::All`] (or `!Expr::None`) appeared in an `OR` clause or as the whole query.
    ConstantAll,
    /// A negated narrower tag appears alongside a broader required tag in an `OR` clause (for
    /// example, `os | !ebpf` or `linux | !linux`): every item either has `ebpf` (and therefore
    /// `os`) or does not.
    ImpliesNegated {
        /// The required broader tag.
        positive: TagId,
        /// The negated narrower tag.
        negated: TagId,
    },
    /// `!ebpf | !ExplicitTag(os)` matches every valid [`TagSet`], because no valid set can list
    /// both a narrower tag and an explicit broader tag it already implies.
    Antichain {
        /// The strictly narrower tag.
        narrower: TagId,
        /// The broader tag negated explicitly.
        explicit_broader: TagId,
    },
    /// Negating two conflicting tags from an exclusive group in an `OR` clause (for example,
    /// `!fast | !slow`, or `!(fast & slow)`) matches every valid item, because no item can carry
    /// both.
    ExclusiveGroup {
        /// The exclusive group.
        group: &'static TagGroup,
        /// Two conflicting tags in `group`.
        conflicting: [TagId; 2],
    },
}

struct Parser<'a, 'b> {
    remaining: &'b [Token<'a>],
    vocabulary: &'b Vocabulary,
    end_offset: usize,
}

impl Parser<'_, '_> {
    fn parse_or(&mut self) -> Result<Expr, ExprParseError> {
        let first = self.parse_and()?;
        let mut operands = vec![first];
        while let Some((token, rest)) = self.remaining.split_first() {
            if matches!(token.kind, TokenKind::Or) {
                self.remaining = rest;
                operands.push(self.parse_and()?);
            } else {
                break;
            }
        }
        Ok(Expr::any(operands))
    }

    fn parse_and(&mut self) -> Result<Expr, ExprParseError> {
        let first = self.parse_not()?;
        let mut operands = vec![first];
        while let Some((token, rest)) = self.remaining.split_first() {
            if matches!(token.kind, TokenKind::And) {
                self.remaining = rest;
                operands.push(self.parse_not()?);
            } else {
                break;
            }
        }
        Ok(Expr::all(operands))
    }

    fn parse_not(&mut self) -> Result<Expr, ExprParseError> {
        let Some((token, rest)) = self.remaining.split_first() else {
            return Err(self.unexpected_end());
        };
        if matches!(token.kind, TokenKind::Not) {
            self.remaining = rest;
            return Ok(!self.parse_not()?);
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<Expr, ExprParseError> {
        let Some((token, rest)) = self.remaining.split_first() else {
            return Err(self.unexpected_end());
        };
        self.remaining = rest;
        match token.kind {
            TokenKind::OpenParen => {
                let inner = self.parse_or()?;
                match self.remaining.split_first() {
                    Some((close, after)) if matches!(close.kind, TokenKind::CloseParen) => {
                        self.remaining = after;
                        Ok(inner)
                    }
                    Some((other, _)) => Err(ExprParseError {
                        span: other.span.clone(),
                        kind: ExprParseErrorKind::UnexpectedToken {
                            token: other.text.to_owned(),
                        },
                    }),
                    None => Err(ExprParseError {
                        span: token.span.clone(),
                        kind: ExprParseErrorKind::UnclosedParen,
                    }),
                }
            }
            TokenKind::Ident(label) => match self.vocabulary.resolve(label) {
                Ok(id) => Ok(Expr::Tag(id)),
                Err(TagLookupError::Group(group)) => Err(ExprParseError {
                    span: token.span.clone(),
                    kind: ExprParseErrorKind::Group {
                        label: label.to_owned(),
                        group,
                    },
                }),
                Err(TagLookupError::Unknown) => Err(ExprParseError {
                    span: token.span.clone(),
                    kind: ExprParseErrorKind::UnknownTag {
                        label: label.to_owned(),
                    },
                }),
            },
            TokenKind::CloseParen | TokenKind::And | TokenKind::Or | TokenKind::Not => {
                Err(ExprParseError {
                    span: token.span.clone(),
                    kind: ExprParseErrorKind::UnexpectedToken {
                        token: token.text.to_owned(),
                    },
                })
            }
        }
    }

    const fn unexpected_end(&self) -> ExprParseError {
        ExprParseError {
            span: self.end_offset..self.end_offset,
            kind: ExprParseErrorKind::UnexpectedEnd,
        }
    }
}

#[derive(Debug, Clone)]
struct Token<'a> {
    kind: TokenKind<'a>,
    text: &'a str,
    span: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind<'a> {
    OpenParen,
    CloseParen,
    Not,
    And,
    Or,
    Ident(&'a str),
}

fn tokenize(input: &str) -> Vec<Token<'_>> {
    let mut chars = input.char_indices().peekable();
    let mut tokens = Vec::new();
    while let Some((start, character)) = chars.next() {
        if character.is_whitespace() {
            continue;
        }
        let token = match character {
            '(' => Token {
                kind: TokenKind::OpenParen,
                text: "(",
                span: start..start + 1,
            },
            ')' => Token {
                kind: TokenKind::CloseParen,
                text: ")",
                span: start..start + 1,
            },
            '!' => Token {
                kind: TokenKind::Not,
                text: "!",
                span: start..start + 1,
            },
            '&' => {
                let end = if chars.next_if(|&(_, next)| next == '&').is_some() {
                    start + 2
                } else {
                    start + 1
                };
                Token {
                    kind: TokenKind::And,
                    text: &input[start..end],
                    span: start..end,
                }
            }
            '|' => {
                let end = if chars.next_if(|&(_, next)| next == '|').is_some() {
                    start + 2
                } else {
                    start + 1
                };
                Token {
                    kind: TokenKind::Or,
                    text: &input[start..end],
                    span: start..end,
                }
            }
            _ => {
                let mut end = start + character.len_utf8();
                while let Some((next_index, next_character)) =
                    chars.next_if(|&(_, next)| is_ident_char(next))
                {
                    end = next_index + next_character.len_utf8();
                }
                let text = &input[start..end];
                let kind = if text.eq_ignore_ascii_case("not") {
                    TokenKind::Not
                } else if text.eq_ignore_ascii_case("and") {
                    TokenKind::And
                } else if text.eq_ignore_ascii_case("or") {
                    TokenKind::Or
                } else {
                    TokenKind::Ident(text)
                };
                Token {
                    kind,
                    text,
                    span: start..end,
                }
            }
        };
        tokens.push(token);
    }
    tokens
}

const fn is_ident_char(character: char) -> bool {
    !character.is_whitespace() && !matches!(character, '(' | ')' | '!' | '&' | '|')
}

/// Pushes `!` inward to individual tags (`!(a & b)` -> `!a | !b`) and flattens nested `And`/`Or`.
fn to_nnf(expression: &Expr, negated: bool) -> Expr {
    match expression {
        Expr::All => {
            if negated {
                Expr::None
            } else {
                Expr::All
            }
        }
        Expr::None => {
            if negated {
                Expr::All
            } else {
                Expr::None
            }
        }
        Expr::Tag(id) => {
            if negated {
                Expr::Not(Box::new(Expr::Tag(*id)))
            } else {
                Expr::Tag(*id)
            }
        }
        Expr::ExplicitTag(id) => {
            if negated {
                Expr::Not(Box::new(Expr::ExplicitTag(*id)))
            } else {
                Expr::ExplicitTag(*id)
            }
        }
        Expr::Not(inner) => to_nnf(inner, !negated),
        Expr::And(children) => {
            let mapped = children.iter().map(|child| to_nnf(child, negated));
            if negated {
                Expr::any(mapped)
            } else {
                Expr::all(mapped)
            }
        }
        Expr::Or(children) => {
            let mapped = children.iter().map(|child| to_nnf(child, negated));
            if negated {
                Expr::all(mapped)
            } else {
                Expr::any(mapped)
            }
        }
    }
}

fn lint_nnf(expression: &Expr, vocabulary: &Vocabulary, warnings: &mut Vec<ExprWarning>) {
    match expression {
        Expr::All | Expr::None | Expr::Tag(_) | Expr::ExplicitTag(_) | Expr::Not(_) => {}
        Expr::And(children) => {
            for child in children {
                lint_nnf(child, vocabulary, warnings);
            }
            lint_clause(ClauseMode::Conjunction, children, vocabulary, warnings);
        }
        Expr::Or(children) => {
            for child in children {
                lint_nnf(child, vocabulary, warnings);
            }
            lint_clause(ClauseMode::Disjunction, children, vocabulary, warnings);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClauseMode {
    Conjunction,
    Disjunction,
}

fn lint_clause(
    mode: ClauseMode,
    children: &[Expr],
    vocabulary: &Vocabulary,
    warnings: &mut Vec<ExprWarning>,
) {
    let mut clause_warnings = Vec::new();
    let absorbing = match mode {
        ClauseMode::Conjunction => Expr::None,
        ClauseMode::Disjunction => Expr::All,
    };
    if children.contains(&absorbing) {
        clause_warnings.push(ExprWarning::Contradiction(ContradictionKind::ConstantNone));
    }
    let literals: Vec<Literal> = match mode {
        ClauseMode::Conjunction => children.iter().filter_map(extract_literal).collect(),
        ClauseMode::Disjunction => children
            .iter()
            .filter_map(extract_literal)
            .map(Literal::negated)
            .collect(),
    };
    analyze_conjunction(&literals, vocabulary, &mut clause_warnings);
    match mode {
        ClauseMode::Conjunction => warnings.extend(clause_warnings),
        ClauseMode::Disjunction => {
            warnings.extend(clause_warnings.into_iter().map(|warning| match warning {
                ExprWarning::Contradiction(kind) => ExprWarning::Tautology(kind.into_tautology()),
                other => other,
            }));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Literal {
    tag: TagId,
    polarity: Polarity,
    scope: Scope,
}

impl Literal {
    const fn negated(self) -> Self {
        Self {
            polarity: match self.polarity {
                Polarity::Positive => Polarity::Negative,
                Polarity::Negative => Polarity::Positive,
            },
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Effective,
    Explicit,
}

fn extract_literal(expression: &Expr) -> Option<Literal> {
    match expression {
        Expr::Tag(tag) => Some(Literal {
            tag: *tag,
            polarity: Polarity::Positive,
            scope: Scope::Effective,
        }),
        Expr::ExplicitTag(tag) => Some(Literal {
            tag: *tag,
            polarity: Polarity::Positive,
            scope: Scope::Explicit,
        }),
        Expr::Not(inner) => match inner.as_ref() {
            Expr::Tag(tag) => Some(Literal {
                tag: *tag,
                polarity: Polarity::Negative,
                scope: Scope::Effective,
            }),
            Expr::ExplicitTag(tag) => Some(Literal {
                tag: *tag,
                polarity: Polarity::Negative,
                scope: Scope::Explicit,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn analyze_conjunction(
    literals: &[Literal],
    vocabulary: &Vocabulary,
    warnings: &mut Vec<ExprWarning>,
) {
    for &first in literals {
        if first.polarity != Polarity::Positive {
            continue;
        }
        for &second in literals {
            match (second.polarity, second.scope) {
                (Polarity::Negative, Scope::Effective)
                    if vocabulary.implies(first.tag, second.tag) =>
                {
                    warnings.push(ExprWarning::Contradiction(
                        ContradictionKind::ImpliesNegated {
                            positive: first.tag,
                            negated: second.tag,
                        },
                    ));
                }
                (Polarity::Negative, Scope::Explicit)
                    if first.scope == Scope::Explicit && first.tag == second.tag =>
                {
                    warnings.push(ExprWarning::Contradiction(
                        ContradictionKind::ImpliesNegated {
                            positive: first.tag,
                            negated: second.tag,
                        },
                    ));
                }
                (Polarity::Positive, Scope::Explicit)
                    if first.tag != second.tag && vocabulary.implies(first.tag, second.tag) =>
                {
                    warnings.push(ExprWarning::Contradiction(ContradictionKind::Antichain {
                        narrower: first.tag,
                        explicit_broader: second.tag,
                    }));
                }
                _ => {}
            }
        }
    }

    let mut positive_closure = FixedBitSet::with_capacity(vocabulary.tags().len());
    for &literal in literals {
        if literal.polarity == Polarity::Positive {
            positive_closure.union_with(vocabulary.closure(literal.tag));
        }
    }
    for group in vocabulary.groups() {
        if let Some(conflicting) = vocabulary.exclusive_conflict(&positive_closure, group) {
            warnings.push(ExprWarning::Contradiction(
                ContradictionKind::ExclusiveGroup { group, conflicting },
            ));
        }
    }

    collect_redundancies(literals, vocabulary, warnings);
}

/// Emits at most one [`ExprWarning::Redundant`] per literal position.
fn collect_redundancies(
    literals: &[Literal],
    vocabulary: &Vocabulary,
    warnings: &mut Vec<ExprWarning>,
) {
    for (position, &candidate) in literals.iter().enumerate() {
        if literals[..position].contains(&candidate) {
            warnings.push(ExprWarning::Redundant {
                redundant: candidate.tag,
                covered_by: candidate.tag,
            });
            continue;
        }
        let subsuming = literals.iter().find_map(|&other| {
            if candidate.polarity != other.polarity || candidate == other {
                return None;
            }
            let covered = match candidate.polarity {
                Polarity::Positive => positive_implies(vocabulary, other, candidate),
                Polarity::Negative => positive_implies(vocabulary, candidate, other),
            };
            covered.then_some(other.tag)
        });
        if let Some(covered_by) = subsuming {
            warnings.push(ExprWarning::Redundant {
                redundant: candidate.tag,
                covered_by,
            });
        }
    }
}

fn positive_implies(vocabulary: &Vocabulary, premise: Literal, conclusion: Literal) -> bool {
    conclusion.scope == Scope::Effective && vocabulary.implies(premise.tag, conclusion.tag)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::tag::{Cardinality, Tag};

    const AREA: TagGroup = TagGroup {
        name: "Area",
        cardinality: Cardinality::AtLeastOne,
    };
    const OS: TagGroup = TagGroup {
        name: "Operating system",
        cardinality: Cardinality::AtMostOne,
    };
    const SPEED: TagGroup = TagGroup {
        name: "Speed",
        cardinality: Cardinality::ExactlyOne,
    };
    const UNIX: Tag = Tag::new(&OS, "unix", "Unix.");
    const LINUX: Tag = Tag::new(&OS, "linux", "Linux.").implies(&[UNIX]);
    const WINDOWS: Tag = Tag::new(&OS, "windows", "Windows.");
    const KERNEL: Tag = Tag::new(&AREA, "kernel", "Kernel.");
    const EBPF: Tag = Tag::new(&AREA, "ebpf", "eBPF.")
        .implies(&[LINUX, KERNEL])
        .aliases(&["bpf"]);
    const USERSPACE: Tag = Tag::new(&AREA, "userspace", "Userspace.");
    const FAST: Tag = Tag::new(&SPEED, "fast", "Fast.");
    const SLOW: Tag = Tag::new(&SPEED, "slow", "Slow.");

    fn vocabulary() -> Vocabulary {
        Vocabulary::new([EBPF, USERSPACE, WINDOWS, FAST, SLOW]).unwrap()
    }

    #[rstest]
    #[case::tag_and_not("(linux | windows) & !slow", &[EBPF, FAST], true)]
    #[case::excluded_by_not("(linux | windows) & !slow", &[EBPF, SLOW], false)]
    #[case::keyword_operators_case_insensitive("bpf AND NOT slow", &[EBPF, FAST], true)]
    #[case::double_operators_and_precedence("windows || unix && fast", &[EBPF, FAST], true)]
    #[case::and_binds_tighter_than_or("windows | unix & slow", &[EBPF, FAST], false)]
    #[case::double_negation("!!bpf & not !fast", &[EBPF, FAST], true)]
    fn parsed_expressions_evaluate_precedence_and_aliases(
        #[case] query: &str,
        #[case] item_tags: &[Tag],
        #[case] expected: bool,
    ) {
        let vocab = vocabulary();
        let set = TagSet::new(&vocab, item_tags.iter().copied()).unwrap();
        let parsed = Expr::parse(&vocab, query).unwrap();
        assert_eq!(parsed.matches(&set), expected);
    }

    #[test]
    fn empty_and_constant_combinators_flatten_identities() {
        let vocab = vocabulary();
        let [unix, ebpf] = ["unix", "ebpf"].map(|label| vocab.id(label).unwrap());

        assert_eq!(Expr::all([]), Expr::All);
        assert_eq!(Expr::any([]), Expr::None);
        assert_eq!(
            (Expr::Tag(unix) | Expr::None) & (Expr::Tag(ebpf) & Expr::All),
            Expr::And(vec![Expr::Tag(unix), Expr::Tag(ebpf)])
        );
    }

    #[test]
    fn explicit_tag_and_negated_constants_evaluate_against_tag_set() {
        let vocab = vocabulary();
        let unix = vocab.id("unix").unwrap();
        let set = TagSet::new(&vocab, [EBPF, FAST]).unwrap();
        let strict_descendant = Expr::Tag(unix) & !Expr::ExplicitTag(unix);

        assert!(strict_descendant.matches(&set));
        assert!((!Expr::None).matches(&set));
        assert!(!(!Expr::All).matches(&set));
    }

    #[rstest]
    #[case::empty("   ", 0..3, "expression is empty")]
    #[case::trailing_and("linux &", 7..7, "unexpected end of expression")]
    #[case::dangling_not("!", 1..1, "unexpected end of expression")]
    #[case::open_paren_at_end("(", 1..1, "unexpected end of expression")]
    #[case::unclosed_paren("(linux & fast", 0..1, "unclosed `(`")]
    #[case::empty_parens("()", 1..2, "unexpected token `)`")]
    #[case::missing_operator_inside_parens("(linux fast)", 7..11, "unexpected token `fast`")]
    #[case::trailing_token("linux fast", 6..10, "unexpected token `fast`")]
    #[case::trailing_bang("linux !", 6..7, "unexpected token `!`")]
    #[case::leading_and("& linux", 0..1, "unexpected token `&`")]
    #[case::triple_amp("linux &&& fast", 8..9, "unexpected token `&`")]
    #[case::double_and_error("linux && &&", 9..11, "unexpected token `&&`")]
    #[case::double_or_error("linux || ||", 9..11, "unexpected token `||`")]
    #[case::single_or_error("linux | |", 8..9, "unexpected token `|`")]
    #[case::group_label("speed & linux", 0..5, "`speed` names group `Speed`, not a tag")]
    #[case::unknown_tag("linxu", 0..5, "unknown tag `linxu`")]
    #[case::single_char_unknown("  x", 2..3, "unknown tag `x`")]
    #[case::multibyte_utf8_unknown("  éß", 2..6, "unknown tag `éß`")]
    fn parse_reports_exact_spans_and_reasons(
        #[case] input: &str,
        #[case] expected_span: Range<usize>,
        #[case] expected_message: &str,
    ) {
        let vocab = vocabulary();
        let error = Expr::parse(&vocab, input).unwrap_err();
        assert_eq!(error.span, expected_span);
        assert_eq!(error.to_string(), expected_message);
    }

    #[rstest]
    #[case::positive_and_negated_unrelated("linux & !slow")]
    #[case::broader_and_negated_narrower("unix & !linux")]
    #[case::disjoint_optional_group("linux | windows")]
    #[case::mixed_boolean_tree("(linux | windows) & !slow")]
    fn lint_accepts_satisfiable_irredundant_queries(#[case] query: &str) {
        let vocab = vocabulary();
        let warnings = Expr::parse(&vocab, query).unwrap().lint(&vocab);
        assert_eq!(warnings, []);
    }

    #[test]
    fn lint_detects_conjunction_contradictions_with_exact_kinds() {
        let vocab = vocabulary();
        let [unix, linux, windows, ebpf, fast, slow] =
            ["unix", "linux", "windows", "ebpf", "fast", "slow"]
                .map(|label| vocab.id(label).unwrap());

        assert_eq!(
            Expr::parse(&vocab, "ebpf & !unix").unwrap().lint(&vocab),
            [ExprWarning::Contradiction(
                ContradictionKind::ImpliesNegated {
                    positive: ebpf,
                    negated: unix,
                }
            )]
        );
        assert_eq!(
            Expr::parse(&vocab, "!!linux & !linux")
                .unwrap()
                .lint(&vocab),
            [ExprWarning::Contradiction(
                ContradictionKind::ImpliesNegated {
                    positive: linux,
                    negated: linux,
                }
            )]
        );
        assert_eq!(
            Expr::parse(&vocab, "fast & slow").unwrap().lint(&vocab),
            [ExprWarning::Contradiction(
                ContradictionKind::ExclusiveGroup {
                    group: &SPEED,
                    conflicting: [fast, slow],
                }
            )]
        );
        assert_eq!(
            Expr::parse(&vocab, "ebpf & windows").unwrap().lint(&vocab),
            [ExprWarning::Contradiction(
                ContradictionKind::ExclusiveGroup {
                    group: &OS,
                    conflicting: [windows, linux],
                }
            )]
        );
    }

    #[test]
    fn lint_detects_disjunction_tautologies_and_de_morgan_duals() {
        let vocab = vocabulary();
        let [unix, linux, ebpf, fast, slow] =
            ["unix", "linux", "ebpf", "fast", "slow"].map(|label| vocab.id(label).unwrap());

        assert_eq!(
            Expr::parse(&vocab, "!(unix | !ebpf)").unwrap().lint(&vocab),
            [ExprWarning::Contradiction(
                ContradictionKind::ImpliesNegated {
                    positive: ebpf,
                    negated: unix,
                }
            )]
        );
        assert_eq!(
            Expr::parse(&vocab, "unix | !ebpf").unwrap().lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ImpliesNegated {
                positive: unix,
                negated: ebpf,
            })]
        );
        assert_eq!(
            Expr::parse(&vocab, "linux | !linux").unwrap().lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ImpliesNegated {
                positive: linux,
                negated: linux,
            })]
        );
        assert_eq!(
            Expr::parse(&vocab, "!(fast & slow)").unwrap().lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ExclusiveGroup {
                group: &SPEED,
                conflicting: [fast, slow],
            })]
        );
    }

    #[test]
    fn lint_flags_root_constant_all_and_none() {
        let vocab = vocabulary();

        assert_eq!(
            Expr::All.lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ConstantAll)]
        );
        assert_eq!(
            Expr::None.lint(&vocab),
            [ExprWarning::Contradiction(ContradictionKind::ConstantNone)]
        );
        assert_eq!(
            (Expr::All & Expr::None).lint(&vocab),
            [ExprWarning::Contradiction(ContradictionKind::ConstantNone)]
        );
        assert_eq!(
            (Expr::None | Expr::All).lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ConstantAll)]
        );
    }

    #[test]
    fn lint_accepts_satisfiable_explicit_tag_combinations() {
        let vocab = vocabulary();
        let [unix, linux, ebpf, userspace] =
            ["unix", "linux", "ebpf", "userspace"].map(|label| vocab.id(label).unwrap());

        assert_eq!(
            (Expr::Tag(unix) & !Expr::ExplicitTag(unix)).lint(&vocab),
            []
        );
        assert_eq!(
            (Expr::Tag(userspace) & Expr::ExplicitTag(unix)).lint(&vocab),
            []
        );
        assert_eq!(
            (Expr::ExplicitTag(unix) | Expr::ExplicitTag(linux)).lint(&vocab),
            []
        );
        assert_eq!(
            (Expr::ExplicitTag(unix) | !Expr::ExplicitTag(ebpf)).lint(&vocab),
            []
        );
    }

    #[test]
    fn lint_handles_antichain_and_constant_branches() {
        let vocab = vocabulary();
        let [unix, linux, ebpf] = ["unix", "linux", "ebpf"].map(|label| vocab.id(label).unwrap());

        assert_eq!(
            (Expr::ExplicitTag(unix) | !Expr::Tag(unix)).lint(&vocab),
            []
        );
        assert_eq!(
            (Expr::Tag(ebpf) & Expr::ExplicitTag(unix)).lint(&vocab),
            [ExprWarning::Contradiction(ContradictionKind::Antichain {
                narrower: ebpf,
                explicit_broader: unix,
            })]
        );
        assert_eq!(
            (!Expr::Tag(ebpf) | !Expr::ExplicitTag(unix)).lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::Antichain {
                narrower: ebpf,
                explicit_broader: unix,
            })]
        );
        assert_eq!(
            Expr::And(vec![Expr::Tag(linux), Expr::None]).lint(&vocab),
            [ExprWarning::Contradiction(ContradictionKind::ConstantNone)]
        );
    }

    #[test]
    fn lint_handles_explicit_negation_tautology_and_constant_all() {
        let vocab = vocabulary();
        let [unix, ebpf] = ["unix", "ebpf"].map(|label| vocab.id(label).unwrap());

        assert_eq!(
            (Expr::Tag(unix) | !Expr::ExplicitTag(ebpf)).lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ImpliesNegated {
                positive: unix,
                negated: ebpf,
            })]
        );
        assert_eq!(
            (Expr::ExplicitTag(unix) & !Expr::ExplicitTag(unix)).lint(&vocab),
            [ExprWarning::Contradiction(
                ContradictionKind::ImpliesNegated {
                    positive: unix,
                    negated: unix,
                }
            )]
        );
        assert_eq!(
            (Expr::ExplicitTag(unix) | !Expr::ExplicitTag(unix)).lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ImpliesNegated {
                positive: unix,
                negated: unix,
            })]
        );
        assert_eq!(
            Expr::Or(vec![Expr::Tag(unix), Expr::All]).lint(&vocab),
            [ExprWarning::Tautology(TautologyKind::ConstantAll)]
        );
    }

    #[rstest]
    #[case::and_ancestor_subsumed("unix & linux & ebpf", &[("unix", "linux"), ("linux", "ebpf")])]
    #[case::and_negated_child_subsumed("!unix & !linux", &[("linux", "unix")])]
    #[case::or_child_subsumed("unix | linux", &[("linux", "unix")])]
    #[case::or_negated_ancestor_subsumed("!unix | !linux", &[("unix", "linux")])]
    #[case::triple_duplicate_emits_once_per_extra("linux & linux & linux", &[("linux", "linux"), ("linux", "linux")])]
    fn lint_emits_at_most_one_redundancy_per_operand(
        #[case] query: &str,
        #[case] expected: &[(&str, &str)],
    ) {
        let vocab = vocabulary();
        let warnings = Expr::parse(&vocab, query).unwrap().lint(&vocab);
        let pairs: Vec<(&str, &str)> = warnings
            .into_iter()
            .map(|warning| match warning {
                ExprWarning::Redundant {
                    redundant,
                    covered_by,
                } => (vocab.tag(redundant).label, vocab.tag(covered_by).label),
                other => panic!("expected Redundant, got {other:?}"),
            })
            .collect();
        assert_eq!(pairs, expected);
    }
}
