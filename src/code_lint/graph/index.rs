//! Shared structural lookup and directed-edge index over [`AbstractionGraph`] (`CodeLintGraph`).
//!
//! Built once from a validated [`AbstractionGraph`] and shared across text renderers (`render.rs`)
//! and bounded structural queries (`query.rs`).

use super::model::{
    AbstractionGraph, DeclarationNode, EdgeReason, GroupKind, GroupNode, NodeId, SummaryEdge,
};
use crate::code_lint::ast::RelationKind;
use std::collections::{BTreeMap, BTreeSet};

/// Semantic classification of a resolved directed declaration edge (`source -> target`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum DirectedEdgeKind {
    /// Derived from an [`AbstractionGraph::ordering_edges`] entry (with `SignatureContract`
    /// oriented in usage direction `callable -> parameter/return type`).
    Ordering(EdgeReason),
    /// Derived from a non-redundant [`AbstractionGraph::reference_edges`] entry.
    Reference(RelationKind),
}

impl DirectedEdgeKind {
    /// Compact bracket tag for this directed edge kind (`"call"`, `"sig"`, `"ref:param"`, etc.).
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Ordering(reason) => reason.as_str(),
            Self::Reference(relation) => match relation {
                RelationKind::Parameter => "ref:param",
                RelationKind::Return => "ref:ret",
                RelationKind::Body => "ref:body",
                RelationKind::Composition => "ref:comp",
                RelationKind::Behavior => "ref:behav",
            },
        }
    }
}

/// One directed edge incident to a declaration node (`source -> peer` or `peer -> target`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DirectedEdge<'a> {
    /// Connected peer declaration identifier.
    pub(super) peer: &'a NodeId,
    /// Kind of ordering or reference dependency.
    pub(super) kind: DirectedEdgeKind,
}

/// One usage-oriented summary edge incident to a group node (`source -> peer` or `peer -> target`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DirectedGroupEdge<'a> {
    /// Connected peer group identifier.
    pub(super) peer: &'a NodeId,
    /// Underlying [`SummaryEdge`] metadata (`reason`, `count`, `reflexion`).
    pub(super) edge: &'a SummaryEdge,
}

/// Borrowed lookup and adjacency index over an [`AbstractionGraph`].
pub(super) struct GraphIndex<'a> {
    graph: &'a AbstractionGraph,
    declarations_by_id: BTreeMap<&'a NodeId, &'a DeclarationNode>,
    groups_by_id: BTreeMap<&'a NodeId, &'a GroupNode>,
    module_group_count: usize,
    module_by_declaration: BTreeMap<&'a NodeId, &'a str>,
    component_by_declaration: BTreeMap<&'a NodeId, &'a str>,
    owner_label_by_declaration: BTreeMap<&'a NodeId, &'a str>,
    declaration_out_edges: BTreeMap<&'a NodeId, Vec<DirectedEdge<'a>>>,
    declaration_in_edges: BTreeMap<&'a NodeId, Vec<DirectedEdge<'a>>>,
    reference_lines: BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>>,
    group_out_edges: BTreeMap<&'a NodeId, Vec<DirectedGroupEdge<'a>>>,
    group_in_edges: BTreeMap<&'a NodeId, Vec<DirectedGroupEdge<'a>>>,
}

impl<'a> GraphIndex<'a> {
    /// Indexes all declarations, groups, ancestry labels, and directed edges of `graph`.
    #[must_use]
    pub(super) fn new(graph: &'a AbstractionGraph) -> Self {
        let declarations_by_id: BTreeMap<&'a NodeId, &'a DeclarationNode> = graph
            .declarations()
            .iter()
            .map(|declaration| (&declaration.id, declaration))
            .collect();
        let groups_by_id: BTreeMap<&'a NodeId, &'a GroupNode> = graph
            .groups()
            .iter()
            .map(|group| (&group.id, group))
            .collect();
        let module_group_count = graph
            .groups()
            .iter()
            .filter(|group| matches!(group.kind, GroupKind::Module { .. }))
            .count();

        let ancestry = DeclarationAncestry::build(graph, &groups_by_id);
        let declaration_adjacency =
            build_declaration_adjacency(graph, &ancestry.method_targets_by_type);
        let group_adjacency = build_group_adjacency(graph);

        Self {
            graph,
            declarations_by_id,
            groups_by_id,
            module_group_count,
            module_by_declaration: ancestry.module_by_declaration,
            component_by_declaration: ancestry.component_by_declaration,
            owner_label_by_declaration: ancestry.owner_label_by_declaration,
            declaration_out_edges: declaration_adjacency.out_edges,
            declaration_in_edges: declaration_adjacency.in_edges,
            reference_lines: declaration_adjacency.reference_lines,
            group_out_edges: group_adjacency.out_edges,
            group_in_edges: group_adjacency.in_edges,
        }
    }

    /// Underlying [`AbstractionGraph`].
    #[must_use]
    pub(super) const fn graph(&self) -> &'a AbstractionGraph {
        self.graph
    }

    /// Looks up a [`DeclarationNode`] by identifier.
    #[must_use]
    pub(super) fn declaration(&self, id: &NodeId) -> Option<&'a DeclarationNode> {
        self.declarations_by_id.get(id).copied()
    }

    /// Looks up a [`GroupNode`] by identifier.
    #[must_use]
    pub(super) fn group(&self, id: &NodeId) -> Option<&'a GroupNode> {
        self.groups_by_id.get(id).copied()
    }

    /// Number of `GroupKind::Module` nodes in the graph.
    #[must_use]
    pub(super) const fn module_group_count(&self) -> usize {
        self.module_group_count
    }

    /// Enclosing canonical module path of declaration `id`, when grouped under a module.
    #[must_use]
    pub(super) fn module_of(&self, id: &NodeId) -> Option<&'a str> {
        self.module_by_declaration.get(id).copied()
    }

    /// Enclosing component label of declaration `id`, when grouped under a component.
    #[must_use]
    pub(super) fn component_of(&self, id: &NodeId) -> Option<&'a str> {
        self.component_by_declaration.get(id).copied()
    }

    /// Enclosing intra-module cluster label (`Owner`, `Shared`, or `Unowned`) of declaration `id`.
    #[must_use]
    pub(super) fn owner_label_of(&self, id: &NodeId) -> Option<&'a str> {
        self.owner_label_by_declaration.get(id).copied()
    }

    /// Formats `id` as `<module>::<name>` when inside a module, or `<name>` otherwise.
    #[must_use]
    pub(super) fn qualified_symbol(&self, id: &NodeId) -> String {
        let Some(declaration) = self.declaration(id) else {
            return id.as_str().to_owned();
        };
        self.module_of(id).map_or_else(
            || declaration.name.clone(),
            |module_path| format!("{module_path}::{}", declaration.name),
        )
    }

    /// Formats `to_id` relative to `from_id` (`<to_module>::<name>` across modules, `<name>` within
    /// the same module).
    #[must_use]
    pub(super) fn relative_symbol(&self, from_id: &NodeId, to_id: &NodeId) -> String {
        let target_name = self
            .declaration(to_id)
            .map_or_else(|| to_id.as_str(), |declaration| declaration.name.as_str());
        let from_module = self.module_of(from_id);
        let to_module = self.module_of(to_id);
        if from_module != to_module
            && let Some(module_path) = to_module
        {
            format!("{module_path}::{target_name}")
        } else {
            target_name.to_owned()
        }
    }

    /// Outgoing `-> uses` edges from declaration `id`.
    #[must_use]
    pub(super) fn declaration_out_edges(&self, id: &NodeId) -> &[DirectedEdge<'a>] {
        self.declaration_out_edges
            .get(id)
            .map_or(&[], Vec::as_slice)
    }

    /// Incoming `<- used_by` edges into declaration `id`.
    #[must_use]
    pub(super) fn declaration_in_edges(&self, id: &NodeId) -> &[DirectedEdge<'a>] {
        self.declaration_in_edges.get(id).map_or(&[], Vec::as_slice)
    }

    /// Sorted, deduplicated 1-based source line numbers in `source` that reference `target`.
    #[must_use]
    pub(super) fn reference_lines(&self, source: &'a NodeId, target: &'a NodeId) -> &[usize] {
        self.reference_lines
            .get(&(source, target))
            .map_or(&[], Vec::as_slice)
    }

    /// Outgoing usage-oriented summary edges (`source -> peer`) from group `id`.
    #[must_use]
    pub(super) fn group_out_edges(&self, id: &NodeId) -> &[DirectedGroupEdge<'a>] {
        self.group_out_edges.get(id).map_or(&[], Vec::as_slice)
    }

    /// Incoming usage-oriented summary edges (`peer -> target`) into group `id`.
    #[must_use]
    pub(super) fn group_in_edges(&self, id: &NodeId) -> &[DirectedGroupEdge<'a>] {
        self.group_in_edges.get(id).map_or(&[], Vec::as_slice)
    }
}

struct DeclarationAncestry<'a> {
    module_by_declaration: BTreeMap<&'a NodeId, &'a str>,
    component_by_declaration: BTreeMap<&'a NodeId, &'a str>,
    owner_label_by_declaration: BTreeMap<&'a NodeId, &'a str>,
    method_targets_by_type: BTreeSet<(&'a NodeId, &'a NodeId)>,
}

impl<'a> DeclarationAncestry<'a> {
    fn build(
        graph: &'a AbstractionGraph,
        groups_by_id: &BTreeMap<&'a NodeId, &'a GroupNode>,
    ) -> Self {
        let mut module_by_declaration = BTreeMap::new();
        let mut component_by_declaration = BTreeMap::new();
        let mut owner_label_by_declaration = BTreeMap::new();
        let mut method_owner_type = BTreeMap::new();

        for declaration in graph.declarations() {
            let mut cursor = graph.parent_of(&declaration.id);
            while let Some(parent_id) = cursor {
                if let Some(group) = groups_by_id.get(parent_id) {
                    match &group.kind {
                        GroupKind::TypeMembers { type_id } => {
                            method_owner_type.entry(&declaration.id).or_insert(type_id);
                        }
                        GroupKind::Owner { .. } | GroupKind::Shared { .. } | GroupKind::Unowned => {
                            owner_label_by_declaration
                                .entry(&declaration.id)
                                .or_insert(group.label.as_str());
                        }
                        GroupKind::Module { path } => {
                            module_by_declaration
                                .entry(&declaration.id)
                                .or_insert(path.as_str());
                        }
                        GroupKind::Component { .. } => {
                            component_by_declaration
                                .entry(&declaration.id)
                                .or_insert(group.label.as_str());
                        }
                        GroupKind::RecursionGroup { .. } => {}
                    }
                }
                cursor = graph.parent_of(parent_id);
            }
        }

        let mut method_targets_by_type = BTreeSet::new();
        for edge in graph.ordering_edges() {
            let endpoints = orient_ordering_endpoints(edge.reason, &edge.higher, &edge.lower);
            if let Some(&type_id) = method_owner_type.get(endpoints.source) {
                method_targets_by_type.insert((type_id, endpoints.target));
            }
        }
        for edge in graph.reference_edges() {
            if let Some(&type_id) = method_owner_type.get(&edge.source) {
                method_targets_by_type.insert((type_id, &edge.target));
            }
        }

        Self {
            module_by_declaration,
            component_by_declaration,
            owner_label_by_declaration,
            method_targets_by_type,
        }
    }
}

struct DeclarationAdjacency<'a> {
    out_edges: BTreeMap<&'a NodeId, Vec<DirectedEdge<'a>>>,
    in_edges: BTreeMap<&'a NodeId, Vec<DirectedEdge<'a>>>,
    reference_lines: BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>>,
}

fn build_declaration_adjacency<'a>(
    graph: &'a AbstractionGraph,
    method_targets_by_type: &BTreeSet<(&'a NodeId, &'a NodeId)>,
) -> DeclarationAdjacency<'a> {
    let mut out_edges: BTreeMap<&'a NodeId, Vec<DirectedEdge<'a>>> = BTreeMap::new();
    let mut in_edges: BTreeMap<&'a NodeId, Vec<DirectedEdge<'a>>> = BTreeMap::new();
    let mut reference_lines: BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>> = BTreeMap::new();
    let mut seen_pairs = BTreeSet::new();

    for edge in graph.ordering_edges() {
        let endpoints = orient_ordering_endpoints(edge.reason, &edge.higher, &edge.lower);
        if edge.reason == EdgeReason::ImplBehavior
            && method_targets_by_type.contains(&(endpoints.source, endpoints.target))
        {
            continue;
        }
        if !seen_pairs.insert((endpoints.source, endpoints.target)) {
            continue;
        }
        let kind = DirectedEdgeKind::Ordering(edge.reason);
        out_edges
            .entry(endpoints.source)
            .or_default()
            .push(DirectedEdge {
                peer: endpoints.target,
                kind,
            });
        in_edges
            .entry(endpoints.target)
            .or_default()
            .push(DirectedEdge {
                peer: endpoints.source,
                kind,
            });
    }

    for edge in graph.reference_edges() {
        if !edge.lines.is_empty() {
            reference_lines
                .entry((&edge.source, &edge.target))
                .or_default()
                .extend_from_slice(&edge.lines);
        }
        if edge.kind == RelationKind::Behavior
            && method_targets_by_type.contains(&(&edge.source, &edge.target))
        {
            continue;
        }
        if !seen_pairs.insert((&edge.source, &edge.target)) {
            continue;
        }
        let kind = DirectedEdgeKind::Reference(edge.kind);
        out_edges
            .entry(&edge.source)
            .or_default()
            .push(DirectedEdge {
                peer: &edge.target,
                kind,
            });
        in_edges
            .entry(&edge.target)
            .or_default()
            .push(DirectedEdge {
                peer: &edge.source,
                kind,
            });
    }

    for edges in out_edges.values_mut() {
        edges.sort_unstable();
    }
    for edges in in_edges.values_mut() {
        edges.sort_unstable();
    }
    for lines in reference_lines.values_mut() {
        lines.sort_unstable();
        lines.dedup();
    }

    DeclarationAdjacency {
        out_edges,
        in_edges,
        reference_lines,
    }
}

struct GroupAdjacency<'a> {
    out_edges: BTreeMap<&'a NodeId, Vec<DirectedGroupEdge<'a>>>,
    in_edges: BTreeMap<&'a NodeId, Vec<DirectedGroupEdge<'a>>>,
}

fn build_group_adjacency(graph: &AbstractionGraph) -> GroupAdjacency<'_> {
    let mut out_edges: BTreeMap<&NodeId, Vec<DirectedGroupEdge<'_>>> = BTreeMap::new();
    let mut in_edges: BTreeMap<&NodeId, Vec<DirectedGroupEdge<'_>>> = BTreeMap::new();
    for edge in graph.summary_edges() {
        let endpoints = orient_ordering_endpoints(edge.reason, &edge.higher, &edge.lower);
        out_edges
            .entry(endpoints.source)
            .or_default()
            .push(DirectedGroupEdge {
                peer: endpoints.target,
                edge,
            });
        in_edges
            .entry(endpoints.target)
            .or_default()
            .push(DirectedGroupEdge {
                peer: endpoints.source,
                edge,
            });
    }
    for edges in out_edges.values_mut() {
        edges.sort_by_key(|entry| (entry.peer, entry.edge.reason));
    }
    for edges in in_edges.values_mut() {
        edges.sort_by_key(|entry| (entry.peer, entry.edge.reason));
    }
    GroupAdjacency {
        out_edges,
        in_edges,
    }
}

struct OrientedEndpoints<'a> {
    source: &'a NodeId,
    target: &'a NodeId,
}

const fn orient_ordering_endpoints<'a>(
    reason: EdgeReason,
    higher: &'a NodeId,
    lower: &'a NodeId,
) -> OrientedEndpoints<'a> {
    match reason {
        EdgeReason::SignatureContract => OrientedEndpoints {
            source: lower,
            target: higher,
        },
        _ => OrientedEndpoints {
            source: higher,
            target: lower,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        DeclarationKind, GraphParts, OrderingEdge, ReferenceEdge, SourceLocation,
    };
    use super::*;
    use crate::code_lint::ast::Visibility;

    fn sample_declaration(
        id: &str,
        name: &str,
        kind: DeclarationKind,
        level: usize,
    ) -> DeclarationNode {
        DeclarationNode {
            id: NodeId::new(id),
            name: name.to_owned(),
            signature: None,
            scope_id: 0,
            kind,
            visibility: Visibility::Public,
            is_constructor: false,
            location: SourceLocation {
                file: "src/sample.rs".to_owned(),
                line: 1,
                column: 1,
                end_line: 3,
            },
            owners: Vec::new(),
            recursion_group: 0,
            level,
            findings: Vec::new(),
        }
    }

    fn impl_dedup_fixture_graph() -> AbstractionGraph {
        let tracker = sample_declaration("decl:Tracker", "Tracker", DeclarationKind::Type, 2);
        let method = sample_declaration("decl:run", "Tracker::run", DeclarationKind::Method, 1);
        let helper = sample_declaration("decl:helper", "helper", DeclarationKind::Callable, 0);
        let members = GroupNode {
            id: NodeId::new("members:Tracker"),
            label: "Tracker".to_owned(),
            kind: GroupKind::TypeMembers {
                type_id: NodeId::new("decl:Tracker"),
            },
            recursion_group: 0,
            level: 1,
            children: vec![NodeId::new("decl:run")],
        };
        let owner = GroupNode {
            id: NodeId::new("owner:Tracker"),
            label: "Tracker".to_owned(),
            kind: GroupKind::Owner {
                root: NodeId::new("decl:Tracker"),
            },
            recursion_group: 0,
            level: 2,
            children: vec![
                NodeId::new("decl:Tracker"),
                NodeId::new("members:Tracker"),
                NodeId::new("decl:helper"),
            ],
        };
        let module = GroupNode {
            id: NodeId::new("mod:core"),
            label: "core".to_owned(),
            kind: GroupKind::Module {
                path: "core".to_owned(),
            },
            recursion_group: 0,
            level: 0,
            children: vec![NodeId::new("owner:Tracker")],
        };
        let component = GroupNode {
            id: NodeId::new("comp:Core"),
            label: "Core".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: Some(Vec::new()),
            },
            recursion_group: 0,
            level: 0,
            children: vec![NodeId::new("mod:core")],
        };
        AbstractionGraph::new(GraphParts {
            title: "project".to_owned(),
            declarations: vec![tracker, method, helper],
            groups: vec![component, module, owner, members],
            ordering_edges: vec![
                OrderingEdge {
                    higher: NodeId::new("decl:Tracker"),
                    lower: NodeId::new("decl:helper"),
                    reason: EdgeReason::ImplBehavior,
                },
                OrderingEdge {
                    higher: NodeId::new("decl:run"),
                    lower: NodeId::new("decl:helper"),
                    reason: EdgeReason::Call,
                },
            ],
            reference_edges: vec![
                ReferenceEdge {
                    source: NodeId::new("decl:Tracker"),
                    target: NodeId::new("decl:helper"),
                    kind: RelationKind::Behavior,
                    lines: Vec::new(),
                },
                ReferenceEdge {
                    source: NodeId::new("decl:run"),
                    target: NodeId::new("decl:helper"),
                    kind: RelationKind::Body,
                    lines: Vec::new(),
                },
            ],
            summary_edges: Vec::new(),
        })
        .expect("valid impl dedup fixture")
    }

    #[test]
    fn graph_index_deduplicates_type_impl_edges_already_represented_by_methods() {
        let graph = impl_dedup_fixture_graph();
        let index = GraphIndex::new(&graph);
        let tracker_id = NodeId::new("decl:Tracker");
        let method_id = NodeId::new("decl:run");
        let helper_id = NodeId::new("decl:helper");

        assert_eq!(index.declaration_out_edges(&tracker_id), []);
        assert_eq!(
            index.declaration_out_edges(&method_id),
            &[DirectedEdge {
                peer: &helper_id,
                kind: DirectedEdgeKind::Ordering(EdgeReason::Call),
            }]
        );
        assert_eq!(
            index.declaration_in_edges(&helper_id),
            &[DirectedEdge {
                peer: &method_id,
                kind: DirectedEdgeKind::Ordering(EdgeReason::Call),
            }]
        );
    }

    #[test]
    fn graph_index_resolves_declaration_ancestry() {
        let graph = impl_dedup_fixture_graph();
        let index = GraphIndex::new(&graph);
        let method_id = NodeId::new("decl:run");

        assert_eq!(index.module_of(&method_id), Some("core"));
        assert_eq!(index.component_of(&method_id), Some("Core"));
        assert_eq!(index.owner_label_of(&method_id), Some("Tracker"));
    }

    #[test]
    fn graph_index_formats_qualified_and_relative_symbols() {
        let graph = impl_dedup_fixture_graph();
        let index = GraphIndex::new(&graph);
        let method_id = NodeId::new("decl:run");
        let helper_id = NodeId::new("decl:helper");

        assert_eq!(index.qualified_symbol(&method_id), "core::Tracker::run");
        assert_eq!(index.relative_symbol(&method_id, &helper_id), "helper");
    }

    #[test]
    fn graph_index_formats_cross_module_and_moduleless_symbols() {
        let single_decl = sample_declaration("decl:solo", "solo", DeclarationKind::Callable, 0);
        let single_graph = AbstractionGraph::new(GraphParts {
            title: "solo.rs".to_owned(),
            declarations: vec![single_decl],
            groups: Vec::new(),
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        })
        .expect("valid single-file graph");
        let single_index = GraphIndex::new(&single_graph);

        let alpha = sample_declaration("decl:alpha", "alpha", DeclarationKind::Callable, 0);
        let beta = sample_declaration("decl:beta", "beta", DeclarationKind::Callable, 0);
        let multi_graph = AbstractionGraph::new(GraphParts {
            title: "multi".to_owned(),
            declarations: vec![alpha, beta],
            groups: vec![
                GroupNode {
                    id: NodeId::new("mod:api"),
                    label: "api".to_owned(),
                    kind: GroupKind::Module {
                        path: "api".to_owned(),
                    },
                    recursion_group: 0,
                    level: 1,
                    children: vec![NodeId::new("decl:alpha")],
                },
                GroupNode {
                    id: NodeId::new("mod:core"),
                    label: "core".to_owned(),
                    kind: GroupKind::Module {
                        path: "core".to_owned(),
                    },
                    recursion_group: 0,
                    level: 0,
                    children: vec![NodeId::new("decl:beta")],
                },
            ],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        })
        .expect("valid multi-module graph");
        let multi_index = GraphIndex::new(&multi_graph);

        assert_eq!(
            single_index.qualified_symbol(&NodeId::new("decl:solo")),
            "solo"
        );
        assert_eq!(
            multi_index.relative_symbol(&NodeId::new("decl:alpha"), &NodeId::new("decl:beta")),
            "core::beta"
        );
        assert_eq!(multi_index.module_group_count(), 2);
    }

    #[test]
    fn graph_index_deduplicates_behavior_ref_and_collapses_multi_role_reference_edges() {
        let tracker = sample_declaration("decl:Tracker", "Tracker", DeclarationKind::Type, 1);
        let method = sample_declaration("decl:run", "Tracker::run", DeclarationKind::Method, 1);
        let config_type = sample_declaration("decl:Config", "Config", DeclarationKind::Type, 0);
        let direct_dep = sample_declaration("decl:Direct", "Direct", DeclarationKind::Callable, 0);
        let solo_ref = sample_declaration("decl:SoloRef", "SoloRef", DeclarationKind::Callable, 0);
        let graph = AbstractionGraph::new(GraphParts {
            title: "refs".to_owned(),
            declarations: vec![tracker, method, config_type, direct_dep, solo_ref],
            groups: vec![GroupNode {
                id: NodeId::new("members:Tracker"),
                label: "Tracker".to_owned(),
                kind: GroupKind::TypeMembers {
                    type_id: NodeId::new("decl:Tracker"),
                },
                recursion_group: 0,
                level: 1,
                children: vec![NodeId::new("decl:run")],
            }],
            ordering_edges: vec![OrderingEdge {
                higher: NodeId::new("decl:Tracker"),
                lower: NodeId::new("decl:Direct"),
                reason: EdgeReason::ImplBehavior,
            }],
            reference_edges: vec![
                ReferenceEdge {
                    source: NodeId::new("decl:Tracker"),
                    target: NodeId::new("decl:Config"),
                    kind: RelationKind::Behavior,
                    lines: Vec::new(),
                },
                ReferenceEdge {
                    source: NodeId::new("decl:Tracker"),
                    target: NodeId::new("decl:SoloRef"),
                    kind: RelationKind::Behavior,
                    lines: Vec::new(),
                },
                ReferenceEdge {
                    source: NodeId::new("decl:run"),
                    target: NodeId::new("decl:Config"),
                    kind: RelationKind::Parameter,
                    lines: Vec::new(),
                },
                ReferenceEdge {
                    source: NodeId::new("decl:run"),
                    target: NodeId::new("decl:Config"),
                    kind: RelationKind::Return,
                    lines: Vec::new(),
                },
                ReferenceEdge {
                    source: NodeId::new("decl:run"),
                    target: NodeId::new("decl:Config"),
                    kind: RelationKind::Body,
                    lines: Vec::new(),
                },
            ],
            summary_edges: Vec::new(),
        })
        .expect("valid ref dedup graph");
        let index = GraphIndex::new(&graph);
        let direct_id = NodeId::new("decl:Direct");
        let solo_id = NodeId::new("decl:SoloRef");
        let config_id = NodeId::new("decl:Config");

        assert_eq!(
            index.declaration_out_edges(&NodeId::new("decl:Tracker")),
            &[
                DirectedEdge {
                    peer: &direct_id,
                    kind: DirectedEdgeKind::Ordering(EdgeReason::ImplBehavior),
                },
                DirectedEdge {
                    peer: &solo_id,
                    kind: DirectedEdgeKind::Reference(RelationKind::Behavior),
                },
            ]
        );
        assert_eq!(
            index.declaration_out_edges(&NodeId::new("decl:run")),
            &[DirectedEdge {
                peer: &config_id,
                kind: DirectedEdgeKind::Reference(RelationKind::Parameter),
            }]
        );
    }

    #[test]
    fn graph_index_orients_signature_contract_group_summaries_in_usage_direction() {
        let frame_group = GroupNode {
            id: NodeId::new("owner:Frame"),
            label: "Frame".to_owned(),
            kind: GroupKind::Unowned,
            recursion_group: 1,
            level: 1,
            children: Vec::new(),
        };
        let fn_group = GroupNode {
            id: NodeId::new("owner:frame_len"),
            label: "frame_len".to_owned(),
            kind: GroupKind::Unowned,
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let graph = AbstractionGraph::new(GraphParts {
            title: "sig_groups".to_owned(),
            declarations: Vec::new(),
            groups: vec![frame_group, fn_group],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: vec![SummaryEdge {
                higher: NodeId::new("owner:Frame"),
                lower: NodeId::new("owner:frame_len"),
                reason: EdgeReason::SignatureContract,
                count: 1,
                reflexion: None,
            }],
        })
        .expect("valid summary graph");
        let index = GraphIndex::new(&graph);
        let frame_id = NodeId::new("owner:Frame");
        let fn_id = NodeId::new("owner:frame_len");

        assert_eq!(index.group_out_edges(&frame_id), []);
        assert_eq!(index.group_out_edges(&fn_id)[0].peer, &frame_id);
        assert_eq!(index.group_in_edges(&frame_id)[0].peer, &fn_id);
    }
}
