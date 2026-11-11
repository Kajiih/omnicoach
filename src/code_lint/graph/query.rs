//! Query evaluators for `omnicoach graph` (`find`, `slice`).

use super::builder::{ProjectDepth, build_project_graph};
use super::facts::{
    CollectedSourceFile, collect_project_facts_with_config, collect_project_source_files,
    resolve_effective_paths,
};
use super::index::{DirectedEdge, GraphIndex};
use super::model::{AbstractionGraph, DeclarationKind, DeclarationNode, NodeId};
use crate::code_lint::ast::Visibility;
use crate::code_lint::project::ArchitectureConfig;
use crate::config::Config;
use crate::diagnostic::Language;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

const TOP_GROUP_LABEL: &str = "top";
const TRUNCATED_BUDGET_LINE: &str = "  ... (truncated by --budget)\n";
const BYTES_PER_TOKEN: usize = 4;
const MAX_CALL_SITES_PER_CALLER: usize = 3;
const MAX_SIBLINGS_IN_SLICE: usize = 10;
const MAX_FALLBACK_MATCHES: usize = 10;
const MAX_DISAMBIGUATION_MATCHES: usize = 15;
const MAX_TRANSITIVE_CALLERS: usize = 15;
const MAX_SLICE_HOPS: usize = 4;
const MULTI_HOP_RESERVE_SHIFT: u32 = 2;

/// Options for `omnicoach graph find`.
#[derive(Debug, Clone)]
pub struct FindQueryOptions {
    /// Case-insensitive substring or path pattern to match against declaration names.
    pub pattern: String,
    /// File or directory paths to search (defaults to `"."` when empty).
    pub paths: Vec<PathBuf>,
    /// Maximum number of matches to display.
    pub limit: usize,
}

/// Options for `omnicoach graph slice`.
#[derive(Debug, Clone)]
pub struct SliceQueryOptions {
    /// One or more unqualified symbols (`"from_file"`), `Type::method`
    /// (`"SuppressionTracker::from_file"`), or module-qualified paths.
    pub symbols: Vec<String>,
    /// File or directory paths to search (defaults to `"."` when empty).
    pub paths: Vec<PathBuf>,
    /// Neighborhood expansion depth (`1` = direct callers/callees, `2..=4` = multi-hop).
    pub hops: usize,
    /// Approximate total token budget for the slice invocation (`1 token ~= 4 bytes`).
    pub budget: usize,
}

/// Runs `omnicoach graph slice` against `options`.
///
/// # Errors
///
/// Returns an error if target paths cannot be read or graph construction fails.
pub fn run_slice_query(options: &SliceQueryOptions, config: &Config) -> anyhow::Result<String> {
    let paths = resolve_effective_paths(&options.paths)?;
    let sources = collect_project_source_files(&paths, config)?;
    if sources.is_empty() {
        anyhow::bail!("No supported source files found in the specified paths");
    }
    slice_in_sources(
        &sources,
        &options.symbols,
        options.hops.max(1),
        options.budget.max(100),
        config.project.architecture.as_ref(),
    )
}

/// Runs `omnicoach graph find` against `options`.
///
/// # Errors
///
/// Returns an error if target paths cannot be read or graph construction fails.
pub fn run_find_query(options: &FindQueryOptions, config: &Config) -> anyhow::Result<String> {
    let paths = resolve_effective_paths(&options.paths)?;
    let sources = collect_project_source_files(&paths, config)?;
    if sources.is_empty() {
        anyhow::bail!("No supported source files found in the specified paths");
    }
    find_in_sources(
        &sources,
        &options.pattern,
        options.limit.max(1),
        config.project.architecture.as_ref(),
    )
}

const MIN_RESERVED_CHARS_PER_SYMBOL: usize = 1_600;

fn slice_in_sources(
    sources: &[CollectedSourceFile],
    symbols: &[String],
    hops: usize,
    budget: usize,
    architecture: Option<&ArchitectureConfig>,
) -> anyhow::Result<String> {
    let built_graphs = build_all_lang_graphs(sources, architecture)?;
    let indices: Vec<LanguageQueryIndex<'_>> = built_graphs
        .iter()
        .map(|built| LanguageQueryIndex::new(built.lang, &built.graph, sources))
        .collect();
    let effective_hops = hops.clamp(1, MAX_SLICE_HOPS);
    let mut remaining_chars = budget.saturating_mul(BYTES_PER_TOKEN);
    let mut outputs = Vec::new();
    for (index, symbol) in symbols.iter().enumerate() {
        let remaining_after = symbols.len().saturating_sub(index + 1);
        let symbol_char_budget = remaining_chars
            .saturating_sub(remaining_after.saturating_mul(MIN_RESERVED_CHARS_PER_SYMBOL))
            .max(MIN_RESERVED_CHARS_PER_SYMBOL.min(remaining_chars));
        if symbol_char_budget == 0 {
            outputs.push(TRUNCATED_BUDGET_LINE.to_owned());
            break;
        }
        let rendered = slice_single_symbol(&indices, symbol, effective_hops, symbol_char_budget);
        remaining_chars = remaining_chars.saturating_sub(rendered.len());
        outputs.push(rendered);
    }
    Ok(outputs.join("\n"))
}

fn find_in_sources(
    sources: &[CollectedSourceFile],
    pattern: &str,
    limit: usize,
    architecture: Option<&ArchitectureConfig>,
) -> anyhow::Result<String> {
    let built_graphs = build_all_lang_graphs(sources, architecture)?;
    let indices: Vec<LanguageQueryIndex<'_>> = built_graphs
        .iter()
        .map(|built| LanguageQueryIndex::new(built.lang, &built.graph, sources))
        .collect();
    Ok(find_in_indices(&indices, pattern, limit))
}

struct LanguageQueryIndex<'a> {
    lang: Language,
    graph_index: GraphIndex<'a>,
    sources_by_file: BTreeMap<&'a str, &'a str>,
}

impl<'a> LanguageQueryIndex<'a> {
    fn new(
        lang: Language,
        graph: &'a AbstractionGraph,
        sources: &'a [CollectedSourceFile],
    ) -> Self {
        let graph_index = GraphIndex::new(graph);
        let sources_by_file: BTreeMap<&'a str, &'a str> = sources
            .iter()
            .map(|file| (file.relative_path.as_str(), file.source.as_str()))
            .collect();
        Self {
            lang,
            graph_index,
            sources_by_file,
        }
    }
}

fn slice_single_symbol(
    indices: &[LanguageQueryIndex<'_>],
    symbol: &str,
    hops: usize,
    char_budget: usize,
) -> String {
    let trimmed = symbol.trim();
    let needle_lower = trimmed.to_ascii_lowercase();
    let mut qualified_exact: Vec<(&LanguageQueryIndex<'_>, &DeclarationNode)> = Vec::new();
    let mut name_exact: Vec<(&LanguageQueryIndex<'_>, &DeclarationNode)> = Vec::new();
    let mut fuzzy_candidates: Vec<(&LanguageQueryIndex<'_>, &DeclarationNode)> = Vec::new();
    for index in indices {
        for declaration in index.graph_index.graph().declarations() {
            let full_path = index.graph_index.qualified_symbol(&declaration.id);
            let short_member = declaration
                .name
                .rsplit("::")
                .next()
                .unwrap_or(declaration.name.as_str());
            if full_path == trimmed {
                qualified_exact.push((index, declaration));
            } else if declaration.name == trimmed {
                name_exact.push((index, declaration));
            } else if short_member == trimmed
                || matches_qualified_suffix(&full_path.to_ascii_lowercase(), &needle_lower)
            {
                fuzzy_candidates.push((index, declaration));
            }
        }
    }
    if let [(index, declaration)] = qualified_exact.as_slice() {
        return render_slice_for_decl(index, declaration, hops, char_budget);
    }
    if qualified_exact.is_empty()
        && let [(index, declaration)] = name_exact.as_slice()
    {
        return render_slice_for_decl(index, declaration, hops, char_budget);
    }
    if qualified_exact.is_empty()
        && name_exact.is_empty()
        && let [(index, declaration)] = fuzzy_candidates.as_slice()
    {
        return render_slice_for_decl(index, declaration, hops, char_budget);
    }
    let ambiguous = if !qualified_exact.is_empty() {
        qualified_exact
    } else if !name_exact.is_empty() {
        name_exact
    } else {
        fuzzy_candidates
    };
    if !ambiguous.is_empty() {
        let mut out = format!(
            "# Ambiguous symbol \"{trimmed}\" ({} matches) — pass a qualified `module::Symbol` or `Type::method`:\n",
            ambiguous.len()
        );
        for (index, declaration) in ambiguous.into_iter().take(MAX_DISAMBIGUATION_MATCHES) {
            out.push_str(&format_find_entry(index, declaration));
        }
        return out;
    }
    find_in_indices(indices, trimmed, MAX_FALLBACK_MATCHES)
}

fn find_in_indices(indices: &[LanguageQueryIndex<'_>], pattern: &str, limit: usize) -> String {
    let needle = pattern.trim().to_ascii_lowercase();
    let mut matches = Vec::new();
    for (graph_idx, index) in indices.iter().enumerate() {
        for (decl_idx, declaration) in index.graph_index.graph().declarations().iter().enumerate() {
            if let Some(rank) = score_declaration_match(index, declaration, &needle) {
                matches.push(ScoredMatch {
                    rank,
                    visibility: declaration.visibility,
                    neg_level: Reverse(declaration.level),
                    file: declaration.location.file.as_str(),
                    start_line: declaration.location.line,
                    graph_idx,
                    decl_idx,
                });
            }
        }
    }
    matches.sort();
    let total = matches.len();
    let shown = matches.into_iter().take(limit).collect::<Vec<_>>();
    let mut out = format!("# Graph Find: \"{pattern}\" ({total} matches)\n");
    if shown.is_empty() {
        out.push_str("  (no declarations matched)\n");
    } else {
        for item in shown {
            let index = &indices[item.graph_idx];
            let declaration = &index.graph_index.graph().declarations()[item.decl_idx];
            out.push_str(&format_find_entry(index, declaration));
        }
        out.push_str(
            "# Hint: run `omnicoach graph slice <symbol>` to inspect callers, call sites, and callees.\n",
        );
    }
    out
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct ScoredMatch<'a> {
    rank: MatchRank,
    visibility: Visibility,
    neg_level: Reverse<usize>,
    file: &'a str,
    start_line: usize,
    graph_idx: usize,
    decl_idx: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchRank {
    ExactSymbol,
    PrefixSymbol,
    QualifiedSubstring,
    SignatureSubstring,
}

fn score_declaration_match(
    index: &LanguageQueryIndex<'_>,
    declaration: &DeclarationNode,
    needle: &str,
) -> Option<MatchRank> {
    let name_lower = declaration.name.to_ascii_lowercase();
    let short_lower = name_lower.rsplit("::").next().unwrap_or(&name_lower);
    let qualified_lower = index
        .graph_index
        .qualified_symbol(&declaration.id)
        .to_ascii_lowercase();
    if short_lower == needle || name_lower == needle || qualified_lower == needle {
        Some(MatchRank::ExactSymbol)
    } else if short_lower.starts_with(needle) || name_lower.starts_with(needle) {
        Some(MatchRank::PrefixSymbol)
    } else if qualified_lower.contains(needle) {
        Some(MatchRank::QualifiedSubstring)
    } else if declaration
        .signature
        .as_ref()
        .is_some_and(|sig| sig.to_ascii_lowercase().contains(needle))
    {
        Some(MatchRank::SignatureSubstring)
    } else {
        None
    }
}

fn format_find_entry(index: &LanguageQueryIndex<'_>, declaration: &DeclarationNode) -> String {
    let qualified = index.graph_index.qualified_symbol(&declaration.id);
    let visibility_label = declaration.visibility.as_str();
    let kind = declaration.kind.as_str();
    let owner = index
        .graph_index
        .owner_label_of(&declaration.id)
        .unwrap_or(TOP_GROUP_LABEL);
    let in_count = index
        .graph_index
        .declaration_in_edges(&declaration.id)
        .len();
    let out_count = index
        .graph_index
        .declaration_out_edges(&declaration.id)
        .len();
    let mut out = format!(
        "- [L{} {visibility_label} {kind}] {qualified} ({}:{}..{}, group={owner})\n",
        declaration.level,
        declaration.location.file,
        declaration.location.line,
        declaration.location.end_line
    );
    if let Some(sig) = &declaration.signature {
        let _ = writeln!(out, "  {sig}");
    }
    let _ = writeln!(
        out,
        "  <- {in_count} callers | -> {out_count} callees/types"
    );
    out
}

fn render_slice_for_decl(
    index: &LanguageQueryIndex<'_>,
    declaration: &DeclarationNode,
    hops: usize,
    char_budget: usize,
) -> String {
    let qualified = index.graph_index.qualified_symbol(&declaration.id);
    let comp = index
        .graph_index
        .component_of(&declaration.id)
        .unwrap_or("unassigned");
    let mod_path = index.graph_index.module_of(&declaration.id).unwrap_or("");
    let owner = index
        .graph_index
        .owner_label_of(&declaration.id)
        .unwrap_or(TOP_GROUP_LABEL);
    let visibility_label = declaration.visibility.as_str();
    let kind = declaration.kind.as_str();

    let mut out = format!("# Graph Slice: {qualified}\n");
    let _ = writeln!(
        out,
        "Component: {comp} | Module: {mod_path} ({}) | Cluster: {owner}",
        declaration.location.file
    );
    let _ = writeln!(
        out,
        "Target: [L{} {visibility_label} {kind}] {qualified} ({}:{}..{})",
        declaration.level,
        declaration.location.file,
        declaration.location.line,
        declaration.location.end_line
    );
    if let Some(sig) = &declaration.signature {
        let _ = writeln!(out, "  {sig}");
    }
    for finding in &declaration.findings {
        let preposition = finding.rule.anchor_preposition();
        let _ = writeln!(
            out,
            "  ! [{}] related to `{}` -> move {preposition} `{}`:{}",
            finding.rule, finding.related, finding.anchor.name, finding.anchor.line
        );
    }

    render_slice_callers(&mut out, index, declaration, hops, char_budget);
    render_slice_callees(&mut out, index, declaration, hops, char_budget);
    render_slice_siblings(&mut out, index, declaration, char_budget);
    render_completeness_footer(&mut out, index.lang, declaration);
    out
}

fn render_slice_callers(
    out: &mut String,
    index: &LanguageQueryIndex<'_>,
    declaration: &DeclarationNode,
    hops: usize,
    char_budget: usize,
) {
    let callers = index.graph_index.declaration_in_edges(&declaration.id);
    let _ = writeln!(out, "\n<- used_by ({}):", callers.len());
    if callers.is_empty() {
        out.push_str("  (none in static graph)\n");
        return;
    }
    let target_mod = index.graph_index.module_of(&declaration.id);
    let mut ordered: Vec<&DirectedEdge<'_>> = callers.iter().collect();
    ordered.sort_by_key(|edge| {
        let same_mod = index.graph_index.module_of(edge.peer) == target_mod;
        (!same_mod, index.graph_index.qualified_symbol(edge.peer))
    });
    let has_callees = !index
        .graph_index
        .declaration_out_edges(&declaration.id)
        .is_empty();
    let caller_budget = if has_callees {
        char_budget.saturating_sub(char_budget >> MULTI_HOP_RESERVE_SHIFT)
    } else {
        char_budget
    };
    let hop1_budget = if hops >= 2 {
        caller_budget.saturating_sub(caller_budget >> MULTI_HOP_RESERVE_SHIFT)
    } else {
        caller_budget
    };
    for edge in &ordered {
        if out.len() >= hop1_budget {
            out.push_str(TRUNCATED_BUDGET_LINE);
            break;
        }
        let Some(caller) = index.graph_index.declaration(edge.peer) else {
            continue;
        };
        let caller_qual = index.graph_index.qualified_symbol(edge.peer);
        let visibility_label = caller.visibility.as_str();
        let kind = caller.kind.as_str();
        let reason = edge.kind.as_str();
        let _ = writeln!(
            out,
            "  - [L{} {visibility_label} {kind}] {caller_qual} [{reason}] ({}:{}..{})",
            caller.level, caller.location.file, caller.location.line, caller.location.end_line
        );
        for &line in index
            .graph_index
            .reference_lines(edge.peer, &declaration.id)
            .iter()
            .take(MAX_CALL_SITES_PER_CALLER)
        {
            let snippet = source_line_snippet(&index.sources_by_file, &caller.location.file, line);
            let _ = writeln!(out, "      @ {}:{line}: {snippet}", caller.location.file);
        }
    }
    if hops >= 2 {
        render_multi_hop_callers(out, index, &declaration.id, &ordered, hops, caller_budget);
    }
}

fn render_multi_hop_callers<'a>(
    out: &mut String,
    index: &LanguageQueryIndex<'a>,
    root_id: &'a NodeId,
    hop1_ordered: &[&DirectedEdge<'a>],
    hops: usize,
    char_budget: usize,
) {
    let mut visited: BTreeSet<&'a NodeId> = BTreeSet::from([root_id]);
    let mut current_frontier: Vec<&'a NodeId> = Vec::new();
    for edge in hop1_ordered {
        if visited.insert(edge.peer) {
            current_frontier.push(edge.peer);
        }
    }
    for hop in 2..=hops {
        if current_frontier.is_empty() || out.len() >= char_budget {
            break;
        }
        let mut next_entries: Vec<(&'a NodeId, &'a NodeId, &'static str)> = Vec::new();
        let mut next_frontier: Vec<&'a NodeId> = Vec::new();
        for &via_id in &current_frontier {
            for edge in index.graph_index.declaration_in_edges(via_id) {
                if visited.insert(edge.peer) {
                    next_entries.push((edge.peer, via_id, edge.kind.as_str()));
                    next_frontier.push(edge.peer);
                }
            }
        }
        if next_entries.is_empty() {
            break;
        }
        let _ = writeln!(out, "\n<- used_by (hop {hop}, {}):", next_entries.len());
        for (caller_id, via_id, reason) in next_entries.into_iter().take(MAX_TRANSITIVE_CALLERS) {
            if out.len() >= char_budget {
                out.push_str(TRUNCATED_BUDGET_LINE);
                break;
            }
            let Some(caller) = index.graph_index.declaration(caller_id) else {
                continue;
            };
            let via_name = index
                .graph_index
                .declaration(via_id)
                .map_or(via_id.as_str(), |node| node.name.as_str());
            let caller_qual = index.graph_index.qualified_symbol(caller_id);
            let visibility_label = caller.visibility.as_str();
            let kind = caller.kind.as_str();
            let _ = writeln!(
                out,
                "  - [L{} {visibility_label} {kind}] {caller_qual} [{reason} via {via_name}] ({}:{}..{})",
                caller.level, caller.location.file, caller.location.line, caller.location.end_line
            );
        }
        current_frontier = next_frontier;
    }
}

fn render_slice_callees(
    out: &mut String,
    index: &LanguageQueryIndex<'_>,
    declaration: &DeclarationNode,
    hops: usize,
    char_budget: usize,
) {
    let callees = index.graph_index.declaration_out_edges(&declaration.id);
    let _ = writeln!(out, "\n-> uses ({}):", callees.len());
    if callees.is_empty() {
        out.push_str("  (leaf declaration)\n");
        return;
    }
    for edge in callees {
        if out.len() >= char_budget {
            out.push_str(TRUNCATED_BUDGET_LINE);
            break;
        }
        let Some(callee) = index.graph_index.declaration(edge.peer) else {
            continue;
        };
        let callee_qual = index.graph_index.qualified_symbol(edge.peer);
        let visibility_label = callee.visibility.as_str();
        let kind = callee.kind.as_str();
        let reason = edge.kind.as_str();
        let _ = writeln!(
            out,
            "  - [L{} {visibility_label} {kind}] {callee_qual} [{reason}] ({}:{}..{})",
            callee.level, callee.location.file, callee.location.line, callee.location.end_line
        );
        if let Some(sig) = &callee.signature {
            let _ = writeln!(out, "      {sig}");
        }
    }
    if hops >= 2 {
        render_multi_hop_callees(out, index, &declaration.id, callees, hops, char_budget);
    }
}

fn render_multi_hop_callees<'a>(
    out: &mut String,
    index: &LanguageQueryIndex<'a>,
    root_id: &'a NodeId,
    hop1_callees: &[DirectedEdge<'a>],
    hops: usize,
    char_budget: usize,
) {
    let mut visited: BTreeSet<&'a NodeId> = BTreeSet::from([root_id]);
    let mut current_frontier: Vec<&'a NodeId> = Vec::new();
    for edge in hop1_callees {
        if visited.insert(edge.peer) {
            current_frontier.push(edge.peer);
        }
    }
    for hop in 2..=hops {
        if current_frontier.is_empty() || out.len() >= char_budget {
            break;
        }
        let mut next_entries: Vec<(&'a NodeId, &'a NodeId, &'static str)> = Vec::new();
        let mut next_frontier: Vec<&'a NodeId> = Vec::new();
        for &via_id in &current_frontier {
            for edge in index.graph_index.declaration_out_edges(via_id) {
                if visited.insert(edge.peer) {
                    next_entries.push((edge.peer, via_id, edge.kind.as_str()));
                    next_frontier.push(edge.peer);
                }
            }
        }
        if next_entries.is_empty() {
            break;
        }
        let _ = writeln!(out, "\n-> uses (hop {hop}, {}):", next_entries.len());
        for (callee_id, via_id, reason) in next_entries {
            if out.len() >= char_budget {
                out.push_str(TRUNCATED_BUDGET_LINE);
                break;
            }
            let Some(callee) = index.graph_index.declaration(callee_id) else {
                continue;
            };
            let via_name = index
                .graph_index
                .declaration(via_id)
                .map_or(via_id.as_str(), |node| node.name.as_str());
            let callee_qual = index.graph_index.qualified_symbol(callee_id);
            let visibility_label = callee.visibility.as_str();
            let kind = callee.kind.as_str();
            let _ = writeln!(
                out,
                "  - [L{} {visibility_label} {kind}] {callee_qual} [{reason} via {via_name}] ({}:{}..{})",
                callee.level, callee.location.file, callee.location.line, callee.location.end_line
            );
            if let Some(sig) = &callee.signature {
                let _ = writeln!(out, "      {sig}");
            }
        }
        current_frontier = next_frontier;
    }
}

fn render_slice_siblings(
    out: &mut String,
    index: &LanguageQueryIndex<'_>,
    declaration: &DeclarationNode,
    char_budget: usize,
) {
    if out.len() >= char_budget {
        return;
    }
    let Some(parent_id) = index.graph_index.graph().parent_of(&declaration.id) else {
        return;
    };
    let Some(group) = index.graph_index.group(parent_id) else {
        return;
    };
    let siblings: Vec<&DeclarationNode> = group
        .children
        .iter()
        .filter(|&child_id| child_id != &declaration.id)
        .filter_map(|child_id| index.graph_index.declaration(child_id))
        .collect();
    if siblings.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "\nCluster siblings in `{}` ({}):",
        group.label,
        siblings.len()
    );
    for sibling in siblings.into_iter().take(MAX_SIBLINGS_IN_SLICE) {
        let visibility_label = sibling.visibility.as_str();
        let kind = sibling.kind.as_str();
        let _ = writeln!(
            out,
            "  - [L{} {visibility_label} {kind}] {} :{}..{}",
            sibling.level, sibling.name, sibling.location.line, sibling.location.end_line
        );
    }
}

fn render_completeness_footer(out: &mut String, lang: Language, decl: &DeclarationNode) {
    let short_name = decl.name.rsplit("::").next().unwrap_or(decl.name.as_str());
    let takes_self = decl.kind == DeclarationKind::Method
        && decl
            .signature
            .as_deref()
            .is_some_and(|signature| signature_has_receiver_param(signature, lang));
    match (lang, takes_self) {
        (Language::Rust, true) => {
            let _ = writeln!(
                out,
                "\n# Completeness: `<-` lists static path references (T::{short_name}, Self::{short_name}, self.{short_name}). Receiver-typed calls (x.{short_name}()) are not resolved statically; run: rg '\\.{short_name}\\s*\\(' src/"
            );
        }
        (Language::Rust, false) => {
            let _ = writeln!(
                out,
                "\n# Completeness: `<-` lists static direct and qualified path references across src/ (excluding #[cfg(test)]). For macro or test references, run: rg '\\b{short_name}\\b' src/ tests/"
            );
        }
        (Language::Python, true) => {
            let _ = writeln!(
                out,
                "\n# Completeness: `<-` lists static class/self attribute references (Cls.{short_name}, self.{short_name}). Receiver-typed calls (x.{short_name}()) are not resolved statically; run: rg '\\.{short_name}\\s*\\('"
            );
        }
        (Language::Python, false) => {
            let _ = writeln!(
                out,
                "\n# Completeness: `<-` lists static direct and imported symbol references across non-test Python modules. For dynamic or test references, run: rg '\\b{short_name}\\b'"
            );
        }
    }
}

struct BuiltLanguageGraph {
    lang: Language,
    graph: AbstractionGraph,
}

fn build_all_lang_graphs(
    sources: &[CollectedSourceFile],
    architecture: Option<&ArchitectureConfig>,
) -> anyhow::Result<Vec<BuiltLanguageGraph>> {
    let mut built_graphs = Vec::new();
    for lang in [Language::Rust, Language::Python] {
        if let Some(built) = build_lang_graph(sources, lang, architecture)? {
            built_graphs.push(built);
        }
    }
    Ok(built_graphs)
}

fn build_lang_graph(
    sources: &[CollectedSourceFile],
    lang: Language,
    architecture: Option<&ArchitectureConfig>,
) -> anyhow::Result<Option<BuiltLanguageGraph>> {
    let lang_files: Vec<(&str, &str)> = sources
        .iter()
        .filter(|file| file.lang == lang)
        .map(|file| (file.relative_path.as_str(), file.source.as_str()))
        .collect();
    if lang_files.is_empty() {
        return Ok(None);
    }
    let facts = collect_project_facts_with_config("project", lang, &lang_files, architecture);
    let graph = build_project_graph(&facts, ProjectDepth::Full)?;
    Ok(Some(BuiltLanguageGraph { lang, graph }))
}

const SELF_RECEIVER_NAME: &str = "self";
const CLS_RECEIVER_NAME: &str = "cls";

fn matches_qualified_suffix(qualified_lower: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    let norm_qualified = qualified_lower.replace('.', "::");
    let norm_needle = needle_lower.replace('.', "::");
    let Some(prefix) = norm_qualified.strip_suffix(&norm_needle) else {
        return false;
    };
    prefix.is_empty() || prefix.ends_with("::")
}

fn signature_has_receiver_param(signature: &str, lang: Language) -> bool {
    let Some(open_paren) = signature.find('(') else {
        return false;
    };
    let rest = &signature[open_paren + 1..];
    let first_param = rest.split([',', ')']).next().map_or("", str::trim);
    if first_param.is_empty() {
        return false;
    }
    match lang {
        Language::Rust => {
            let without_colon = first_param.split(':').next().unwrap_or(first_param);
            without_colon
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .any(|token| token == SELF_RECEIVER_NAME)
        }
        Language::Python => {
            let param_name = first_param
                .split([':', '='])
                .next()
                .unwrap_or(first_param)
                .trim();
            matches!(param_name, SELF_RECEIVER_NAME | CLS_RECEIVER_NAME)
        }
    }
}

fn source_line_snippet<'a>(
    sources_by_file: &BTreeMap<&'a str, &'a str>,
    file: &str,
    line: usize,
) -> &'a str {
    let Some(&source) = sources_by_file.get(file) else {
        return "";
    };
    source
        .lines()
        .nth(line.saturating_sub(1))
        .map_or("", str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rust_sources() -> Vec<CollectedSourceFile> {
        vec![
            CollectedSourceFile {
                relative_path: "src/architecture.rs".to_owned(),
                lang: Language::Rust,
                source: indoc::indoc! {r"
                    define_architecture! {
                        Core => [],
                        Runner => [Core],
                    }
                "}
                .to_owned(),
            },
            CollectedSourceFile {
                relative_path: "src/core.rs".to_owned(),
                lang: Language::Rust,
                source: indoc::indoc! {r"
                    architecture_component!(Core);

                    pub struct Tracker {
                        pub count: usize,
                    }

                    impl Tracker {
                        pub fn from_source(text: &str) -> Self {
                            Self { count: count_lines(text) }
                        }

                        pub fn total(&self) -> usize {
                            self.count
                        }
                    }

                    fn count_lines(text: &str) -> usize {
                        text.lines().count()
                    }
                "}
                .to_owned(),
            },
            CollectedSourceFile {
                relative_path: "src/runner.rs".to_owned(),
                lang: Language::Rust,
                source: indoc::indoc! {r"
                    architecture_component!(Runner);

                    use crate::core::Tracker;

                    pub fn execute(input: &str) -> usize {
                        let tracker = Tracker::from_source(input);
                        summarize(&tracker)
                    }

                    fn summarize(tracker: &Tracker) -> usize {
                        tracker.total()
                    }
                "}
                .to_owned(),
            },
        ]
    }

    #[test]
    fn find_symbols_ranks_exact_before_prefix_and_substring() {
        let sources = sample_rust_sources();
        let out = find_in_sources(&sources, "from_source", 10, None).unwrap();
        assert!(
            out.contains("core::Tracker::from_source"),
            "expected Tracker::from_source in find output:\n{out}"
        );
        assert!(
            out.contains("pub fn from_source(text: &str) -> Self"),
            "expected signature in find output:\n{out}"
        );
        assert!(
            out.contains("<- 1 callers"),
            "expected caller count in find output:\n{out}"
        );
    }

    #[test]
    fn slice_symbol_renders_target_callers_with_call_site_lines_callees_and_completeness_footer() {
        let sources = sample_rust_sources();
        let out =
            slice_in_sources(&sources, &["Tracker::from_source".to_owned()], 1, 800, None).unwrap();
        assert!(
            out.contains("src/runner.rs:6:"),
            "expected cross-module caller with 1-based call-site line in slice:\n{out}"
        );
        assert!(
            out.contains("fn count_lines(text: &str) -> usize"),
            "expected callee stub in slice:\n{out}"
        );
        assert!(
            out.contains("# Completeness:"),
            "expected completeness footer in slice:\n{out}"
        );
    }

    #[test]
    fn slice_supports_multiple_symbols_and_two_hop_neighborhood() {
        let sources = sample_rust_sources();
        let multi = slice_in_sources(
            &sources,
            &["Tracker::from_source".to_owned(), "execute".to_owned()],
            2,
            800,
            None,
        )
        .unwrap();
        assert!(
            multi.contains("# Graph Slice: core::Tracker::from_source"),
            "expected first symbol slice:\n{multi}"
        );
        assert!(
            multi.contains("# Graph Slice: runner::execute"),
            "expected second symbol slice:\n{multi}"
        );
        assert!(
            multi.contains("-> uses (hop 2,"),
            "expected 2-hop callees in slice with hops=2:\n{multi}"
        );
        assert!(
            multi.contains("core::count_lines"),
            "expected 2-hop callee count_lines via from_source:\n{multi}"
        );
    }

    #[rstest::rstest]
    #[case::rust_ref_self("pub fn total(&self) -> usize", Language::Rust, true)]
    #[case::rust_mut_self("pub fn reset(&mut self)", Language::Rust, true)]
    #[case::rust_assoc_fn_with_self_substring(
        "pub fn from_self_config(self_path: &str) -> Self",
        Language::Rust,
        false
    )]
    #[case::python_self("def load(self, key: str) -> str", Language::Python, true)]
    #[case::python_cls("def create(cls, raw: str) -> Self", Language::Python, true)]
    #[case::python_staticmethod_with_cls_substring(
        "def parse(cls_name: str) -> None",
        Language::Python,
        false
    )]
    fn signature_receiver_detection_checks_first_parameter_only(
        #[case] signature: &str,
        #[case] language: Language,
        #[case] expected: bool,
    ) {
        assert_eq!(signature_has_receiver_param(signature, language), expected);
    }

    #[test]
    fn matches_qualified_suffix_requires_module_or_type_separator_boundary() {
        assert!(matches_qualified_suffix(
            "core::tracker::new",
            "tracker::new"
        ));
        assert!(matches_qualified_suffix("pkg.service.run", "service.run"));
        assert!(!matches_qualified_suffix(
            "core::subtracker::new",
            "tracker::new"
        ));
        assert!(matches_qualified_suffix(
            "pkg.service::tracker::run",
            "pkg.service.tracker.run"
        ));
    }
}
