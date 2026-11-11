//! Alpha / experimental Layer L3 text renderers (`AnnotatedStubs` and `CompactBi`).
//!
//! Maps [`AbstractionGraph`] onto the two agent-facing formats selected in
//! `docs/dev/abstraction_graph/08_stage_c_agent_formats.md`.

use super::index::{DirectedEdge, DirectedGroupEdge, GraphIndex};
use super::model::{AbstractionGraph, DeclarationNode, EdgeReason, GroupKind, GroupNode, NodeId};
use crate::code_lint::ast::FindingFact;
use std::fmt::Write as _;

/// Alpha status banner included in rendered `# Scope:` headers.
const ALPHA_BANNER: &str =
    "[ALPHA — experimental, under active investigation & design; format may change at any time]";

const MAX_EMPTY_MODULES_PER_COMPONENT: usize = 2;
const MAX_MODULES_WITH_EDGES: usize = 20;
const MAX_MODULE_EDGE_PEERS: usize = 2;
const MAX_COMPONENT_EDGE_PEERS: usize = 6;

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
    let graph_index = GraphIndex::new(graph);
    if omits_module_edges(&graph_index) {
        let comment_prefix = match format {
            OutputFormat::AnnotatedStubs => "// ",
            OutputFormat::CompactBi => "",
        };
        let _ = writeln!(
            out,
            "{comment_prefix}Module edges omitted for {} modules; graph a single component's directory to see them.",
            graph_index.module_group_count()
        );
    }

    if graph.groups().is_empty() {
        for declaration in graph.declarations() {
            write_decl(&mut out, declaration, 0, true, format, &graph_index);
        }
    } else {
        for group in graph
            .groups()
            .iter()
            .filter(|group| graph.parent_of(&group.id).is_none())
        {
            write_group(&mut out, group, 0, false, format, &graph_index);
        }
    }
    out
}

fn write_group(
    out: &mut String,
    group: &GroupNode,
    depth: usize,
    in_module: bool,
    format: OutputFormat,
    graph_index: &GraphIndex<'_>,
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
            let file_suffix = find_group_file(group, graph_index)
                .map_or_else(String::new, |file| format!(" ({file})"));
            let leading = if group.children.is_empty() { "" } else { "\n" };
            let _ = writeln!(
                out,
                "{leading}### Module [L{}] {path}{file_suffix}",
                group.level
            );
        }
        _ => {
            let kind = format_group_kind(&group.kind, graph_index);
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

    let comment_prefix = match format {
        OutputFormat::AnnotatedStubs => "// ",
        OutputFormat::CompactBi => "",
    };
    write_group_edges(out, group, depth, comment_prefix, graph_index);

    let next_depth = match &group.kind {
        GroupKind::Component { .. } | GroupKind::Module { .. } => 0,
        _ => depth + 1,
    };
    let truncate_empty_modules = (omits_module_edges(graph_index)
        || !graph_index.graph().declarations().is_empty())
        && count_empty_module_children(group, graph_index) > MAX_EMPTY_MODULES_PER_COMPONENT + 1;
    let mut empty_modules_seen = 0usize;
    let mut omitted_modules = 0usize;
    for child_id in &group.children {
        if let Some(child_group) = graph_index.group(child_id) {
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
            write_group(
                out,
                child_group,
                next_depth,
                now_in_module,
                format,
                graph_index,
            );
        } else if let Some(declaration) = graph_index.declaration(child_id) {
            write_decl(
                out,
                declaration,
                next_depth,
                !now_in_module,
                format,
                graph_index,
            );
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

fn format_declared_dependencies(declared: Option<&[String]>) -> String {
    declared.map_or_else(String::new, |deps| {
        format!(" (depends_on=[{}])", deps.join(", "))
    })
}

fn find_group_file<'a>(group: &'a GroupNode, graph_index: &GraphIndex<'a>) -> Option<&'a str> {
    for child_id in &group.children {
        if let Some(declaration) = graph_index.declaration(child_id) {
            return Some(declaration.location.file.as_str());
        }
        if let Some(child_group) = graph_index.group(child_id)
            && let Some(found) = find_group_file(child_group, graph_index)
        {
            return Some(found);
        }
    }
    None
}

fn format_group_kind(kind: &GroupKind, graph_index: &GraphIndex<'_>) -> String {
    match kind {
        GroupKind::Component { .. } => "component".to_owned(),
        GroupKind::Module { path } => format!("module:{path}"),
        GroupKind::Owner { root } => {
            let owner_name = graph_index
                .declaration(root)
                .map_or_else(|| root.as_str(), |declaration| declaration.name.as_str());
            format!("owner:{owner_name}")
        }
        GroupKind::Shared { owners } => {
            let names: Vec<&str> = owners
                .iter()
                .map(|id| {
                    graph_index
                        .declaration(id)
                        .map_or_else(|| id.as_str(), |declaration| declaration.name.as_str())
                })
                .collect();
            format!("shared:{}", names.join("+"))
        }
        GroupKind::Unowned => "unowned".to_owned(),
        GroupKind::TypeMembers { type_id } => {
            let type_name = graph_index
                .declaration(type_id)
                .map_or_else(|| type_id.as_str(), |declaration| declaration.name.as_str());
            format!("type_members:{type_name}")
        }
        GroupKind::RecursionGroup { group_id } => format!("recursion_group:{group_id}"),
    }
}

fn write_group_edges(
    out: &mut String,
    group: &GroupNode,
    depth: usize,
    comment_prefix: &str,
    graph_index: &GraphIndex<'_>,
) {
    if matches!(group.kind, GroupKind::Module { .. }) && omits_module_edges(graph_index) {
        return;
    }
    let edge_indent = match &group.kind {
        GroupKind::Component { .. } | GroupKind::Module { .. } => String::new(),
        _ => "  ".repeat(depth + 1),
    };
    let out_edges = graph_index.group_out_edges(&group.id);
    if !out_edges.is_empty() {
        let formatted_peers = format_summary_edges(out_edges, graph_index);
        let formatted = format_group_edge_list(&formatted_peers, &group.kind);
        let _ = writeln!(out, "{edge_indent}{comment_prefix}-> {formatted}");
    }
    let in_edges = graph_index.group_in_edges(&group.id);
    if !in_edges.is_empty() {
        let formatted_peers = format_summary_edges(in_edges, graph_index);
        let formatted = format_group_edge_list(&formatted_peers, &group.kind);
        let _ = writeln!(out, "{edge_indent}{comment_prefix}<- {formatted}");
    }
}

fn omits_module_edges(graph_index: &GraphIndex<'_>) -> bool {
    graph_index.graph().declarations().is_empty()
        && graph_index.module_group_count() > MAX_MODULES_WITH_EDGES
}

fn format_summary_edges(
    edges: &[DirectedGroupEdge<'_>],
    graph_index: &GraphIndex<'_>,
) -> Vec<String> {
    edges
        .iter()
        .map(|directed| {
            let peer_label = graph_index
                .group(directed.peer)
                .map_or_else(|| directed.peer.as_str(), |group| group.label.as_str());
            let reflexion_tag = directed
                .edge
                .reflexion
                .map_or_else(String::new, |status| format!(",{}", status.as_str()));
            format!(
                "{peer_label}[{} x{}{reflexion_tag}]",
                directed.edge.reason.as_str(),
                directed.edge.count
            )
        })
        .collect()
}

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

fn count_empty_module_children(group: &GroupNode, graph_index: &GraphIndex<'_>) -> usize {
    if !matches!(group.kind, GroupKind::Component { .. }) {
        return 0;
    }
    group
        .children
        .iter()
        .filter(|child_id| {
            graph_index.group(child_id).is_some_and(|child_group| {
                matches!(child_group.kind, GroupKind::Module { .. })
                    && child_group.children.is_empty()
            })
        })
        .count()
}

fn write_decl(
    out: &mut String,
    declaration: &DeclarationNode,
    depth: usize,
    include_file: bool,
    format: OutputFormat,
    graph_index: &GraphIndex<'_>,
) {
    let indent = "  ".repeat(depth);
    let visibility_tag = declaration.visibility.as_str();
    let kind_tag = declaration.kind.as_str();
    let ctor_tag = if declaration.is_constructor {
        " [ctor]"
    } else {
        ""
    };
    let span = format_span(declaration, include_file);
    let owner_names: Vec<&str> = declaration
        .owners
        .iter()
        .map(|owner_id| {
            graph_index
                .declaration(owner_id)
                .map_or_else(|| owner_id.as_str(), |owner| owner.name.as_str())
        })
        .collect();
    let roots = owner_names.join(", ");
    let edge_prefix = match format {
        OutputFormat::AnnotatedStubs => {
            let sig_text = declaration
                .signature
                .as_deref()
                .unwrap_or(declaration.name.as_str());
            let _ = writeln!(
                out,
                "{indent}// [L{} {visibility_tag} {kind_tag}{ctor_tag} {} {span} roots=[{roots}]]",
                declaration.level, declaration.name
            );
            let _ = writeln!(out, "{indent}{sig_text}");
            format!("{indent}//   ")
        }
        OutputFormat::CompactBi => {
            let _ = writeln!(
                out,
                "{indent}- [L{}] {visibility_tag} {kind_tag}{ctor_tag} {} ({span}, roots=[{roots}])",
                declaration.level, declaration.name
            );
            "  ".repeat(depth + 1)
        }
    };
    let out_edges = graph_index.declaration_out_edges(&declaration.id);
    if !out_edges.is_empty() {
        let formatted = format_declaration_edges(&declaration.id, out_edges, graph_index);
        let _ = writeln!(out, "{edge_prefix}-> {formatted}");
    }
    let in_edges = graph_index.declaration_in_edges(&declaration.id);
    if !in_edges.is_empty() {
        let formatted = format_declaration_edges(&declaration.id, in_edges, graph_index);
        let _ = writeln!(out, "{edge_prefix}<- {formatted}");
    }
    for finding in &declaration.findings {
        let _ = writeln!(out, "{edge_prefix}! {}", format_finding(finding));
    }
}

fn format_span(declaration: &DeclarationNode, include_file: bool) -> String {
    let prefix = if include_file {
        declaration.location.file.as_str()
    } else {
        ""
    };
    let (line, end_line) = (declaration.location.line, declaration.location.end_line);
    if end_line > line {
        format!("{prefix}:{line}..{end_line}")
    } else {
        format!("{prefix}:{line}")
    }
}

fn format_declaration_edges(
    from_id: &NodeId,
    edges: &[DirectedEdge<'_>],
    graph_index: &GraphIndex<'_>,
) -> String {
    edges
        .iter()
        .map(|edge| {
            format!(
                "{}[{}]",
                graph_index.relative_symbol(from_id, edge.peer),
                edge.kind.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_finding(finding: &FindingFact) -> String {
    let reason = finding.reason.map_or_else(String::new, |reason| {
        format!(" reason={}", EdgeReason::from(reason).as_str())
    });
    format!(
        "[{}] related={}{reason} anchor={}:{}",
        finding.rule, finding.related, finding.anchor.name, finding.anchor.line
    )
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        DeclarationKind, GraphParts, OrderingEdge, ReferenceEdge, ReflexionStatus, SourceLocation,
        SummaryEdge,
    };
    use super::*;
    use crate::code_lint::ast::{
        CallClusterRule, OrderingReason, PlacementAnchor, RelationKind, Visibility,
    };

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
                lines: Vec::new(),
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
                lines: Vec::new(),
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

    #[test]
    fn signature_contract_group_summaries_render_in_usage_direction() {
        let frame_owner = GroupNode {
            id: NodeId::new("owner:Frame"),
            label: "Frame".to_owned(),
            kind: GroupKind::Unowned,
            recursion_group: 1,
            level: 1,
            children: Vec::new(),
        };
        let len_owner = GroupNode {
            id: NodeId::new("owner:frame_len"),
            label: "frame_len".to_owned(),
            kind: GroupKind::Unowned,
            recursion_group: 0,
            level: 0,
            children: Vec::new(),
        };
        let graph = AbstractionGraph::new(GraphParts {
            title: "src/frame.rs".to_owned(),
            declarations: Vec::new(),
            groups: vec![frame_owner, len_owner],
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

        let rendered = render_graph(&graph, OutputFormat::AnnotatedStubs);

        assert!(rendered.contains("  // <- frame_len[sig x1]\n"));
        assert!(rendered.contains("  // -> Frame[sig x1]\n"));
    }

    #[test]
    fn module_depth_renders_all_modules_and_edges_up_to_twenty_modules_and_truncates_above() {
        let twenty = render_graph(&module_depth_fixture_graph(20), OutputFormat::CompactBi);
        let twenty_one = render_graph(&module_depth_fixture_graph(21), OutputFormat::CompactBi);

        assert!(twenty.contains("### Module [L0] unit_19\n"));
        assert!(twenty.contains("-> unit_1[xmod x1,inferred]\n"));
        assert!(twenty_one.contains("... (+19 more modules in Core)\n"));
    }

    fn module_depth_fixture_graph(module_count: usize) -> AbstractionGraph {
        let mut child_ids = Vec::with_capacity(module_count);
        let mut groups = Vec::with_capacity(module_count + 1);
        for index in 0..module_count {
            let id = NodeId::new(format!("mod:unit_{index}"));
            child_ids.push(id.clone());
            groups.push(GroupNode {
                id,
                label: format!("unit_{index}"),
                kind: GroupKind::Module {
                    path: format!("unit_{index}"),
                },
                recursion_group: 0,
                level: 0,
                children: Vec::new(),
            });
        }
        groups.insert(
            0,
            GroupNode {
                id: NodeId::new("comp:Core"),
                label: "Core".to_owned(),
                kind: GroupKind::Component {
                    declared_dependencies: Some(Vec::new()),
                },
                recursion_group: 0,
                level: 0,
                children: child_ids,
            },
        );
        AbstractionGraph::new(GraphParts {
            title: "core".to_owned(),
            declarations: Vec::new(),
            groups,
            ordering_edges: Vec::new(),
            reference_edges: Vec::new(),
            summary_edges: vec![SummaryEdge {
                higher: NodeId::new("mod:unit_0"),
                lower: NodeId::new("mod:unit_1"),
                reason: EdgeReason::CrossModule,
                count: 1,
                reflexion: Some(ReflexionStatus::Inferred),
            }],
        })
        .expect("valid module-depth fixture")
    }
}
