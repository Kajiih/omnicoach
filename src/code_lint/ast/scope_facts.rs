//! Per-file declaration-scope facts extracted from a [`ParsedFile`].
//!
//! Collects each scope's items, raw reference relations, `callee-before-caller` ordering edges,
//! public-root ownership, recursion groups, topological levels, and ordering findings for a single
//! parsed file. Multi-file project resolution lives in `crate::code_lint::graph`.

use super::call_clusters::{
    CallClusterRule, DeclarationScope, DeclarationScopeKind, OrderingEdge, OrderingReason,
    PlacementAnchor, RelationFact, RelationKind, ScopeGraph, ScopeItem, ScopeItemKind, Visibility,
    collect_graph_declaration_scopes, scope_findings,
};
use super::tree::ParsedFile;
use crate::diagnostic::Language;
use std::collections::HashSet;

/// Everything the analyzer knows about one scope (`Module`, `Class`, or `Impl`) in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFacts {
    /// Which syntactic container this scope covers.
    pub scope: DeclarationScopeKind,
    /// Declarations in source order. Every index in [`ScopeFacts`] (`roots`, `source`, `target`,
    /// `higher`, `lower`) indexes into this slice.
    pub items: Vec<ItemFacts>,
    /// Direct intra-scope references, labeled by where the reference occurs.
    pub relations: Vec<RelationFact>,
    /// Downward `callee-before-caller` ordering edges (`higher -> lower`), labeled by why `higher`
    /// sits above `lower`.
    pub ordering: Vec<OrderingEdge>,
}

/// One declaration in [`ScopeFacts::items`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFacts {
    /// Identifier name of the declaration.
    pub name: String,
    /// Callable (`fn` / `def`) or type (`struct`, `enum`, `union`, `trait`, `class`).
    pub kind: ScopeItemKind,
    /// Public or private visibility tier.
    pub visibility: Visibility,
    /// True if this callable is a lifecycle constructor (`__init__`, `pub fn new`, etc.).
    pub is_constructor: bool,
    /// True if this method takes an instance or class receiver (`self` / `cls`).
    pub has_receiver: bool,
    /// 1-based `(start_line, end_line)` spans of `impl` blocks attached to this type declaration.
    pub impl_spans: Vec<(usize, usize)>,
    /// 1-based first line of the declaration (including decorators or outer attributes).
    pub start_line: usize,
    /// 1-based line of the declaration's name identifier.
    pub line: usize,
    /// 1-based last line of the declaration, including its attached `impl` blocks or grouped
    /// overloads and accessors.
    pub end_line: usize,
    /// 1-based column of the declaration's name identifier.
    pub column: usize,
    /// Compact one-line signature header (parameters + return type for callables; public fields or
    /// variants for types), when available.
    pub signature: Option<String>,
    /// Sorted indices in [`ScopeFacts::items`] of the public entrypoint units that reach this item
    /// through private references (`Roots(f)` with constructors collapsed to the scope's first
    /// constructor).
    pub roots: Vec<usize>,
    /// Strongly connected component id over [`ScopeFacts::ordering`]. Mutual recursion shares an id.
    pub recursion_group: usize,
    /// Topological level in [`ScopeFacts::ordering`]: `0` for leaves, `1 + max(callee.level)` above
    /// them, with a recursion group sharing one level.
    pub level: usize,
    /// Call-cluster ordering findings reported on this declaration, in rule priority order.
    pub findings: Vec<FindingFact>,
}

/// A call-cluster lint finding attached to an [`ItemFacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingFact {
    /// Call-cluster placement rule violated by this item.
    pub rule: CallClusterRule,
    /// Name of the owning or calling declaration the finding relates to.
    pub related: String,
    /// Why `related` sits above this item (`callee-before-caller` only).
    pub reason: Option<OrderingReason>,
    /// Sibling declaration next to which this item belongs.
    pub anchor: PlacementAnchor,
}

/// A resolved intra-file reference between two declarations in the same file, with its 1-based
/// source line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntraFileReference {
    /// Enclosing declaration name (`"fn_name"`, `"Type"`, or `"Type::method"`).
    pub source_symbol: String,
    /// Target declaration name (`"fn_name"`, `"Type"`, or `"Type::method"`).
    pub target_symbol: String,
    /// 1-based line number of the reference site in the file.
    pub line: usize,
    /// Syntactic role of the reference (`Parameter`, `Return`, `Body`, or `Composition`).
    pub kind: RelationKind,
}

/// Collects [`ScopeFacts`] for every coalesced top-level module and type scope in `file`, in
/// source order.
#[must_use]
pub fn collect_scope_facts(file: &ParsedFile) -> Vec<ScopeFacts> {
    collect_graph_declaration_scopes(file)
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
                has_receiver: item.has_receiver,
                impl_spans: item.impl_spans.clone(),
                start_line: item.start_line,
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
        let related = finding.related_name(scope);
        item_facts[finding.item].findings.push(FindingFact {
            rule: finding.rule,
            related,
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

/// Collects resolved intra-file references between module declarations and methods in `file`,
/// preserving the 1-based source line of each reference site.
#[must_use]
pub fn collect_intra_file_references(
    file: &ParsedFile,
    scopes: &[ScopeFacts],
) -> Vec<IntraFileReference> {
    let mut module_items: HashSet<&str> = HashSet::new();
    let mut module_types: HashSet<&str> = HashSet::new();
    let mut method_items: HashSet<String> = HashSet::new();
    for scope in scopes {
        match &scope.scope {
            DeclarationScopeKind::Module => {
                for item in &scope.items {
                    module_items.insert(item.name.as_str());
                    if item.kind == ScopeItemKind::Type {
                        module_types.insert(item.name.as_str());
                    }
                }
            }
            DeclarationScopeKind::Class { name }
            | DeclarationScopeKind::Impl { type_name: name } => {
                for item in &scope.items {
                    method_items.insert(format!("{name}::{}", item.name));
                }
            }
        }
    }
    let raw_sites = match file.lang() {
        Language::Python => {
            super::python::collect_intra_file_ref_sites(file, &module_items, &method_items)
        }
        Language::Rust => super::rust::collect_intra_file_ref_sites(
            file,
            &module_items,
            &module_types,
            &method_items,
        ),
    };
    let mut refs: Vec<IntraFileReference> = raw_sites
        .into_iter()
        .filter(|site| {
            let source_owner = site
                .source_symbol
                .split("::")
                .next()
                .unwrap_or(site.source_symbol.as_str());
            site.source_symbol != site.target_symbol && source_owner != site.target_symbol
        })
        .collect();
    refs.sort();
    refs.dedup();
    refs
}

/// Resolves the most specific declaration symbol (`"Type::method"`, `"Type"`, or `"fn_name"`)
/// enclosing 1-based `line` across `scopes`.
#[must_use]
pub fn enclosing_scope_symbol(scopes: &[ScopeFacts], line: usize) -> Option<String> {
    for scope in scopes {
        if let DeclarationScopeKind::Class { name } | DeclarationScopeKind::Impl { type_name: name } =
            &scope.scope
            && let Some(item) = scope
                .items
                .iter()
                .find(|item| item.start_line <= line && line <= item.end_line)
        {
            return Some(format!("{name}::{}", item.name));
        }
    }
    for scope in scopes {
        if scope.scope == DeclarationScopeKind::Module
            && let Some(item) = scope.items.iter().find(|item| {
                item.kind == ScopeItemKind::Callable
                    && item.start_line <= line
                    && line <= item.end_line
            })
        {
            return Some(item.name.clone());
        }
    }
    for scope in scopes {
        if scope.scope == DeclarationScopeKind::Module
            && let Some(item) = scope.items.iter().find(|item| {
                item.impl_spans
                    .iter()
                    .any(|&(start, end)| start <= line && line <= end)
            })
        {
            return Some(item.name.clone());
        }
    }
    for scope in scopes {
        if scope.scope == DeclarationScopeKind::Module
            && let Some(item) = scope
                .items
                .iter()
                .filter(|item| item.start_line <= line && line <= item.end_line)
                .max_by_key(|item| item.start_line)
        {
            return Some(item.name.clone());
        }
    }
    None
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

    #[test]
    fn python_intra_file_references_include_class_composition_and_method_decorators() {
        let file = ParsedFile::new(PYTHON_TYPE_PARTS, Language::Python);
        let scopes = collect_scope_facts(&file);
        let refs = collect_intra_file_references(&file, &scopes);

        assert_eq!(
            refs,
            vec![
                IntraFileReference {
                    source_symbol: "Config".to_owned(),
                    target_symbol: "Header".to_owned(),
                    line: 5,
                    kind: RelationKind::Composition,
                },
                IntraFileReference {
                    source_symbol: "Config::size".to_owned(),
                    target_symbol: "_measure".to_owned(),
                    line: 8,
                    kind: RelationKind::Body,
                },
                IntraFileReference {
                    source_symbol: "_measure".to_owned(),
                    target_symbol: "Header".to_owned(),
                    line: 15,
                    kind: RelationKind::Parameter,
                },
            ]
        );
    }

    #[test]
    fn rust_conditional_redefinitions_coalesce_without_leaking_inline_mod() {
        let source = indoc::indoc! {r"
            mod nested {
                pub fn internal_only() -> usize { 9 }
            }

            #[cfg(unix)]
            pub fn open_handle() -> usize {
                shared_leaf()
            }

            #[cfg(windows)]
            pub fn open_handle() -> usize {
                2
            }

            pub fn run_after_cfg() -> usize {
                open_handle() + trailing_helper()
            }

            fn trailing_helper() -> usize {
                3
            }

            fn shared_leaf() -> usize {
                4
            }
        "};
        let scopes = collect_scope_facts(&ParsedFile::rust(source));
        let summary: Vec<_> = scopes[0]
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.roots.as_slice(), item.level))
            .collect();

        assert_eq!(scopes.len(), 1);
        assert_eq!(
            summary,
            [
                ("open_handle", &[0][..], 1),
                ("run_after_cfg", &[1][..], 2),
                ("trailing_helper", &[1][..], 0),
                ("shared_leaf", &[0][..], 0),
            ]
        );
    }

    #[test]
    fn python_conditional_class_definitions_coalesce_methods() {
        let source = indoc::indoc! {r#"
            import sys

            def helper() -> None:
                class InnerLocal:
                    def ignored(self) -> None:
                        pass

            if sys.platform == "linux":
                class Client:
                    def connect(self) -> None:
                        pass
            else:
                class Client:
                    def connect(self) -> None:
                        pass

                    def reconnect(self) -> None:
                        self.connect()
        "#};
        let scopes = collect_scope_facts(&ParsedFile::new(source, Language::Python));
        let client_scope = scopes
            .iter()
            .find(|scope| matches!(&scope.scope, DeclarationScopeKind::Class { name } if name == "Client"))
            .expect("expected Client class scope");
        let methods: Vec<_> = client_scope
            .items
            .iter()
            .map(|item| (item.name.as_str(), item.level))
            .collect();

        assert_eq!(scopes[0].scope, DeclarationScopeKind::Module);
        assert_eq!(methods, [("connect", 0), ("reconnect", 1)]);
        assert!(!scopes.iter().any(|scope| {
            matches!(&scope.scope, DeclarationScopeKind::Class { name } if name == "InnerLocal")
        }));
    }
}
