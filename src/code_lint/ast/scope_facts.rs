//! Per-file declaration-scope facts extracted from a [`ParsedFile`].
//!
//! Collects each scope's items, raw reference relations, `callee-before-caller` ordering edges,
//! public-root ownership, recursion groups, topological levels, and ordering findings for a single
//! parsed file. Multi-file project resolution lives in `crate::code_lint::graph`.

use super::call_clusters::{
    CallClusterRule, DeclarationScope, DeclarationScopeKind, OrderingEdge, OrderingReason,
    PlacementAnchor, RelationFact, ScopeGraph, ScopeItem, ScopeItemKind, Visibility,
    collect_declaration_scopes, scope_findings,
};
use super::tree::ParsedFile;

/// Everything the analyzer knows about one scope (`Module`, `Class`, or `Impl`) in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ScopeFacts {
    /// Which syntactic container this scope covers.
    pub(in crate::code_lint) scope: DeclarationScopeKind,
    /// Declarations in source order. Every index in [`ScopeFacts`] (`roots`, `source`, `target`,
    /// `higher`, `lower`) indexes into this slice.
    pub(in crate::code_lint) items: Vec<ItemFacts>,
    /// Direct intra-scope references, labeled by where the reference occurs.
    pub(in crate::code_lint) relations: Vec<RelationFact>,
    /// Downward `callee-before-caller` ordering edges (`higher -> lower`), labeled by why `higher`
    /// sits above `lower`.
    pub(in crate::code_lint) ordering: Vec<OrderingEdge>,
}

/// One declaration in [`ScopeFacts::items`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct ItemFacts {
    /// Identifier name of the declaration.
    pub(in crate::code_lint) name: String,
    /// Callable (`fn` / `def`) or type (`struct`, `enum`, `union`, `trait`, `class`).
    pub(in crate::code_lint) kind: ScopeItemKind,
    /// Public or private visibility tier.
    pub(in crate::code_lint) visibility: Visibility,
    /// True if this callable is a lifecycle constructor (`__init__`, `pub fn new`, etc.).
    pub(in crate::code_lint) is_constructor: bool,
    /// 1-based line of the declaration's name identifier.
    pub(in crate::code_lint) line: usize,
    /// 1-based last line of the declaration, including its attached `impl` blocks or grouped
    /// overloads and accessors.
    pub(in crate::code_lint) end_line: usize,
    /// 1-based column of the declaration's name identifier.
    pub(in crate::code_lint) column: usize,
    /// Compact one-line signature header (parameters + return type for callables; public fields or
    /// variants for types), when available.
    pub(in crate::code_lint) signature: Option<String>,
    /// Sorted indices in [`ScopeFacts::items`] of the public entrypoint units that reach this item
    /// through private references (`Roots(f)` with constructors collapsed to the scope's first
    /// constructor).
    pub(in crate::code_lint) roots: Vec<usize>,
    /// Strongly connected component id over [`ScopeFacts::ordering`]. Mutual recursion shares an id.
    pub(in crate::code_lint) recursion_group: usize,
    /// Topological level in [`ScopeFacts::ordering`]: `0` for leaves, `1 + max(callee.level)` above
    /// them, with a recursion group sharing one level.
    pub(in crate::code_lint) level: usize,
    /// Call-cluster ordering findings reported on this declaration, in rule priority order.
    pub(in crate::code_lint) findings: Vec<FindingFact>,
}

/// A call-cluster lint finding attached to an [`ItemFacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint) struct FindingFact {
    /// Call-cluster placement rule violated by this item.
    pub(in crate::code_lint) rule: CallClusterRule,
    /// Name of the owning or calling declaration the finding relates to.
    pub(in crate::code_lint) related: String,
    /// Why `related` sits above this item (`callee-before-caller` only).
    pub(in crate::code_lint) reason: Option<OrderingReason>,
    /// Sibling declaration next to which this item belongs.
    pub(in crate::code_lint) anchor: PlacementAnchor,
}

/// Collects [`ScopeFacts`] for every declaration scope in `file`, in source order.
#[must_use]
pub(in crate::code_lint) fn collect_scope_facts(file: &ParsedFile) -> Vec<ScopeFacts> {
    collect_declaration_scopes(file)
        .iter()
        .map(scope_facts)
        .collect()
}

fn scope_facts(scope: &DeclarationScope<'_>) -> ScopeFacts {
    let items = &scope.items;
    let graph = ScopeGraph::new(items);
    let mut item_facts: Vec<ItemFacts> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let coordinate = item.name_node.start_coordinate();
            ItemFacts {
                name: item.name.clone(),
                kind: item.kind,
                visibility: item.visibility,
                is_constructor: item.is_constructor,
                line: coordinate.line,
                end_line: item.end_line,
                column: coordinate.column,
                signature: item.signature.clone(),
                roots: graph.unit_owners[idx].clone(),
                recursion_group: graph.recursion_group[idx],
                level: graph.level[idx],
                findings: Vec::new(),
            }
        })
        .collect();
    for finding in scope_findings(scope, &graph) {
        item_facts[finding.item].findings.push(FindingFact {
            rule: finding.rule,
            related: items[finding.related].name.clone(),
            reason: finding.reason,
            anchor: finding.anchor,
        });
    }
    ScopeFacts {
        scope: scope.kind.clone(),
        items: item_facts,
        relations: raw_relations(items),
        ordering: graph.edges,
    }
}

/// Direct intra-scope references (`source != target`), sorted by `(source, target, kind)`.
fn raw_relations(items: &[ScopeItem<'_>]) -> Vec<RelationFact> {
    let mut relations: Vec<RelationFact> = items
        .iter()
        .enumerate()
        .flat_map(|(source, item)| {
            item.references
                .iter()
                .filter(move |&&(target, _)| target != source && target < items.len())
                .map(move |&(target, kind)| RelationFact {
                    source,
                    target,
                    kind,
                })
        })
        .collect();
    relations.sort();
    relations.dedup();
    relations
}

#[cfg(test)]
mod tests {
    use super::super::call_clusters::RelationKind;
    use super::*;
    use crate::diagnostic::Language;

    const SANDWICH: &str = indoc::indoc! {r"
        pub fn parse_config(raw: &str) -> usize {
            measure(raw)
        }

        fn measure(raw: &str) -> usize {
            let frame = Frame { len: raw.len() };
            frame_len(&frame)
        }

        fn frame_len(frame: &Frame) -> usize {
            frame.len
        }

        struct Frame {
            len: usize,
        }
    "};

    const MUTUAL_RECURSION: &str = indoc::indoc! {r"
        pub fn evaluate(n: usize) -> bool {
            is_even(n)
        }

        fn is_even(n: usize) -> bool {
            if n == 0 { true } else { is_odd(n - 1) }
        }

        fn is_odd(n: usize) -> bool {
            if n == 0 { false } else { is_even(n - 1) }
        }
    "};

    fn only_scope(source: &str) -> ScopeFacts {
        let mut scopes = collect_scope_facts(&ParsedFile::rust(source));
        assert_eq!(scopes.len(), 1, "expected exactly one scope");
        scopes.remove(0)
    }

    #[test]
    fn module_scope_items_roots_and_levels() {
        let facts = only_scope(SANDWICH);
        let summary: Vec<_> = facts
            .items
            .iter()
            .map(|item| {
                (
                    item.name.as_str(),
                    item.kind,
                    item.visibility,
                    item.roots.as_slice(),
                    item.level,
                )
            })
            .collect();

        assert_eq!(facts.scope, DeclarationScopeKind::Module);
        assert_eq!(
            summary,
            [
                (
                    "parse_config",
                    ScopeItemKind::Callable,
                    Visibility::Public,
                    &[0][..],
                    3
                ),
                (
                    "measure",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    &[0][..],
                    2
                ),
                (
                    "frame_len",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    &[0][..],
                    0
                ),
                (
                    "Frame",
                    ScopeItemKind::Type,
                    Visibility::Private,
                    &[0][..],
                    1
                ),
            ]
        );
    }

    #[test]
    fn items_carry_their_name_location() {
        let facts = only_scope(SANDWICH);

        assert_eq!(facts.items[2].line, 10);
        assert_eq!(facts.items[2].column, 4);
    }

    #[test]
    fn module_scope_relations_and_ordering() {
        let facts = only_scope(SANDWICH);

        assert_eq!(
            facts.relations,
            [
                RelationFact {
                    source: 0,
                    target: 1,
                    kind: RelationKind::Body
                },
                RelationFact {
                    source: 1,
                    target: 2,
                    kind: RelationKind::Body
                },
                RelationFact {
                    source: 1,
                    target: 3,
                    kind: RelationKind::Body
                },
                RelationFact {
                    source: 2,
                    target: 3,
                    kind: RelationKind::Parameter
                },
            ]
        );
        assert_eq!(
            facts.ordering,
            [
                OrderingEdge {
                    higher: 0,
                    lower: 1,
                    reason: OrderingReason::Call
                },
                OrderingEdge {
                    higher: 1,
                    lower: 2,
                    reason: OrderingReason::Call
                },
                OrderingEdge {
                    higher: 1,
                    lower: 3,
                    reason: OrderingReason::Encapsulation
                },
                OrderingEdge {
                    higher: 3,
                    lower: 2,
                    reason: OrderingReason::SignatureContract
                },
            ]
        );
    }

    #[test]
    fn findings_attach_to_their_item() {
        let facts = only_scope(SANDWICH);

        assert_eq!(
            facts.items[2].findings,
            [FindingFact {
                rule: CallClusterRule::CalleeBeforeCaller,
                related: "Frame".to_owned(),
                reason: Some(OrderingReason::SignatureContract),
                anchor: PlacementAnchor {
                    name: "Frame".to_owned(),
                    line: 16,
                },
            }]
        );
        assert!(
            facts
                .items
                .iter()
                .enumerate()
                .all(|(idx, item)| idx == 2 || item.findings.is_empty())
        );
    }

    #[test]
    fn mutual_recursion_shares_a_group() {
        let facts = only_scope(MUTUAL_RECURSION);

        assert_eq!(
            facts.items[1].recursion_group,
            facts.items[2].recursion_group
        );
        assert_ne!(
            facts.items[0].recursion_group,
            facts.items[1].recursion_group
        );
    }

    #[test]
    fn recursion_group_counts_as_one_level() {
        let facts = only_scope(MUTUAL_RECURSION);

        assert_eq!(facts.items[1].level, 0);
        assert_eq!(facts.items[2].level, 0);
        assert_eq!(facts.items[0].level, 1);
    }

    const RUST_TYPE_PARTS: &str = indoc::indoc! {r"
        pub struct Config {
            header: Header,
        }

        struct Header {
            len: usize,
        }

        impl Config {
            pub fn size(&self) -> usize {
                measure(&self.header)
            }
        }

        fn measure(header: &Header) -> usize {
            header.len
        }
    "};

    const PYTHON_TYPE_PARTS: &str = indoc::indoc! {r"
        from __future__ import annotations


        class Config:
            header: Header

            def size(self) -> int:
                return _measure(self.header)


        class Header:
            length: int


        def _measure(header: Header) -> int:
            return header.length
    "};

    fn relations_from(facts: &ScopeFacts, source: usize) -> Vec<(usize, RelationKind)> {
        facts
            .relations
            .iter()
            .filter(|relation| relation.source == source)
            .map(|relation| (relation.target, relation.kind))
            .collect()
    }

    #[test]
    fn rust_type_splits_composition_from_behavior() {
        let facts = collect_scope_facts(&ParsedFile::rust(RUST_TYPE_PARTS)).remove(0);

        assert_eq!(
            relations_from(&facts, 0),
            [(1, RelationKind::Composition), (2, RelationKind::Behavior)]
        );
    }

    #[test]
    fn python_class_splits_composition_from_behavior() {
        let mut scopes = collect_scope_facts(&ParsedFile::new(PYTHON_TYPE_PARTS, Language::Python));
        let facts = scopes.remove(0);

        assert_eq!(
            relations_from(&facts, 0),
            [(1, RelationKind::Composition), (2, RelationKind::Behavior)]
        );
    }

    #[test]
    fn type_reached_only_from_impl_is_behavior_not_containment() {
        let facts = collect_scope_facts(&ParsedFile::rust(indoc::indoc! {r"
            pub struct Report;

            impl Report {
                pub fn render(&self) -> usize {
                    Cache::default().len
                }
            }

            #[derive(Default)]
            struct Cache {
                len: usize,
            }
        "}))
        .remove(0);

        assert_eq!(
            facts.ordering,
            [OrderingEdge {
                higher: 0,
                lower: 1,
                reason: OrderingReason::ImplBehavior
            }]
        );
    }

    #[test]
    fn cycle_through_public_entrypoint_terminates_with_one_level() {
        let facts = only_scope(indoc::indoc! {r"
            pub fn evaluate(depth: usize) -> usize {
                if depth == 0 { 0 } else { step_one(depth) }
            }

            fn step_two(depth: usize) -> usize {
                evaluate(depth - 1)
            }

            fn step_one(depth: usize) -> usize {
                step_two(depth)
            }
        "});

        assert!(facts.items.iter().all(|item| item.level == 0));
        assert!(
            facts
                .items
                .iter()
                .all(|item| item.recursion_group == facts.items[0].recursion_group)
        );
    }
}
