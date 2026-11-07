//! Alpha / experimental Layer L3 text renderers (`AnnotatedStubs` and `CompactBi`).
//!
//! Maps [`AbstractionGraph`] onto the two agent-facing formats selected in
//! `docs/dev/abstraction_graph/08_stage_c_agent_formats.md`.

use super::model::{
    AbstractionGraph, DeclarationKind, DeclarationNode, EdgeReason, GroupKind, GroupNode, NodeId,
    ReflexionStatus,
};
use crate::code_lint::ast::{FindingFact, RelationKind, Visibility};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Alpha status banner included in rendered `# Scope:` headers.
const ALPHA_BANNER: &str =
    "[ALPHA — experimental, under active investigation & design; format may change at any time]";

/// Supported text output formats for `omnicoach graph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Code-stub hybrid combining language signatures and fields with `[Lk]` layers, ownership,
    /// and bidirectional edges (Stage C `F3`, default).
    #[default]
    AnnotatedStubs,
    /// Token-dense bidirectional (`-> uses` and `<- used_by`) graph DSL with `:start..end` spans
    /// without signature lines (Stage C `F2`, `--no-signatures`).
    CompactBi,
}

/// Serializes `graph` into `format`.
#[must_use]
pub(super) fn render_graph(graph: &AbstractionGraph, format: OutputFormat) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Scope: {} {ALPHA_BANNER}", graph.title());
    let idx = GraphEdgeIndex::build(graph);
    if idx.omits_module_edges() {
        let comment_prefix = match format {
            OutputFormat::AnnotatedStubs => "// ",
            OutputFormat::CompactBi => "",
        };
        let _ = writeln!(
            out,
            "{comment_prefix}Module edges omitted for {} modules; graph a single component's directory to see them.",
            idx.module_group_count
        );
    }

    if graph.groups().is_empty() {
        for decl in graph.declarations() {
            write_decl(&mut out, decl, 0, true, format, &idx);
        }
    } else {
        for group in graph
            .groups()
            .iter()
            .filter(|group| graph.parent_of(&group.id).is_none())
        {
            write_group(&mut out, group, 0, false, format, &idx);
        }
    }
    out
}

const METHOD_KIND: &str = "method";
const TYPE_KIND: &str = "type";
const CALL_REASON: &str = "call";
const CTOR_TAG: &str = " [ctor]";

pub(super) const fn format_short_reason(reason: EdgeReason) -> &'static str {
    match reason {
        EdgeReason::Call => CALL_REASON,
        EdgeReason::Containment => "contain",
        EdgeReason::ImplBehavior => "impl",
        EdgeReason::Encapsulation => "encap",
        EdgeReason::SignatureContract => "sig",
        EdgeReason::CrossModule => "xmod",
    }
}

pub(super) const fn format_short_ref_kind(kind: RelationKind) -> &'static str {
    match kind {
        RelationKind::Parameter => "ref:param",
        RelationKind::Return => "ref:ret",
        RelationKind::Body => "ref:body",
        RelationKind::Composition => "ref:comp",
        RelationKind::Behavior => "ref:behav",
    }
}

pub(super) const fn format_short_visibility(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "pub",
        Visibility::Private => "priv",
    }
}

pub(super) const fn format_short_decl_kind(kind: DeclarationKind) -> &'static str {
    match kind {
        DeclarationKind::Callable => "fn",
        DeclarationKind::Method => METHOD_KIND,
        DeclarationKind::Type => TYPE_KIND,
    }
}

pub(super) struct DeclAncestry<'a> {
    pub(super) decl_module_path: BTreeMap<&'a NodeId, &'a str>,
    pub(super) decl_component: BTreeMap<&'a NodeId, &'a str>,
    pub(super) decl_owner_label: BTreeMap<&'a NodeId, String>,
    pub(super) decl_by_mod_and_name: BTreeMap<(&'a str, &'a str), &'a NodeId>,
    pub(super) method_targets_by_type: std::collections::BTreeSet<(&'a NodeId, &'a NodeId)>,
}

impl<'a> DeclAncestry<'a> {
    pub(super) fn from_graph(
        graph: &'a AbstractionGraph,
        group_by_id: &BTreeMap<&'a NodeId, &'a GroupNode>,
    ) -> Self {
        let mut decl_module_path = BTreeMap::new();
        let mut decl_component = BTreeMap::new();
        let mut decl_owner_label = BTreeMap::new();
        let mut decl_by_mod_and_name = BTreeMap::new();
        let mut method_owner_type = BTreeMap::new();
        for decl in graph.declarations() {
            let mut cursor = graph.parent_of(&decl.id);
            while let Some(parent_id) = cursor {
                if let Some(group) = group_by_id.get(parent_id) {
                    match &group.kind {
                        GroupKind::TypeMembers { type_id }
                            if !method_owner_type.contains_key(&decl.id) =>
                        {
                            method_owner_type.insert(&decl.id, type_id);
                        }
                        GroupKind::Owner { .. } | GroupKind::Shared { .. } | GroupKind::Unowned
                            if !decl_owner_label.contains_key(&decl.id) =>
                        {
                            decl_owner_label.insert(&decl.id, group.label.clone());
                        }
                        GroupKind::Module { path } if !decl_module_path.contains_key(&decl.id) => {
                            decl_module_path.insert(&decl.id, path.as_str());
                            decl_by_mod_and_name
                                .insert((path.as_str(), decl.name.as_str()), &decl.id);
                        }
                        GroupKind::Component { .. } if !decl_component.contains_key(&decl.id) => {
                            decl_component.insert(&decl.id, group.label.as_str());
                        }
                        _ => {}
                    }
                }
                cursor = graph.parent_of(parent_id);
            }
        }
        let mut method_targets_by_type = std::collections::BTreeSet::new();
        for edge in graph.reference_edges() {
            if let Some(&type_id) = method_owner_type.get(&edge.source) {
                method_targets_by_type.insert((type_id, &edge.target));
            }
        }
        Self {
            decl_module_path,
            decl_component,
            decl_owner_label,
            decl_by_mod_and_name,
            method_targets_by_type,
        }
    }
}

pub(super) struct DirectedDeclEdge<'a> {
    pub(super) source_id: &'a NodeId,
    pub(super) target_id: &'a NodeId,
    pub(super) tag: &'static str,
}

pub(super) fn resolve_directed_decl_edges<'a>(
    graph: &'a AbstractionGraph,
    ancestry: &DeclAncestry<'a>,
) -> Vec<DirectedDeclEdge<'a>> {
    let mut directed = Vec::new();
    let mut ordering_pairs = std::collections::BTreeSet::new();
    for edge in graph.ordering_edges() {
        let (source_id, target_id) = match edge.reason {
            EdgeReason::SignatureContract => (&edge.lower, &edge.higher),
            _ => (&edge.higher, &edge.lower),
        };
        ordering_pairs.insert((source_id, target_id));
        if edge.reason == EdgeReason::ImplBehavior
            && ancestry
                .method_targets_by_type
                .contains(&(source_id, target_id))
        {
            continue;
        }
        directed.push(DirectedDeclEdge {
            source_id,
            target_id,
            tag: format_short_reason(edge.reason),
        });
    }
    for edge in graph.reference_edges() {
        if ordering_pairs.contains(&(&edge.source, &edge.target))
            || (edge.kind == RelationKind::Behavior
                && ancestry
                    .method_targets_by_type
                    .contains(&(&edge.source, &edge.target)))
        {
            continue;
        }
        directed.push(DirectedDeclEdge {
            source_id: &edge.source,
            target_id: &edge.target,
            tag: format_short_ref_kind(edge.kind),
        });
    }
    directed
}

struct GraphEdgeIndex<'a> {
    decl_by_id: BTreeMap<&'a NodeId, &'a DeclarationNode>,
    group_by_id: BTreeMap<&'a NodeId, &'a GroupNode>,
    module_group_count: usize,
    group_out_edges: BTreeMap<&'a NodeId, Vec<String>>,
    group_in_edges: BTreeMap<&'a NodeId, Vec<String>>,
    decl_out_edges: BTreeMap<&'a NodeId, Vec<String>>,
    decl_in_edges: BTreeMap<&'a NodeId, Vec<String>>,
}

impl<'a> GraphEdgeIndex<'a> {
    fn build(graph: &'a AbstractionGraph) -> Self {
        let decl_by_id: BTreeMap<&'a NodeId, &'a DeclarationNode> = graph
            .declarations()
            .iter()
            .map(|decl| (&decl.id, decl))
            .collect();
        let group_by_id: BTreeMap<&'a NodeId, &'a GroupNode> = graph
            .groups()
            .iter()
            .map(|group| (&group.id, group))
            .collect();

        let ancestry = DeclAncestry::from_graph(graph, &group_by_id);
        let mut group_out_edges: BTreeMap<&'a NodeId, Vec<String>> = BTreeMap::new();
        let mut group_in_edges: BTreeMap<&'a NodeId, Vec<String>> = BTreeMap::new();
        Self::populate_group_edges(
            graph,
            &group_by_id,
            &mut group_out_edges,
            &mut group_in_edges,
        );

        let mut decl_out_edges: BTreeMap<&'a NodeId, Vec<String>> = BTreeMap::new();
        let mut decl_in_edges: BTreeMap<&'a NodeId, Vec<String>> = BTreeMap::new();
        for edge in resolve_directed_decl_edges(graph, &ancestry) {
            let target_label = format_relative_decl(
                edge.source_id,
                edge.target_id,
                &decl_by_id,
                &ancestry.decl_module_path,
            );
            let source_label = format_relative_decl(
                edge.target_id,
                edge.source_id,
                &decl_by_id,
                &ancestry.decl_module_path,
            );
            decl_out_edges
                .entry(edge.source_id)
                .or_default()
                .push(format!("{target_label}[{}]", edge.tag));
            decl_in_edges
                .entry(edge.target_id)
                .or_default()
                .push(format!("{source_label}[{}]", edge.tag));
        }

        let module_group_count = graph
            .groups()
            .iter()
            .filter(|group| matches!(group.kind, GroupKind::Module { .. }))
            .count();
        Self {
            decl_by_id,
            group_by_id,
            module_group_count,
            group_out_edges,
            group_in_edges,
            decl_out_edges,
            decl_in_edges,
        }
    }

    /// Whether a module-depth graph (no declarations) has more than `MAX_MODULES_WITH_EDGES`
    /// modules, in which case per-module edges are left out (with a notice) to keep the
    /// output within inline tool limits; component edges are always rendered.
    fn omits_module_edges(&self) -> bool {
        self.decl_by_id.is_empty() && self.module_group_count > MAX_MODULES_WITH_EDGES
    }

    fn populate_group_edges(
        graph: &'a AbstractionGraph,
        group_by_id: &BTreeMap<&'a NodeId, &'a GroupNode>,
        group_out_edges: &mut BTreeMap<&'a NodeId, Vec<String>>,
        group_in_edges: &mut BTreeMap<&'a NodeId, Vec<String>>,
    ) {
        for edge in graph.summary_edges() {
            let higher_label = group_by_id
                .get(&edge.higher)
                .map_or_else(|| edge.higher.as_str(), |group| group.label.as_str());
            let lower_label = group_by_id
                .get(&edge.lower)
                .map_or_else(|| edge.lower.as_str(), |group| group.label.as_str());
            let reflexion_tag = edge.reflexion.map_or_else(String::new, |status| {
                format!(",{}", format_reflexion_status(status))
            });
            let tag = format!(
                "{} x{}{reflexion_tag}",
                format_short_reason(edge.reason),
                edge.count
            );
            group_out_edges
                .entry(&edge.higher)
                .or_default()
                .push(format!("{lower_label}[{tag}]"));
            group_in_edges
                .entry(&edge.lower)
                .or_default()
                .push(format!("{higher_label}[{tag}]"));
        }
    }
}

fn write_group(
    out: &mut String,
    group: &GroupNode,
    depth: usize,
    in_module: bool,
    format: OutputFormat,
    idx: &GraphEdgeIndex<'_>,
) {
    let indent = "  ".repeat(depth);
    let now_in_module = in_module || matches!(group.kind, GroupKind::Module { .. });
    match &group.kind {
        GroupKind::Component {
            declared_dependencies,
        } => {
            let deps = format_declared_dependencies(declared_dependencies.as_deref());
            let _ = writeln!(
                out,
                "\n## Component [L{}] {}{deps}",
                group.level, group.label
            );
        }
        GroupKind::Module { path } => {
            let file_suffix =
                find_group_file(group, idx).map_or_else(String::new, |file| format!(" ({file})"));
            let leading = if group.children.is_empty() { "" } else { "\n" };
            let _ = writeln!(
                out,
                "{leading}### Module [L{}] {path}{file_suffix}",
                group.level
            );
        }
        _ => {
            let kind = format_group_kind(&group.kind, &idx.decl_by_id);
            match format {
                OutputFormat::AnnotatedStubs => {
                    let _ = writeln!(out, "{indent}// === [L{}] {kind} ===", group.level);
                }
                OutputFormat::CompactBi => {
                    let _ = writeln!(out, "{indent}* [L{}] {kind}", group.level);
                }
            }
        }
    }

    let edge_indent = match &group.kind {
        GroupKind::Component { .. } | GroupKind::Module { .. } => String::new(),
        _ => "  ".repeat(depth + 1),
    };
    let comment_prefix = match format {
        OutputFormat::AnnotatedStubs => "// ",
        OutputFormat::CompactBi => "",
    };
    if !(matches!(group.kind, GroupKind::Module { .. }) && idx.omits_module_edges()) {
        if let Some(outs) = idx.group_out_edges.get(&group.id) {
            let formatted = format_group_edge_list(outs, &group.kind);
            let _ = writeln!(out, "{edge_indent}{comment_prefix}-> {formatted}");
        }
        if let Some(ins) = idx.group_in_edges.get(&group.id) {
            let formatted = format_group_edge_list(ins, &group.kind);
            let _ = writeln!(out, "{edge_indent}{comment_prefix}<- {formatted}");
        }
    }

    let next_depth = match &group.kind {
        GroupKind::Component { .. } | GroupKind::Module { .. } => 0,
        _ => depth + 1,
    };
    let truncate_empty_modules =
        count_empty_module_children(group, idx) > MAX_EMPTY_MODULES_PER_COMPONENT + 1;
    let mut empty_modules_seen = 0usize;
    let mut omitted_modules = 0usize;
    for child_id in &group.children {
        if let Some(child_group) = idx.group_by_id.get(child_id) {
            if matches!(group.kind, GroupKind::Component { .. })
                && matches!(child_group.kind, GroupKind::Module { .. })
                && child_group.children.is_empty()
            {
                empty_modules_seen += 1;
                if truncate_empty_modules && empty_modules_seen > MAX_EMPTY_MODULES_PER_COMPONENT {
                    omitted_modules += 1;
                    continue;
                }
            }
            write_group(out, child_group, next_depth, now_in_module, format, idx);
        } else if let Some(decl) = idx.decl_by_id.get(child_id) {
            write_decl(out, decl, next_depth, !now_in_module, format, idx);
        }
    }
    if omitted_modules > 0 {
        let _ = writeln!(
            out,
            "{comment_prefix}... (+{omitted_modules} more modules in {})",
            group.label
        );
    }
}

fn count_empty_module_children(group: &GroupNode, idx: &GraphEdgeIndex<'_>) -> usize {
    if !matches!(group.kind, GroupKind::Component { .. }) {
        return 0;
    }
    group
        .children
        .iter()
        .filter(|child_id| {
            idx.group_by_id.get(child_id).is_some_and(|child_group| {
                matches!(child_group.kind, GroupKind::Module { .. })
                    && child_group.children.is_empty()
            })
        })
        .count()
}

const MAX_EMPTY_MODULES_PER_COMPONENT: usize = 2;
const MAX_MODULES_WITH_EDGES: usize = 20;
const MAX_MODULE_EDGE_PEERS: usize = 2;
const MAX_COMPONENT_EDGE_PEERS: usize = 6;

fn format_group_edge_list(edges: &[String], kind: &GroupKind) -> String {
    let max_peers = match kind {
        GroupKind::Module { .. } => Some(MAX_MODULE_EDGE_PEERS),
        GroupKind::Component { .. } => Some(MAX_COMPONENT_EDGE_PEERS),
        _ => None,
    };
    if let Some(limit) = max_peers
        && edges.len() > limit + 1
    {
        let shown = edges[..limit].join(", ");
        let remaining = edges.len() - limit;
        format!("{shown}, ... (+{remaining} more)")
    } else {
        edges.join(", ")
    }
}

fn write_decl(
    out: &mut String,
    decl: &DeclarationNode,
    depth: usize,
    include_file: bool,
    format: OutputFormat,
    idx: &GraphEdgeIndex<'_>,
) {
    let indent = "  ".repeat(depth);
    let visibility_tag = format_short_visibility(decl.visibility);
    let kind_tag = format_short_decl_kind(decl.kind);
    let ctor_tag = if decl.is_constructor { CTOR_TAG } else { "" };
    let span = format_span(decl, include_file);
    let owner_names: Vec<&str> = decl
        .owners
        .iter()
        .map(|owner_id| {
            idx.decl_by_id
                .get(owner_id)
                .map_or_else(|| owner_id.as_str(), |owner| owner.name.as_str())
        })
        .collect();
    let roots = owner_names.join(", ");
    let edge_prefix = match format {
        OutputFormat::AnnotatedStubs => {
            let sig_text = decl.signature.as_deref().unwrap_or(decl.name.as_str());
            let _ = writeln!(
                out,
                "{indent}// [L{} {visibility_tag} {kind_tag}{ctor_tag} {} {span} roots=[{roots}]]",
                decl.level, decl.name
            );
            let _ = writeln!(out, "{indent}{sig_text}");
            format!("{indent}//   ")
        }
        OutputFormat::CompactBi => {
            let _ = writeln!(
                out,
                "{indent}- [L{}] {visibility_tag} {kind_tag}{ctor_tag} {} ({span}, roots=[{roots}])",
                decl.level, decl.name
            );
            "  ".repeat(depth + 1)
        }
    };
    if let Some(outs) = idx.decl_out_edges.get(&decl.id) {
        let _ = writeln!(out, "{edge_prefix}-> {}", outs.join(", "));
    }
    if let Some(ins) = idx.decl_in_edges.get(&decl.id) {
        let _ = writeln!(out, "{edge_prefix}<- {}", ins.join(", "));
    }
    for finding in &decl.findings {
        let _ = writeln!(out, "{edge_prefix}! {}", format_finding(finding));
    }
}

fn format_finding(finding: &FindingFact) -> String {
    let reason = finding.reason.map_or_else(String::new, |reason| {
        format!(" reason={}", format_short_reason(EdgeReason::from(reason)))
    });
    format!(
        "[{}] related={}{reason} anchor={}:{}",
        finding.rule, finding.related, finding.anchor.name, finding.anchor.line
    )
}

fn format_declared_dependencies(declared: Option<&[String]>) -> String {
    declared.map_or_else(String::new, |deps| {
        format!(" (depends_on=[{}])", deps.join(", "))
    })
}

fn format_relative_decl<'a>(
    from_id: &'a NodeId,
    to_id: &'a NodeId,
    decl_by_id: &BTreeMap<&'a NodeId, &'a DeclarationNode>,
    decl_module_path: &BTreeMap<&'a NodeId, &'a str>,
) -> String {
    let target_name = decl_by_id
        .get(to_id)
        .map_or_else(|| to_id.as_str(), |decl| decl.name.as_str());
    let from_mod = decl_module_path.get(from_id).copied();
    let to_mod = decl_module_path.get(to_id).copied();
    if from_mod != to_mod
        && let Some(mod_path) = to_mod
    {
        format!("{mod_path}::{target_name}")
    } else {
        target_name.to_owned()
    }
}

fn find_group_file<'a>(group: &'a GroupNode, idx: &GraphEdgeIndex<'a>) -> Option<&'a str> {
    for child_id in &group.children {
        if let Some(decl) = idx.decl_by_id.get(child_id) {
            return Some(decl.location.file.as_str());
        }
        if let Some(child_group) = idx.group_by_id.get(child_id)
            && let Some(found) = find_group_file(child_group, idx)
        {
            return Some(found);
        }
    }
    None
}

fn format_span(decl: &DeclarationNode, include_file: bool) -> String {
    let prefix = if include_file {
        decl.location.file.as_str()
    } else {
        ""
    };
    let (line, end_line) = (decl.location.line, decl.location.end_line);
    if end_line > line {
        format!("{prefix}:{line}..{end_line}")
    } else {
        format!("{prefix}:{line}")
    }
}

fn format_group_kind(kind: &GroupKind, decl_by_id: &BTreeMap<&NodeId, &DeclarationNode>) -> String {
    match kind {
        GroupKind::Component { .. } => "component".to_owned(),
        GroupKind::Module { path } => format!("module:{path}"),
        GroupKind::Owner { root } => {
            let owner_name = decl_by_id
                .get(root)
                .map_or_else(|| root.as_str(), |decl| decl.name.as_str());
            format!("owner:{owner_name}")
        }
        GroupKind::Shared { owners } => {
            let names: Vec<&str> = owners
                .iter()
                .map(|id| {
                    decl_by_id
                        .get(id)
                        .map_or_else(|| id.as_str(), |decl| decl.name.as_str())
                })
                .collect();
            format!("shared:{}", names.join("+"))
        }
        GroupKind::Unowned => "unowned".to_owned(),
        GroupKind::TypeMembers { type_id } => {
            let type_name = decl_by_id
                .get(type_id)
                .map_or_else(|| type_id.as_str(), |decl| decl.name.as_str());
            format!("type_members:{type_name}")
        }
        GroupKind::RecursionGroup { group_id } => format!("recursion_group:{group_id}"),
    }
}

const fn format_reflexion_status(status: ReflexionStatus) -> &'static str {
    match status {
        ReflexionStatus::Convergent => "convergent",
        ReflexionStatus::Absent => "absent",
        ReflexionStatus::Divergent => "divergent",
        ReflexionStatus::Inferred => "inferred",
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        GraphParts, OrderingEdge, ReferenceEdge, SourceLocation, SummaryEdge,
    };
    use super::*;
    use crate::code_lint::ast::{CallClusterRule, OrderingReason, PlacementAnchor};

    fn fixture_graph() -> AbstractionGraph {
        let entry = DeclarationNode {
            id: NodeId::new("s0:i0:entry"),
            name: "entry".to_owned(),
            signature: Some("pub fn entry(x: usize) -> bool".to_owned()),
            scope_id: 0,
            kind: DeclarationKind::Callable,
            visibility: Visibility::Public,
            is_constructor: false,
            location: SourceLocation {
                file: "src/fixture.rs".to_owned(),
                line: 10,
                column: 1,
                end_line: 18,
            },
            owners: vec![NodeId::new("s0:i0:entry")],
            recursion_group: 1,
            level: 1,
            findings: Vec::new(),
        };
        let helper = DeclarationNode {
            id: NodeId::new("s0:i1:helper"),
            name: "helper".to_owned(),
            signature: Some("fn helper(x: usize) -> bool".to_owned()),
            scope_id: 0,
            kind: DeclarationKind::Callable,
            visibility: Visibility::Private,
            is_constructor: false,
            location: SourceLocation {
                file: "src/fixture.rs".to_owned(),
                line: 20,
                column: 1,
                end_line: 25,
            },
            owners: vec![NodeId::new("s0:i0:entry")],
            recursion_group: 0,
            level: 0,
            findings: vec![FindingFact {
                rule: CallClusterRule::CalleeBeforeCaller,
                related: "entry".to_owned(),
                reason: Some(OrderingReason::Call),
                anchor: PlacementAnchor {
                    name: "entry".to_owned(),
                    line: 18,
                },
            }],
        };
        let owner_group = GroupNode {
            id: NodeId::new("owner:s0:i0:entry"),
            label: "entry".to_owned(),
            kind: GroupKind::Owner {
                root: NodeId::new("s0:i0:entry"),
            },
            recursion_group: 0,
            level: 1,
            children: vec![NodeId::new("s0:i0:entry"), NodeId::new("s0:i1:helper")],
        };
        AbstractionGraph::new(GraphParts {
            title: "src/fixture.rs".to_owned(),
            declarations: vec![entry, helper],
            groups: vec![owner_group],
            ordering_edges: vec![OrderingEdge {
                higher: NodeId::new("s0:i0:entry"),
                lower: NodeId::new("s0:i1:helper"),
                reason: EdgeReason::Call,
            }],
            reference_edges: vec![ReferenceEdge {
                source: NodeId::new("s0:i0:entry"),
                target: NodeId::new("s0:i1:helper"),
                kind: RelationKind::Body,
            }],
            summary_edges: Vec::<SummaryEdge>::new(),
        })
        .expect("valid fixture graph")
    }

    #[test]
    fn compact_bi_adapter_renders_bidirectional_edges_and_end_lines() {
        let graph = fixture_graph();

        let first = render_graph(&graph, OutputFormat::CompactBi);
        let second = render_graph(&graph, OutputFormat::CompactBi);

        assert_eq!(first, second);
        assert!(first.contains(ALPHA_BANNER));
        assert!(first.contains(":10..18"));
        assert!(first.contains("<- entry[call]"));
    }

    #[test]
    fn annotated_stubs_adapter_renders_signatures_and_used_by_edges() {
        let graph = fixture_graph();

        let first = render_graph(&graph, OutputFormat::AnnotatedStubs);
        let second = render_graph(&graph, OutputFormat::AnnotatedStubs);

        assert_eq!(first, second);
        assert!(first.contains(ALPHA_BANNER));
        assert!(first.contains("pub fn entry(x: usize) -> bool"));
        assert!(first.contains("//   <- entry[call]"));
    }

    #[test]
    fn project_renderers_emit_declared_dependencies_and_reflexion_statuses() {
        let comp_high = GroupNode {
            id: NodeId::new("comp:Runner"),
            label: "Runner".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: Some(vec!["Rules".to_owned()]),
            },
            recursion_group: 1,
            level: 1,
            children: vec![NodeId::new("mod:runner")],
        };
        let comp_low = GroupNode {
            id: NodeId::new("comp:Rules"),
            label: "Rules".to_owned(),
            kind: GroupKind::Component {
                declared_dependencies: Some(Vec::new()),
            },
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let mod_runner = GroupNode {
            id: NodeId::new("mod:runner"),
            label: "runner".to_owned(),
            kind: GroupKind::Module {
                path: "runner".to_owned(),
            },
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let graph = AbstractionGraph::new(GraphParts {
            title: "project".to_owned(),
            declarations: Vec::new(),
            groups: vec![comp_high, comp_low, mod_runner],
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: vec![SummaryEdge {
                higher: NodeId::new("comp:Runner"),
                lower: NodeId::new("comp:Rules"),
                reason: EdgeReason::CrossModule,
                count: 0,
                reflexion: Some(ReflexionStatus::Absent),
            }],
        })
        .expect("valid project fixture");

        let stubs = render_graph(&graph, OutputFormat::AnnotatedStubs);
        let compact = render_graph(&graph, OutputFormat::CompactBi);

        assert!(stubs.contains("## Component [L1] Runner (depends_on=[Rules])"));
        assert!(stubs.contains("## Component [L0] Rules (depends_on=[])"));
        assert!(stubs.contains("absent"));
        assert!(compact.contains("## Component [L1] Runner (depends_on=[Rules])"));
    }

    #[test]
    fn signature_contract_edges_render_in_usage_direction_without_duplicate_ref() {
        let frame = DeclarationNode {
            id: NodeId::new("s0:i0:Frame"),
            name: "Frame".to_owned(),
            signature: Some("pub struct Frame".to_owned()),
            scope_id: 0,
            kind: DeclarationKind::Type,
            visibility: Visibility::Public,
            is_constructor: false,
            location: SourceLocation {
                file: "src/frame.rs".to_owned(),
                line: 1,
                column: 1,
                end_line: 3,
            },
            owners: vec![NodeId::new("s0:i0:Frame")],
            recursion_group: 1,
            level: 1,
            findings: Vec::new(),
        };
        let frame_len = DeclarationNode {
            id: NodeId::new("s0:i1:frame_len"),
            name: "frame_len".to_owned(),
            signature: Some("pub fn frame_len(f: &Frame) -> usize".to_owned()),
            scope_id: 0,
            kind: DeclarationKind::Callable,
            visibility: Visibility::Public,
            is_constructor: false,
            location: SourceLocation {
                file: "src/frame.rs".to_owned(),
                line: 5,
                column: 1,
                end_line: 7,
            },
            owners: vec![NodeId::new("s0:i1:frame_len")],
            recursion_group: 0,
            level: 0,
            findings: Vec::new(),
        };
        let graph = AbstractionGraph::new(GraphParts {
            title: "src/frame.rs".to_owned(),
            declarations: vec![frame, frame_len],
            groups: Vec::new(),
            ordering_edges: vec![OrderingEdge {
                higher: NodeId::new("s0:i0:Frame"),
                lower: NodeId::new("s0:i1:frame_len"),
                reason: EdgeReason::SignatureContract,
            }],
            reference_edges: vec![ReferenceEdge {
                source: NodeId::new("s0:i1:frame_len"),
                target: NodeId::new("s0:i0:Frame"),
                kind: RelationKind::Parameter,
            }],
            summary_edges: Vec::new(),
        })
        .expect("valid sig graph");

        let rendered = render_graph(&graph, OutputFormat::AnnotatedStubs);

        assert!(rendered.contains("//   <- frame_len[sig]"));
        assert!(rendered.contains("//   -> Frame[sig]"));
        assert!(!rendered.contains("ref:param"));
    }

    #[test]
    fn group_edge_and_empty_module_truncation_apply_only_when_omitting_at_least_two_items() {
        let edges_three = vec![
            "a[xmod x1]".to_owned(),
            "b[xmod x1]".to_owned(),
            "c[xmod x1]".to_owned(),
        ];
        let edges_four = vec![
            "a[xmod x1]".to_owned(),
            "b[xmod x1]".to_owned(),
            "c[xmod x1]".to_owned(),
            "d[xmod x1]".to_owned(),
        ];
        let mod_kind = GroupKind::Module {
            path: "m".to_owned(),
        };
        assert_eq!(
            format_group_edge_list(&edges_three, &mod_kind),
            "a[xmod x1], b[xmod x1], c[xmod x1]"
        );
        assert_eq!(
            format_group_edge_list(&edges_four, &mod_kind),
            "a[xmod x1], b[xmod x1], ... (+2 more)"
        );
    }
}
