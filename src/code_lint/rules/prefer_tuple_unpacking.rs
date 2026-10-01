//! Flags tuple elements read by literal position instead of being unpacked once (`prefer-tuple-unpacking`).

use crate::code_lint::ast::{self, AstNode, ParsedFile, ScopePositionalReads};
use crate::code_lint::rule::CodeDetector;
use crate::core::{Config, Detector, LanguageDefaults};
use crate::diagnostic::{Diagnostic, RuleName, ViolationTemplate, violation_template};
use crate::rule_documentation::{ConfigShape, Reference, RuleDoc};
use crate::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
use ast_grep_language::SupportLang;
use std::collections::BTreeSet;
use std::path::Path;

/// Default minimum number of distinct positions read from one receiver (`min`, `2`).
const DEFAULT_MIN_POSITIONS: LanguageDefaults<usize> = LanguageDefaults::new(2, &[]);

/// Default maximum number of `_` placeholders the unpacking may need (`max`, `2`); sparser reads
/// call for a named record rather than unpacking.
const DEFAULT_MAX_PLACEHOLDERS: LanguageDefaults<usize> = LanguageDefaults::new(2, &[]);

const TEMPLATE: ViolationTemplate = violation_template! {
    summary: {
        base: "`{receiver}` is read by index at positions {positions}.",
        Python => "`{receiver}` is indexed with literal positions {positions}.",
        Rust => "Tuple fields {positions} of `{receiver}` are read by index.",
    },
    rationale: "Positional indices hide what each element means, and every index site misreads or breaks when the tuple layout changes.",
    suggestion: {
        base: "Unpack the value once into named variables.",
        Python => "Unpack once into named variables (`x, y = point`, or `x, y, *_ = point` when the sequence can be longer); return a `NamedTuple` or dataclass when the tuple crosses a function boundary.",
        Rust => "Destructure once into named bindings (`let (start, end) = span;`, `let Point(x, y) = point;` for tuple structs, or `let (start, end) = &span;` when fields are not `Copy`); use a struct with named fields when the tuple crosses a function boundary.",
    },
};

/// Rule that flags a receiver read at several literal positions in one scope.
pub struct PreferTupleUnpacking;

impl PreferTupleUnpacking {
    /// The rule's declared facets (ADR 007).
    pub(crate) const CLASSIFICATION: Classification = Classification {
        topics: &[Topic::POSITIONAL_INDEXING],
        precision: Precision::Heuristic,
        consensus: Consensus::Opinionated,
        impacted_quality: ImpactedQuality::Maintainability,
    };

    /// The rule's user-facing doc.
    pub(crate) const DOC: RuleDoc = RuleDoc {
        summary: "Flags a value read at several literal positions instead of being unpacked once.",
        what_it_does: "Groups positional reads by value within one function and flags \
                       the value when at least `min` distinct positions are read (2 by \
                       default) and unpacking them would need at most `max` `_` \
                       placeholders (2 by default; `row[0], row[7]` is left alone). In \
                       Python, a read is an index by a decimal integer literal, negative \
                       allowed (`point[0]`, `xs[-1]`), on a name, attribute or index \
                       chain without calls (`self.pair[1]`, `rows[i][0]`); the type is \
                       not known, so lists and dicts with integer keys count too. \
                       Module-level code counts as one scope, comprehensions belong to \
                       their function, and lambdas and class bodies are ignored. A value \
                       is not flagged in a scope where it is also written through an \
                       index, deleted from, sliced, indexed by a variable or a \
                       non-decimal literal, iterated, passed to `len`, `enumerate`, \
                       `zip`, `reversed` or `sorted`, or mutated by a method such as \
                       `append` or `update`. In Rust, a read is a tuple field access \
                       (`span.0`, `self.1`, `cmd.span.0`) inside a function, closures \
                       included; a value whose field is assigned or mutably borrowed is \
                       not flagged, and macro arguments (`assert_eq!(t.0, t.1)`) are not \
                       inspected.",
        why_is_this_bad: "An index says where an element sits, not what it means: \
                          `point[0]` and `span.1` force the reader to remember the \
                          layout, and every index site silently reads the wrong element \
                          when the layout changes.\n\n\
                          Unpack once into named variables: `x, y = point` (or \
                          `first, *_, last = xs`) in Python, `let (start, end) = span;` \
                          in Rust. When the tuple crosses a function boundary, return a \
                          `NamedTuple`, a dataclass or a struct with named fields \
                          instead.",
        configuration: &[ConfigShape::Threshold],
        references: &[
            Reference {
                title: "PEP 3132: Extended Iterable Unpacking",
                url: "https://peps.python.org/pep-3132/",
            },
            Reference {
                title: "The Rust Programming Language: The Tuple Type",
                url: "https://doc.rust-lang.org/book/ch03-02-data-types.html#the-tuple-type",
            },
        ],
    };
}

impl Detector for PreferTupleUnpacking {
    fn name(&self) -> RuleName {
        RuleName("prefer-tuple-unpacking")
    }

    fn supported_languages(&self) -> &'static [SupportLang] {
        &[SupportLang::Python, SupportLang::Rust]
    }

    fn violation_template(&self) -> &'static ViolationTemplate {
        &TEMPLATE
    }
}

/// Positions read from one receiver in one scope, anchored on its first read.
struct ReceiverReads<'a> {
    receiver: String,
    first_read: AstNode<'a>,
    positions: BTreeSet<i64>,
}

/// Groups a scope's reads by receiver in first-seen order, skipping the scope's exempt receivers.
fn group_reads_by_receiver(scope: ScopePositionalReads<'_>) -> Vec<ReceiverReads<'_>> {
    let mut groups: Vec<ReceiverReads<'_>> = Vec::new();
    for read in scope.reads {
        if scope.exempt_receivers.contains(&read.receiver) {
            continue;
        }
        match groups
            .iter_mut()
            .find(|group| group.receiver == read.receiver)
        {
            Some(group) => {
                group.positions.insert(read.position);
            }
            None => groups.push(ReceiverReads {
                receiver: read.receiver,
                first_read: read.node,
                positions: BTreeSet::from([read.position]),
            }),
        }
    }
    groups
}

/// Number of `_` placeholders an unpacking pattern reading `positions` needs, with a free `*_`
/// between the leading (non-negative) and trailing (negative) positions.
fn placeholder_count(positions: &BTreeSet<i64>) -> usize {
    let leading = positions.range(0..);
    let trailing = positions.range(..0);
    let leading_gaps = leading
        .clone()
        .next_back()
        .map_or(0, |&last| last.unsigned_abs() + 1 - leading.count() as u64);
    let trailing_gaps = trailing
        .clone()
        .next()
        .map_or(0, |&first| first.unsigned_abs() - trailing.count() as u64);
    usize::try_from(leading_gaps + trailing_gaps).unwrap_or(usize::MAX)
}

impl CodeDetector for PreferTupleUnpacking {
    fn check_file(&self, path: &Path, file: &ParsedFile, config: &Config) -> Vec<Diagnostic> {
        let lang = file.lang();
        let min_positions = self.effective_min_threshold(lang, config, &DEFAULT_MIN_POSITIONS);
        let max_placeholders =
            self.effective_max_threshold(lang, config, &DEFAULT_MAX_PLACEHOLDERS);

        ast::collect_positional_reads(file)
            .into_iter()
            .flat_map(group_reads_by_receiver)
            .filter(|group| {
                group.positions.len() >= min_positions
                    && placeholder_count(&group.positions) <= max_placeholders
            })
            .map(|group| {
                let positions = group
                    .positions
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                self.diagnostic_at_node(
                    path,
                    &group.first_read,
                    &[("receiver", &group.receiver), ("positions", &positions)],
                )
            })
            .collect()
    }
}

#[cfg(test)]
crate::test_utils::rule_test!(
    PreferTupleUnpacking,
    {
        Python => {
            pass: [
                canonical_unpacking => r#"
                    def plot_point(point):
                        x, y = point
                        plot(x, y)
                "#,
                single_position_read => r#"
                    def first(xs):
                        return xs[0]
                "#,
                same_position_twice => r#"
                    def double_first(xs):
                        return xs[0] + xs[0]
                "#,
                call_receiver => r#"
                    def total():
                        return get_pair()[0] + get_pair()[1]
                "#,
                three_placeholders_over_limit => r#"
                    def ends(row):
                        return row[0], row[4]
                "#,
                tail_placeholders_over_limit => r#"
                    def ends(row):
                        return row[0], row[-4]
                "#,
                subscript_write_exempts_receiver => r#"
                    def shift(p):
                        p[0] = p[1] + p[2]
                "#,
                augmented_write_exempts_receiver => r#"
                    def accumulate(p):
                        p[0] += p[1] + p[2]
                "#,
                delete_exempts_receiver => r#"
                    def drop_head(p):
                        keep = p[1], p[2]
                        del p[0]
                "#,
                tuple_target_exempts_receiver => r#"
                    def reset(p, q):
                        p[0], q = p[1], p[2]
                "#,
                for_target_exempts_receiver => r#"
                    def fill(p, xs):
                        for p[0] in xs:
                            use(p[1], p[2])
                "#,
                slice_exempts_receiver => r#"
                    def split(p):
                        return p[0], p[1], p[2:]
                "#,
                variable_index_exempts_receiver => r#"
                    def pick(p, i):
                        return p[0], p[1], p[i]
                "#,
                tuple_key_exempts_receiver => r#"
                    def corner(grid):
                        return grid[0], grid[1], grid[1, 2]
                "#,
                non_decimal_index_exempts_receiver => r#"
                    def flags(p):
                        return p[0], p[1], p[0x1]
                "#,
                iterated_exempts_receiver => r#"
                    def main(argv):
                        src, dst = argv[1], argv[2]
                        for arg in argv:
                            check(arg)
                "#,
                enumerate_exempts_receiver => r#"
                    def bounds(xs):
                        for i, x in enumerate(xs):
                            check(i, x)
                        return xs[0], xs[-1]
                "#,
                len_exempts_receiver => r#"
                    def describe(row):
                        return row[0], row[1], len(row)
                "#,
                mutating_method_exempts_receiver => r#"
                    def push(stack, x):
                        stack.append(x)
                        return stack[0], stack[1]
                "#,
                default_arguments_in_enclosing_scope => r#"
                    for limit in LIMITS:
                        check(limit)

                    def clamp(v, lo=LIMITS[0], hi=LIMITS[1]):
                        return v
                "#,
                reads_split_across_functions => r#"
                    def first(p):
                        return p[0]

                    def second(p):
                        return p[1]
                "#,
                lambda_body_skipped => r#"
                    def by_second(pairs):
                        return sorted(pairs, key=lambda p: (p[1], p[0]))
                "#,
                class_body_ignored => r#"
                    class Origin:
                        X = POINT[0]
                        Y = POINT[1]
                "#,
            ],
            fail: [
                two_positions_in_function => r#"
                    def plot_point(point):
                        plot(point[0], point[1])
                "# => "point[0]",
                head_and_tail_positions => r#"
                    def bounds(xs):
                        first, last = xs[0], xs[-1]
                "# => "xs[0]",
                attribute_receiver => r#"
                    def area(self):
                        return self.pair[0] * self.pair[1]
                "# => "self.pair[0]",
                subscript_receiver => r#"
                    def cell(rows, i):
                        return rows[i][0], rows[i][1]
                "# => "rows[i][0]",
                two_placeholders_at_limit => r#"
                    def ends(row):
                        return row[0], row[3]
                "# => "row[0]",
                tail_placeholders_at_limit => r#"
                    def ends(row):
                        return row[0], row[-3]
                "# => "row[0]",
                index_inside_write_target_is_read => r#"
                    def move(a, p):
                        a[p[0]] = p[1]
                "# => "p[0]",
                comprehension_groups_with_function => r#"
                    def sums(pairs):
                        return [p[0] + p[1] for p in pairs]
                "# => "p[0]",
                nested_function_is_own_scope => r#"
                    def outer(p):
                        head = p[0]

                        def inner():
                            return p[1], p[2]
                "# => "p[1]",
            ],
        },
        Rust => {
            pass: [
                canonical_destructuring => r#"
                    fn width(span: (u32, u32)) -> u32 {
                        let (start, end) = span;
                        end - start
                    }
                "#,
                newtype_single_field => r#"
                    fn raw(id: UserId) -> u64 {
                        id.0
                    }
                "#,
                call_receiver => r#"
                    fn total() -> u32 {
                        pair().0 + pair().1
                    }
                "#,
                field_write_exempts_receiver => r#"
                    fn accumulate(mut t: (u32, u32, u32)) -> (u32, u32, u32) {
                        t.0 += t.1 + t.2;
                        t
                    }
                "#,
                mutable_borrow_exempts_receiver => r#"
                    fn bump(t: &mut (u32, u32, u32)) -> u32 {
                        let first = &mut t.0;
                        *first += 1;
                        t.1 + t.2
                    }
                "#,
                swap_assignment_exempts_receiver => r#"
                    fn swap(mut t: (u32, u32)) -> (u32, u32) {
                        (t.0, t.1) = (t.1, t.0);
                        t
                    }
                "#,
                items_outside_functions_ignored => r#"
                    const SUM: u32 = PAIR.0 + PAIR.1;
                "#,
                known_gap_macro_arguments_not_inspected => r#"
                    fn check(t: (u32, u32)) {
                        assert_eq!(t.0, t.1);
                    }
                "#,
                reads_split_across_functions => r#"
                    fn first(t: (u32, u32)) -> u32 {
                        t.0
                    }

                    fn second(t: (u32, u32)) -> u32 {
                        t.1
                    }
                "#,
                three_placeholders_over_limit => r#"
                    fn ends(t: Row) -> (u32, u32) {
                        (t.0, t.4)
                    }
                "#,
            ],
            fail: [
                two_fields_in_function => r#"
                    fn width(span: (u32, u32)) -> u32 {
                        span.1 - span.0
                    }
                "# => "span.1",
                self_tuple_struct_fields => r#"
                    impl Range {
                        fn len(&self) -> u32 {
                            self.1 - self.0
                        }
                    }
                "# => "self.1",
                field_chain_receiver => r#"
                    fn span_of(cmd: &Command) -> SourceSpan {
                        SourceSpan::new(cmd.span.0, cmd.span.1)
                    }
                "# => "cmd.span.0",
                closure_groups_with_function => r#"
                    fn split(t: (u32, u32)) {
                        let a = t.0;
                        let b = move || t.1;
                    }
                "# => "t.0",
                nested_function_is_own_scope => r#"
                    fn outer(t: (u32, u32, u32)) {
                        let head = t.0;
                        fn inner(t: (u32, u32, u32)) -> u32 {
                            t.1 + t.2
                        }
                    }
                "# => "t.1",
            ],
        },
    }
);
