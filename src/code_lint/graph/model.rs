//! Layer L1: tool-agnostic abstraction graph model with validated constructors.
//!
//! Enforces the invariants in `docs/dev/abstraction_graph/03_design.md` §5.2 so no adapter ever
//! sees an invalid graph, and knows nothing about `omnicoach` internals or external viewers (D19,
//! D21).

use crate::code_lint::ast::{FindingFact, OrderingReason, RelationKind, Visibility};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Unique identifier of a declaration or group node within an [`AbstractionGraph`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub String);

impl NodeId {
    /// Creates a node identifier from a string slice.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the underlying identifier string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Source file and 1-indexed position of a declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// Relative path of the source file.
    pub file: String,
    /// 1-indexed line number.
    pub line: usize,
    /// 1-indexed column number.
    pub column: usize,
    /// 1-indexed ending line number of the declaration span.
    pub end_line: usize,
}

/// Kind of declaration represented by a leaf node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeclarationKind {
    /// A module-level function or callable.
    Callable,
    /// A method inside an `impl` block or `class`.
    Method,
    /// A struct, enum, union, trait, or class.
    Type,
}

/// A leaf node representing one source declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationNode {
    /// Unique identifier within the graph.
    pub id: NodeId,
    /// Unqualified or qualified display name.
    pub name: String,
    /// Optional compact language signature or type definition header.
    pub signature: Option<String>,
    /// Identifier of the declaration scope this item came from.
    pub scope_id: usize,
    /// Callable, method, or type.
    pub kind: DeclarationKind,
    /// Public or private.
    pub visibility: Visibility,
    /// Whether this callable is a constructor (`new`, `__init__`, etc.).
    pub is_constructor: bool,
    /// File, line, and column of the declaration name.
    pub location: SourceLocation,
    /// Public declaration IDs that reach this declaration through private declarations only.
    pub owners: Vec<NodeId>,
    /// Recursion group identifier within `scope_id`.
    pub recursion_group: usize,
    /// Lakos abstraction level within `scope_id` (0 = leaf).
    pub level: usize,
    /// Lint findings reported on this declaration.
    pub findings: Vec<FindingFact>,
}

/// Semantic role of a container group in the containment tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKind {
    /// A declared or inferred architectural component containing modules.
    Component {
        /// Explicitly declared dependency component names from `define_architecture!`, when
        /// the project declares an architectural DAG.
        declared_dependencies: Option<Vec<String>>,
    },
    /// A module or file containing owner/shared/unowned groups or declarations.
    Module {
        /// Module path (such as `code_lint::rules::callee_before_caller`).
        path: String,
    },
    /// A public entrypoint or public type `root` together with its exclusive private helpers.
    Owner {
        /// Identifier of the owning public declaration.
        root: NodeId,
    },
    /// Private helpers shared by two or more public owners (`owners.len() >= 2`).
    Shared {
        /// Sorted identifiers of the public declarations that share these helpers.
        owners: Vec<NodeId>,
    },
    /// Private declarations not reachable from any public item in the scope.
    Unowned,
    /// Methods belonging to a `Type` declaration (`type_id`).
    TypeMembers {
        /// Identifier of the `Type` declaration whose methods are inside this group.
        type_id: NodeId,
    },
    /// Cycle of two or more mutually dependent declarations.
    RecursionGroup {
        /// Identifier of the recursion group within its scope.
        group_id: usize,
    },
}

/// A container node in the containment forest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNode {
    /// Unique identifier within the graph.
    pub id: NodeId,
    /// Display label for the group.
    pub label: String,
    /// Semantic kind of group.
    pub kind: GroupKind,
    /// Component-level recursion group identifier (for top-level groups).
    pub recursion_group: usize,
    /// Abstraction level of the group within its parent scope.
    pub level: usize,
    /// Direct child node IDs (either [`GroupNode`]s or [`DeclarationNode`]s).
    pub children: Vec<NodeId>,
}

/// Why one declaration sits at a higher abstraction level than another (ADR 011 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeReason {
    /// A callable calls another callable.
    Call,
    /// A type's own definition contains another type.
    Containment,
    /// A type's methods or `impl` block use another declaration.
    ImplBehavior,
    /// A callable encapsulates a private type inside its body only.
    Encapsulation,
    /// A type is a parameter or return contract of a callable.
    SignatureContract,
    /// A cross-module reference or dependency between items, modules, or components.
    CrossModule,
}

impl From<OrderingReason> for EdgeReason {
    fn from(reason: OrderingReason) -> Self {
        match reason {
            OrderingReason::Call => Self::Call,
            OrderingReason::Containment => Self::Containment,
            OrderingReason::ImplBehavior => Self::ImplBehavior,
            OrderingReason::Encapsulation => Self::Encapsulation,
            OrderingReason::SignatureContract => Self::SignatureContract,
        }
    }
}

/// Status of a component-to-component summary edge under Murphy et al.'s Reflexion Model (D30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReflexionStatus {
    /// Declared in `define_architecture!` and observed in source (`count > 0`).
    Convergent,
    /// Declared in `define_architecture!` with zero observed references (`count == 0`).
    Absent,
    /// Not permitted by `define_architecture!` yet observed in source (`count > 0`).
    Divergent,
    /// Inferred from source references when no `define_architecture!` declaration exists.
    Inferred,
}

/// An abstraction-ordering edge between two declarations (`higher` is above `lower`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderingEdge {
    /// Higher-level declaration ID.
    pub higher: NodeId,
    /// Lower-level declaration ID.
    pub lower: NodeId,
    /// ADR 011 §3.4 ordering reason.
    pub reason: EdgeReason,
}

/// A raw syntactic reference from `source` to `target`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReferenceEdge {
    /// Referencing declaration ID.
    pub source: NodeId,
    /// Referenced declaration ID.
    pub target: NodeId,
    /// Syntactic site of the reference.
    pub kind: RelationKind,
}

/// Aggregated relation between two container groups in the folded overview (CP3, CUJ4, D30).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SummaryEdge {
    /// Higher-level group ID.
    pub higher: NodeId,
    /// Lower-level group ID.
    pub lower: NodeId,
    /// Ordering reason being summarized.
    pub reason: EdgeReason,
    /// Number of underlying declaration-to-declaration or cross-module edges of this reason.
    pub count: usize,
    /// Optional 3-way Reflexion classification for component-to-component edges.
    pub reflexion: Option<ReflexionStatus>,
}

/// Invariant violation rejected by [`AbstractionGraph::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    /// A node ID is empty.
    EmptyNodeId,
    /// Two nodes share the same identifier.
    DuplicateNodeId(NodeId),
    /// A relation references a node ID that does not exist in the expected node set.
    UnknownRelationEndpoint(NodeId),
    /// A group lists a child ID that does not exist.
    UnknownChild {
        /// Group listing the unknown child.
        group: NodeId,
        /// Missing child ID.
        child: NodeId,
    },
    /// A node is listed as a child of more than one group (or twice in the same group).
    NodeInMultipleGroups(NodeId),
    /// Group containment forms a cycle.
    ContainmentCycle(NodeId),
    /// A private item with two or more public owners was placed inside a single owner group.
    SharedItemInSingleOwner {
        /// Shared private declaration ID.
        item: NodeId,
        /// Single owner declaration ID.
        owner: NodeId,
    },
    /// A private item was placed inside an owner group that is not its single owner.
    PrivateItemInWrongOwner {
        /// Private declaration ID.
        item: NodeId,
        /// Owner declaration ID of the group.
        owner: NodeId,
    },
    /// A shared group has fewer than two owners or contains a declaration with different owners.
    InvalidSharedGroup(NodeId),
    /// An ordering edge points upward against abstraction levels outside a recursion group.
    UpwardOrderingRelation {
        /// Declared higher-level node ID.
        higher: NodeId,
        /// Declared lower-level node ID.
        lower: NodeId,
        /// Level of `higher`.
        higher_level: usize,
        /// Level of `lower`.
        lower_level: usize,
    },
    /// A summary edge has `count == 0`.
    EmptySummaryRelation {
        /// Higher group ID.
        higher: NodeId,
        /// Lower group ID.
        lower: NodeId,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyNodeId => formatter.write_str("node identifier must not be empty"),
            Self::DuplicateNodeId(id) => write!(formatter, "duplicate node identifier: {id}"),
            Self::UnknownRelationEndpoint(id) => {
                write!(formatter, "relation references unknown node: {id}")
            }
            Self::UnknownChild { group, child } => {
                write!(formatter, "group {group} references unknown child {child}")
            }
            Self::NodeInMultipleGroups(id) => {
                write!(formatter, "node {id} is placed in multiple groups")
            }
            Self::ContainmentCycle(id) => {
                write!(formatter, "containment cycle involving group {id}")
            }
            Self::SharedItemInSingleOwner { item, owner } => {
                write!(
                    formatter,
                    "shared item {item} cannot be placed inside single owner {owner}"
                )
            }
            Self::PrivateItemInWrongOwner { item, owner } => {
                write!(
                    formatter,
                    "private item {item} does not belong to single owner {owner}"
                )
            }
            Self::InvalidSharedGroup(id) => {
                write!(
                    formatter,
                    "shared group {id} has invalid owners or member ownership"
                )
            }
            Self::UpwardOrderingRelation {
                higher,
                lower,
                higher_level,
                lower_level,
            } => write!(
                formatter,
                "ordering relation {higher} (level {higher_level}) -> {lower} (level {lower_level}) points upward outside a recursion group"
            ),
            Self::EmptySummaryRelation { higher, lower } => {
                write!(
                    formatter,
                    "summary relation {higher} -> {lower} has zero count"
                )
            }
        }
    }
}

impl std::error::Error for ModelError {}

/// Unvalidated components passed to [`AbstractionGraph::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphParts {
    /// Title or path of the scope being modeled.
    pub title: String,
    /// Leaf declaration nodes.
    pub declarations: Vec<DeclarationNode>,
    /// Container group nodes.
    pub groups: Vec<GroupNode>,
    /// Abstraction ordering edges between declarations.
    pub ordering_edges: Vec<OrderingEdge>,
    /// Raw syntactic reference edges between declarations.
    pub reference_edges: Vec<ReferenceEdge>,
    /// Aggregated summary edges between top-level groups.
    pub summary_edges: Vec<SummaryEdge>,
}

/// Validated, tool-agnostic abstraction graph (Layer L1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbstractionGraph {
    title: String,
    declarations: Vec<DeclarationNode>,
    groups: Vec<GroupNode>,
    ordering_edges: Vec<OrderingEdge>,
    reference_edges: Vec<ReferenceEdge>,
    summary_edges: Vec<SummaryEdge>,
    parent_by_child: BTreeMap<NodeId, NodeId>,
}

impl AbstractionGraph {
    /// Validates `parts` against all §5.2 model invariants and constructs an [`AbstractionGraph`].
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if any invariant in `03_design.md` §5.2 is violated.
    pub fn new(parts: GraphParts) -> Result<Self, ModelError> {
        let decl_by_id = index_declarations(&parts.declarations)?;
        let group_by_id = index_groups(&parts.groups, &decl_by_id)?;
        let parent_by_child = validate_containment(&parts.groups, &decl_by_id, &group_by_id)?;
        validate_ownership_groups(&parts.groups, &decl_by_id)?;
        validate_relations(
            &parts.ordering_edges,
            &parts.reference_edges,
            &parts.summary_edges,
            &decl_by_id,
            &group_by_id,
            &parent_by_child,
        )?;
        Ok(Self {
            title: parts.title,
            declarations: parts.declarations,
            groups: parts.groups,
            ordering_edges: parts.ordering_edges,
            reference_edges: parts.reference_edges,
            summary_edges: parts.summary_edges,
            parent_by_child,
        })
    }

    /// Title or source path of the graph.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Leaf declaration nodes in deterministic order.
    #[must_use]
    pub fn declarations(&self) -> &[DeclarationNode] {
        &self.declarations
    }

    /// Container group nodes in deterministic order.
    #[must_use]
    pub fn groups(&self) -> &[GroupNode] {
        &self.groups
    }

    /// Validated abstraction-ordering edges between declarations.
    #[must_use]
    pub fn ordering_edges(&self) -> &[OrderingEdge] {
        &self.ordering_edges
    }

    /// Validated raw reference edges between declarations.
    #[must_use]
    pub fn reference_edges(&self) -> &[ReferenceEdge] {
        &self.reference_edges
    }

    /// Validated summary edges between container groups.
    #[must_use]
    pub fn summary_edges(&self) -> &[SummaryEdge] {
        &self.summary_edges
    }

    /// Returns the parent group ID of `node_id`, if it is nested inside a group.
    #[must_use]
    pub fn parent_of(&self, node_id: &NodeId) -> Option<&NodeId> {
        self.parent_by_child.get(node_id)
    }
}

fn index_declarations(
    declarations: &[DeclarationNode],
) -> Result<BTreeMap<NodeId, &DeclarationNode>, ModelError> {
    let mut by_id = BTreeMap::new();
    for decl in declarations {
        if decl.id.as_str().is_empty() {
            return Err(ModelError::EmptyNodeId);
        }
        if by_id.insert(decl.id.clone(), decl).is_some() {
            return Err(ModelError::DuplicateNodeId(decl.id.clone()));
        }
    }
    Ok(by_id)
}

fn index_groups<'a>(
    groups: &'a [GroupNode],
    decl_by_id: &BTreeMap<NodeId, &DeclarationNode>,
) -> Result<BTreeMap<NodeId, &'a GroupNode>, ModelError> {
    let mut by_id = BTreeMap::new();
    for group in groups {
        if group.id.as_str().is_empty() {
            return Err(ModelError::EmptyNodeId);
        }
        if decl_by_id.contains_key(&group.id) || by_id.insert(group.id.clone(), group).is_some() {
            return Err(ModelError::DuplicateNodeId(group.id.clone()));
        }
    }
    Ok(by_id)
}

fn validate_containment(
    groups: &[GroupNode],
    decl_by_id: &BTreeMap<NodeId, &DeclarationNode>,
    group_by_id: &BTreeMap<NodeId, &GroupNode>,
) -> Result<BTreeMap<NodeId, NodeId>, ModelError> {
    let mut parent_by_child = BTreeMap::new();
    for group in groups {
        for child in &group.children {
            if !decl_by_id.contains_key(child) && !group_by_id.contains_key(child) {
                return Err(ModelError::UnknownChild {
                    group: group.id.clone(),
                    child: child.clone(),
                });
            }
            if parent_by_child
                .insert(child.clone(), group.id.clone())
                .is_some()
            {
                return Err(ModelError::NodeInMultipleGroups(child.clone()));
            }
        }
    }
    for group in groups {
        let mut visited = BTreeSet::new();
        let mut cursor = Some(&group.id);
        while let Some(current) = cursor {
            if !visited.insert(current.clone()) {
                return Err(ModelError::ContainmentCycle(group.id.clone()));
            }
            cursor = parent_by_child.get(current);
        }
    }
    Ok(parent_by_child)
}

fn validate_ownership_groups(
    groups: &[GroupNode],
    decl_by_id: &BTreeMap<NodeId, &DeclarationNode>,
) -> Result<(), ModelError> {
    for group in groups {
        match &group.kind {
            GroupKind::Owner { root } => {
                if !decl_by_id.contains_key(root) {
                    return Err(ModelError::UnknownRelationEndpoint(root.clone()));
                }
                for child_id in &group.children {
                    if let Some(decl) = decl_by_id.get(child_id)
                        && decl.visibility == Visibility::Private
                    {
                        if decl.owners.len() >= 2 {
                            return Err(ModelError::SharedItemInSingleOwner {
                                item: decl.id.clone(),
                                owner: root.clone(),
                            });
                        }
                        if decl.owners.as_slice() != [root.clone()] {
                            return Err(ModelError::PrivateItemInWrongOwner {
                                item: decl.id.clone(),
                                owner: root.clone(),
                            });
                        }
                    }
                }
            }
            GroupKind::Shared { owners } => {
                if owners.len() < 2 {
                    return Err(ModelError::InvalidSharedGroup(group.id.clone()));
                }
                for owner in owners {
                    if !decl_by_id.contains_key(owner) {
                        return Err(ModelError::UnknownRelationEndpoint(owner.clone()));
                    }
                }
                for child_id in &group.children {
                    if let Some(decl) = decl_by_id.get(child_id)
                        && decl.owners != *owners
                    {
                        return Err(ModelError::InvalidSharedGroup(group.id.clone()));
                    }
                }
            }
            GroupKind::TypeMembers { type_id } => {
                if !decl_by_id.contains_key(type_id) {
                    return Err(ModelError::UnknownRelationEndpoint(type_id.clone()));
                }
            }
            GroupKind::Unowned
            | GroupKind::RecursionGroup { .. }
            | GroupKind::Component { .. }
            | GroupKind::Module { .. } => {}
        }
    }
    Ok(())
}

fn validate_relations(
    ordering_edges: &[OrderingEdge],
    reference_edges: &[ReferenceEdge],
    summary_edges: &[SummaryEdge],
    decl_by_id: &BTreeMap<NodeId, &DeclarationNode>,
    group_by_id: &BTreeMap<NodeId, &GroupNode>,
    parent_by_child: &BTreeMap<NodeId, NodeId>,
) -> Result<(), ModelError> {
    for edge in ordering_edges {
        let higher = decl_by_id
            .get(&edge.higher)
            .ok_or_else(|| ModelError::UnknownRelationEndpoint(edge.higher.clone()))?;
        let lower = decl_by_id
            .get(&edge.lower)
            .ok_or_else(|| ModelError::UnknownRelationEndpoint(edge.lower.clone()))?;
        let same_level_domain = group_by_id.is_empty() || higher.owners == lower.owners;
        if higher.scope_id == lower.scope_id
            && same_level_domain
            && higher.recursion_group != lower.recursion_group
            && higher.level < lower.level
        {
            return Err(ModelError::UpwardOrderingRelation {
                higher: edge.higher.clone(),
                lower: edge.lower.clone(),
                higher_level: higher.level,
                lower_level: lower.level,
            });
        }
    }
    for edge in reference_edges {
        if !decl_by_id.contains_key(&edge.source) {
            return Err(ModelError::UnknownRelationEndpoint(edge.source.clone()));
        }
        if !decl_by_id.contains_key(&edge.target) {
            return Err(ModelError::UnknownRelationEndpoint(edge.target.clone()));
        }
    }
    for edge in summary_edges {
        let higher_group = group_by_id
            .get(&edge.higher)
            .ok_or_else(|| ModelError::UnknownRelationEndpoint(edge.higher.clone()))?;
        let lower_group = group_by_id
            .get(&edge.lower)
            .ok_or_else(|| ModelError::UnknownRelationEndpoint(edge.lower.clone()))?;
        if edge.count == 0 && edge.reflexion != Some(ReflexionStatus::Absent) {
            return Err(ModelError::EmptySummaryRelation {
                higher: edge.higher.clone(),
                lower: edge.lower.clone(),
            });
        }
        let same_parent = parent_by_child.get(&edge.higher) == parent_by_child.get(&edge.lower);
        if same_parent
            && edge.reflexion != Some(ReflexionStatus::Divergent)
            && higher_group.recursion_group != lower_group.recursion_group
            && higher_group.level < lower_group.level
        {
            return Err(ModelError::UpwardOrderingRelation {
                higher: edge.higher.clone(),
                lower: edge.lower.clone(),
                higher_level: higher_group.level,
                lower_level: lower_group.level,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_declaration(
        id: &str,
        visibility: Visibility,
        owners: &[&str],
        level: usize,
        recursion_group: usize,
    ) -> DeclarationNode {
        DeclarationNode {
            id: NodeId::new(id),
            name: id.to_owned(),
            signature: None,
            scope_id: 0,
            kind: DeclarationKind::Callable,
            visibility,
            is_constructor: false,
            location: SourceLocation {
                file: "src/sample.rs".to_owned(),
                line: 1,
                column: 1,
                end_line: 1,
            },
            owners: owners.iter().map(|owner| NodeId::new(*owner)).collect(),
            recursion_group,
            level,
            findings: Vec::new(),
        }
    }

    #[test]
    fn rejects_duplicate_node_identifier() {
        let first = sample_declaration("dup", Visibility::Public, &["dup"], 0, 0);
        let second = sample_declaration("dup", Visibility::Private, &["dup"], 0, 1);

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: vec![first, second],
            groups: Vec::new(),
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(result, Err(ModelError::DuplicateNodeId(NodeId::new("dup"))));
    }

    #[test]
    fn rejects_relation_with_missing_endpoint() {
        let entry = sample_declaration("entry", Visibility::Public, &["entry"], 1, 0);
        let edge = OrderingEdge {
            higher: NodeId::new("entry"),
            lower: NodeId::new("ghost"),
            reason: EdgeReason::Call,
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: vec![entry],
            groups: Vec::new(),
            ordering_edges: vec![edge],
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(
            result,
            Err(ModelError::UnknownRelationEndpoint(NodeId::new("ghost")))
        );
    }

    #[test]
    fn rejects_item_placed_in_two_groups() {
        let pub_a = sample_declaration("pub_a", Visibility::Public, &["pub_a"], 1, 0);
        let helper = sample_declaration("helper", Visibility::Private, &["pub_a"], 0, 1);
        let group_one = GroupNode {
            id: NodeId::new("g1"),
            label: "g1".to_owned(),
            kind: GroupKind::RecursionGroup { group_id: 1 },
            recursion_group: 0,
            level: 1,
            children: vec![NodeId::new("pub_a"), NodeId::new("helper")],
        };
        let group_two = GroupNode {
            id: NodeId::new("g2"),
            label: "g2".to_owned(),
            kind: GroupKind::RecursionGroup { group_id: 0 },
            recursion_group: 0,
            level: 0,
            children: vec![NodeId::new("helper")],
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: vec![pub_a, helper],
            groups: vec![group_one, group_two],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(
            result,
            Err(ModelError::NodeInMultipleGroups(NodeId::new("helper")))
        );
    }

    #[test]
    fn rejects_containment_cycle_between_groups() {
        let group_a = GroupNode {
            id: NodeId::new("ga"),
            label: "ga".to_owned(),
            kind: GroupKind::RecursionGroup { group_id: 1 },
            recursion_group: 0,
            level: 1,
            children: vec![NodeId::new("gb")],
        };
        let group_b = GroupNode {
            id: NodeId::new("gb"),
            label: "gb".to_owned(),
            kind: GroupKind::RecursionGroup { group_id: 0 },
            recursion_group: 0,
            level: 0,
            children: vec![NodeId::new("ga")],
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: Vec::new(),
            groups: vec![group_a, group_b],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(result, Err(ModelError::ContainmentCycle(NodeId::new("ga"))));
    }

    #[test]
    fn rejects_shared_item_inside_single_owner_group() {
        let pub_a = sample_declaration("pub_a", Visibility::Public, &["pub_a"], 1, 0);
        let pub_b = sample_declaration("pub_b", Visibility::Public, &["pub_b"], 1, 1);
        let shared = sample_declaration("shared", Visibility::Private, &["pub_a", "pub_b"], 0, 2);
        let owner_group = GroupNode {
            id: NodeId::new("owner:pub_a"),
            label: "pub_a".to_owned(),
            kind: GroupKind::Owner {
                root: NodeId::new("pub_a"),
            },
            recursion_group: 0,
            level: 1,
            children: vec![NodeId::new("pub_a"), NodeId::new("shared")],
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: vec![pub_a, pub_b, shared],
            groups: vec![owner_group],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(
            result,
            Err(ModelError::SharedItemInSingleOwner {
                item: NodeId::new("shared"),
                owner: NodeId::new("pub_a"),
            })
        );
    }

    #[test]
    fn rejects_private_item_inside_wrong_owner_group() {
        let pub_a = sample_declaration("pub_a", Visibility::Public, &["pub_a"], 1, 0);
        let pub_b = sample_declaration("pub_b", Visibility::Public, &["pub_b"], 1, 1);
        let helper_b = sample_declaration("helper_b", Visibility::Private, &["pub_b"], 0, 2);
        let owner_a = GroupNode {
            id: NodeId::new("owner:pub_a"),
            label: "pub_a".to_owned(),
            kind: GroupKind::Owner {
                root: NodeId::new("pub_a"),
            },
            recursion_group: 0,
            level: 1,
            children: vec![NodeId::new("pub_a"), NodeId::new("helper_b")],
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: vec![pub_a, pub_b, helper_b],
            groups: vec![owner_a],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(
            result,
            Err(ModelError::PrivateItemInWrongOwner {
                item: NodeId::new("helper_b"),
                owner: NodeId::new("pub_a"),
            })
        );
    }

    #[test]
    fn rejects_upward_ordering_relation_outside_recursion_group() {
        let low = sample_declaration("low", Visibility::Public, &["low"], 0, 0);
        let high = sample_declaration("high", Visibility::Private, &["low"], 2, 1);
        let upward_edge = OrderingEdge {
            higher: NodeId::new("low"),
            lower: NodeId::new("high"),
            reason: EdgeReason::Call,
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: vec![low, high],
            groups: Vec::new(),
            ordering_edges: vec![upward_edge],
            reference_edges: Vec::new(),
            summary_edges: Vec::new(),
        });

        assert_eq!(
            result,
            Err(ModelError::UpwardOrderingRelation {
                higher: NodeId::new("low"),
                lower: NodeId::new("high"),
                higher_level: 0,
                lower_level: 2,
            })
        );
    }

    #[test]
    fn accepts_absent_reflexion_summary_edge_with_zero_count() {
        let comp_high = GroupNode {
            id: NodeId::new("comp:Runner"),
            label: "Runner".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: None,
            },
            recursion_group: 1,
            level: 1,
            children: Vec::new(),
        };
        let comp_low = GroupNode {
            id: NodeId::new("comp:Rules"),
            label: "Rules".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: None,
            },
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let absent_edge = SummaryEdge {
            higher: NodeId::new("comp:Runner"),
            lower: NodeId::new("comp:Rules"),
            reason: EdgeReason::CrossModule,
            count: 0,
            reflexion: Some(ReflexionStatus::Absent),
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: Vec::new(),
            groups: vec![comp_high, comp_low],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: vec![absent_edge],
        });

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_non_absent_summary_edge_with_zero_count() {
        let comp_high = GroupNode {
            id: NodeId::new("comp:Runner"),
            label: "Runner".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: None,
            },
            recursion_group: 1,
            level: 1,
            children: Vec::new(),
        };
        let comp_low = GroupNode {
            id: NodeId::new("comp:Rules"),
            label: "Rules".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: None,
            },
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let zero_edge = SummaryEdge {
            higher: NodeId::new("comp:Runner"),
            lower: NodeId::new("comp:Rules"),
            reason: EdgeReason::CrossModule,
            count: 0,
            reflexion: Some(ReflexionStatus::Convergent),
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: Vec::new(),
            groups: vec![comp_high, comp_low],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: vec![zero_edge],
        });

        assert_eq!(
            result,
            Err(ModelError::EmptySummaryRelation {
                higher: NodeId::new("comp:Runner"),
                lower: NodeId::new("comp:Rules"),
            })
        );
    }

    #[test]
    fn accepts_divergent_reflexion_summary_edge_pointing_upward() {
        let comp_high = GroupNode {
            id: NodeId::new("comp:Runner"),
            label: "Runner".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: None,
            },
            recursion_group: 1,
            level: 1,
            children: Vec::new(),
        };
        let comp_low = GroupNode {
            id: NodeId::new("comp:Rules"),
            label: "Rules".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: None,
            },
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let divergent_edge = SummaryEdge {
            higher: NodeId::new("comp:Rules"),
            lower: NodeId::new("comp:Runner"),
            reason: EdgeReason::CrossModule,
            count: 2,
            reflexion: Some(ReflexionStatus::Divergent),
        };

        let result = AbstractionGraph::new(GraphParts {
            title: "test".to_owned(),
            declarations: Vec::new(),
            groups: vec![comp_high, comp_low],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: vec![divergent_edge],
        });

        assert!(result.is_ok());
    }
}
