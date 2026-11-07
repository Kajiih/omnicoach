//! Query evaluators for `omnicoach graph` (`find`, `slice`).

use super::builder::{ProjectDepth, build_project_graph};
use super::facts::{
    CollectedSourceFile, ProjectFacts, collect_project_facts_from_sources,
    collect_project_source_files, resolve_effective_paths,
};
use super::model::{AbstractionGraph, DeclarationKind, DeclarationNode, GroupNode, NodeId};
use super::render::{
    DeclAncestry, format_short_decl_kind, format_short_visibility, resolve_directed_decl_edges,
};
use crate::code_lint::ast::Visibility;
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
    find_in_sources(&sources, &options.pattern, options.limit.max(1))
}

const MIN_RESERVED_CHARS_PER_SYMBOL: usize = 1_600;

fn slice_in_sources(
    sources: &[CollectedSourceFile],
    symbols: &[String],
    hops: usize,
    budget: usize,
) -> anyhow::Result<String> {
    let built_graphs = build_all_lang_graphs(sources)?;
    let indices: Vec<IndexedProjectGraph<'_>> = built_graphs
        .iter()
        .map(|built| {
            IndexedProjectGraph::from_graph(built.lang, &built.graph, &built.facts, sources)
        })
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
) -> anyhow::Result<String> {
    let built_graphs = build_all_lang_graphs(sources)?;
    let indices: Vec<IndexedProjectGraph<'_>> = built_graphs
        .iter()
        .map(|built| {
            IndexedProjectGraph::from_graph(built.lang, &built.graph, &built.facts, sources)
        })
        .collect();
    Ok(find_in_indices(&indices, pattern, limit))
}

struct IndexedProjectGraph<'a> {
    lang: Language,
    graph: &'a AbstractionGraph,
    decl_by_id: BTreeMap<&'a NodeId, &'a DeclarationNode>,
    group_by_id: BTreeMap<&'a NodeId, &'a GroupNode>,
    module_by_decl: BTreeMap<&'a NodeId, &'a str>,
    component_by_decl: BTreeMap<&'a NodeId, &'a str>,
    owner_label_by_decl: BTreeMap<&'a NodeId, String>,
    out_edges: BTreeMap<&'a NodeId, Vec<(&'a NodeId, String)>>,
    in_edges: BTreeMap<&'a NodeId, Vec<(&'a NodeId, String)>>,
    call_sites: BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>>,
    sources_by_file: BTreeMap<&'a str, &'a str>,
}

impl<'a> IndexedProjectGraph<'a> {
    fn from_graph(
        lang: Language,
        graph: &'a AbstractionGraph,
        facts: &'a ProjectFacts,
        sources: &'a [CollectedSourceFile],
    ) -> Self {
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
        let sources_by_file: BTreeMap<&'a str, &'a str> = sources
            .iter()
            .map(|file| (file.relative_path.as_str(), file.source.as_str()))
            .collect();

        let ancestry = DeclAncestry::from_graph(graph, &group_by_id);
        let mut out_edges: BTreeMap<&'a NodeId, Vec<(&'a NodeId, String)>> = BTreeMap::new();
        let mut in_edges: BTreeMap<&'a NodeId, Vec<(&'a NodeId, String)>> = BTreeMap::new();
        for edge in resolve_directed_decl_edges(graph, &ancestry) {
            out_edges
                .entry(edge.source_id)
                .or_default()
                .push((edge.target_id, edge.tag.to_owned()));
            in_edges
                .entry(edge.target_id)
                .or_default()
                .push((edge.source_id, edge.tag.to_owned()));
        }
        let call_sites = Self::collect_call_site_lines(facts, &ancestry.decl_by_mod_and_name);

        Self {
            lang,
            graph,
            decl_by_id,
            group_by_id,
            module_by_decl: ancestry.decl_module_path,
            component_by_decl: ancestry.decl_component,
            owner_label_by_decl: ancestry.decl_owner_label,
            out_edges,
            in_edges,
            call_sites,
            sources_by_file,
        }
    }

    fn collect_call_site_lines(
        facts: &'a ProjectFacts,
        decl_by_mod_and_name: &BTreeMap<(&'a str, &'a str), &'a NodeId>,
    ) -> BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>> {
        let mut sites: BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>> = BTreeMap::new();
        for xmod in &facts.cross_module_refs {
            let (Some(src_sym), Some(dst_sym)) =
                (xmod.source_symbol.as_deref(), xmod.target_symbol.as_deref())
            else {
                continue;
            };
            Self::record_ref_call_site(
                &mut sites,
                decl_by_mod_and_name,
                &xmod.source_module,
                src_sym,
                &xmod.target_module,
                dst_sym,
                xmod.line,
            );
        }
        for module in &facts.modules {
            let mod_path = module.module_path.as_str();
            for iref in &module.intra_file_refs {
                Self::record_ref_call_site(
                    &mut sites,
                    decl_by_mod_and_name,
                    mod_path,
                    &iref.source_symbol,
                    mod_path,
                    &iref.target_symbol,
                    iref.line,
                );
            }
        }
        for list in sites.values_mut() {
            list.sort_unstable();
            list.dedup();
        }
        sites
    }

    fn record_ref_call_site(
        sites: &mut BTreeMap<(&'a NodeId, &'a NodeId), Vec<usize>>,
        decl_by_mod_and_name: &BTreeMap<(&'a str, &'a str), &'a NodeId>,
        src_mod: &str,
        src_sym: &str,
        dst_mod: &str,
        dst_sym: &str,
        line: usize,
    ) {
        let Some(&src_id) = decl_by_mod_and_name.get(&(src_mod, src_sym)).or_else(|| {
            let owner = src_sym.split("::").next()?;
            decl_by_mod_and_name.get(&(src_mod, owner))
        }) else {
            return;
        };
        if let Some(&dst_id) = decl_by_mod_and_name.get(&(dst_mod, dst_sym)) {
            sites.entry((src_id, dst_id)).or_default().push(line);
        }
        if let Some((owner, _)) = dst_sym.split_once("::")
            && let Some(&owner_id) = decl_by_mod_and_name.get(&(dst_mod, owner))
        {
            sites.entry((src_id, owner_id)).or_default().push(line);
        }
    }

    fn qualified_symbol(&self, decl_id: &NodeId) -> String {
        let Some(decl) = self.decl_by_id.get(decl_id) else {
            return decl_id.as_str().to_owned();
        };
        match self.module_by_decl.get(decl_id) {
            Some(&mod_path) if !mod_path.is_empty() => format!("{mod_path}::{}", decl.name),
            _ => decl.name.clone(),
        }
    }
}

fn slice_single_symbol(
    indices: &[IndexedProjectGraph<'_>],
    symbol: &str,
    hops: usize,
    char_budget: usize,
) -> String {
    let trimmed = symbol.trim();
    let needle_lower = trimmed.to_ascii_lowercase();
    let mut qualified_exact: Vec<(&IndexedProjectGraph<'_>, &DeclarationNode)> = Vec::new();
    let mut name_exact: Vec<(&IndexedProjectGraph<'_>, &DeclarationNode)> = Vec::new();
    let mut fuzzy_candidates: Vec<(&IndexedProjectGraph<'_>, &DeclarationNode)> = Vec::new();
    for index in indices {
        for decl in index.graph.declarations() {
            let full_path = index.qualified_symbol(&decl.id);
            let short_member = decl.name.rsplit("::").next().unwrap_or(decl.name.as_str());
            if full_path == trimmed {
                qualified_exact.push((index, decl));
            } else if decl.name == trimmed {
                name_exact.push((index, decl));
            } else if short_member == trimmed
                || matches_qualified_suffix(&full_path.to_ascii_lowercase(), &needle_lower)
            {
                fuzzy_candidates.push((index, decl));
            }
        }
    }
    if let [(index, decl)] = qualified_exact.as_slice() {
        return render_slice_for_decl(index, decl, hops, char_budget);
    }
    if qualified_exact.is_empty()
        && let [(index, decl)] = name_exact.as_slice()
    {
        return render_slice_for_decl(index, decl, hops, char_budget);
    }
    if qualified_exact.is_empty()
        && name_exact.is_empty()
        && let [(index, decl)] = fuzzy_candidates.as_slice()
    {
        return render_slice_for_decl(index, decl, hops, char_budget);
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
        for (index, decl) in ambiguous.into_iter().take(MAX_DISAMBIGUATION_MATCHES) {
            out.push_str(&format_find_entry(index, decl));
        }
        return out;
    }
    find_in_indices(indices, trimmed, MAX_FALLBACK_MATCHES)
}

fn find_in_indices(indices: &[IndexedProjectGraph<'_>], pattern: &str, limit: usize) -> String {
    let needle = pattern.trim().to_ascii_lowercase();
    let mut matches = Vec::new();
    for (graph_idx, index) in indices.iter().enumerate() {
        for (decl_idx, decl) in index.graph.declarations().iter().enumerate() {
            if let Some(rank) = score_declaration_match(index, decl, &needle) {
                matches.push(ScoredMatch {
                    rank,
                    visibility: decl.visibility,
                    neg_level: Reverse(decl.level),
                    file: decl.location.file.as_str(),
                    start_line: decl.location.line,
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
            let decl = &index.graph.declarations()[item.decl_idx];
            out.push_str(&format_find_entry(index, decl));
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
    index: &IndexedProjectGraph<'_>,
    decl: &DeclarationNode,
    needle: &str,
) -> Option<MatchRank> {
    let name_lower = decl.name.to_ascii_lowercase();
    let short_lower = name_lower.rsplit("::").next().unwrap_or(&name_lower);
    let qualified_lower = index.qualified_symbol(&decl.id).to_ascii_lowercase();
    if short_lower == needle || name_lower == needle || qualified_lower == needle {
        Some(MatchRank::ExactSymbol)
    } else if short_lower.starts_with(needle) || name_lower.starts_with(needle) {
        Some(MatchRank::PrefixSymbol)
    } else if qualified_lower.contains(needle) {
        Some(MatchRank::QualifiedSubstring)
    } else if decl
        .signature
        .as_ref()
        .is_some_and(|sig| sig.to_ascii_lowercase().contains(needle))
    {
        Some(MatchRank::SignatureSubstring)
    } else {
        None
    }
}

fn format_find_entry(index: &IndexedProjectGraph<'_>, decl: &DeclarationNode) -> String {
    let qualified = index.qualified_symbol(&decl.id);
    let visibility_label = format_short_visibility(decl.visibility);
    let kind = format_short_decl_kind(decl.kind);
    let owner = index
        .owner_label_by_decl
        .get(&decl.id)
        .map_or(TOP_GROUP_LABEL, String::as_str);
    let in_count = index.in_edges.get(&decl.id).map_or(0, Vec::len);
    let out_count = index.out_edges.get(&decl.id).map_or(0, Vec::len);
    let mut out = format!(
        "- [L{} {visibility_label} {kind}] {qualified} ({}:{}..{}, group={owner})\n",
        decl.level, decl.location.file, decl.location.line, decl.location.end_line
    );
    if let Some(sig) = &decl.signature {
        let _ = writeln!(out, "  {sig}");
    }
    let _ = writeln!(
        out,
        "  <- {in_count} callers | -> {out_count} callees/types"
    );
    out
}

fn render_slice_for_decl(
    index: &IndexedProjectGraph<'_>,
    decl: &DeclarationNode,
    hops: usize,
    char_budget: usize,
) -> String {
    let qualified = index.qualified_symbol(&decl.id);
    let comp = index
        .component_by_decl
        .get(&decl.id)
        .copied()
        .unwrap_or("unassigned");
    let mod_path = index.module_by_decl.get(&decl.id).copied().unwrap_or("");
    let owner = index
        .owner_label_by_decl
        .get(&decl.id)
        .map_or(TOP_GROUP_LABEL, String::as_str);
    let visibility_label = format_short_visibility(decl.visibility);
    let kind = format_short_decl_kind(decl.kind);

    let mut out = format!("# Graph Slice: {qualified}\n");
    let _ = writeln!(
        out,
        "Component: {comp} | Module: {mod_path} ({}) | Cluster: {owner}",
        decl.location.file
    );
    let _ = writeln!(
        out,
        "Target: [L{} {visibility_label} {kind}] {qualified} ({}:{}..{})",
        decl.level, decl.location.file, decl.location.line, decl.location.end_line
    );
    if let Some(sig) = &decl.signature {
        let _ = writeln!(out, "  {sig}");
    }
    for finding in &decl.findings {
        let preposition = finding.rule.anchor_preposition();
        let _ = writeln!(
            out,
            "  ! [{}] related to `{}` -> move {preposition} `{}`:{}",
            finding.rule, finding.related, finding.anchor.name, finding.anchor.line
        );
    }

    render_slice_callers(&mut out, index, decl, hops, char_budget);
    render_slice_callees(&mut out, index, decl, hops, char_budget);
    render_slice_siblings(&mut out, index, decl, char_budget);
    render_completeness_footer(&mut out, index.lang, decl);
    out
}

fn render_slice_callers(
    out: &mut String,
    index: &IndexedProjectGraph<'_>,
    decl: &DeclarationNode,
    hops: usize,
    char_budget: usize,
) {
    let callers = index.in_edges.get(&decl.id).map_or(&[][..], Vec::as_slice);
    let _ = writeln!(out, "\n<- used_by ({}):", callers.len());
    if callers.is_empty() {
        out.push_str("  (none in static graph)\n");
        return;
    }
    let target_mod = index.module_by_decl.get(&decl.id).copied();
    let mut ordered: Vec<&(&NodeId, String)> = callers.iter().collect();
    ordered.sort_by_key(|(caller_id, _)| {
        let same_mod = index.module_by_decl.get(caller_id).copied() == target_mod;
        (!same_mod, index.qualified_symbol(caller_id))
    });
    let has_callees = index
        .out_edges
        .get(&decl.id)
        .is_some_and(|edges| !edges.is_empty());
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
    for (caller_id, reason) in &ordered {
        if out.len() >= hop1_budget {
            out.push_str(TRUNCATED_BUDGET_LINE);
            break;
        }
        let Some(caller) = index.decl_by_id.get(caller_id) else {
            continue;
        };
        let caller_qual = index.qualified_symbol(caller_id);
        let visibility_label = format_short_visibility(caller.visibility);
        let kind = format_short_decl_kind(caller.kind);
        let _ = writeln!(
            out,
            "  - [L{} {visibility_label} {kind}] {caller_qual} [{reason}] ({}:{}..{})",
            caller.level, caller.location.file, caller.location.line, caller.location.end_line
        );
        if let Some(lines) = index.call_sites.get(&(*caller_id, &decl.id)) {
            for &line in lines.iter().take(MAX_CALL_SITES_PER_CALLER) {
                let snippet =
                    source_line_snippet(&index.sources_by_file, &caller.location.file, line);
                let _ = writeln!(out, "      @ {}:{line}: {snippet}", caller.location.file);
            }
        }
    }
    if hops >= 2 {
        render_multi_hop_callers(out, index, &decl.id, &ordered, hops, caller_budget);
    }
}

fn render_multi_hop_callers<'a>(
    out: &mut String,
    index: &IndexedProjectGraph<'a>,
    root_id: &'a NodeId,
    hop1_ordered: &[&(&'a NodeId, String)],
    hops: usize,
    char_budget: usize,
) {
    let mut visited: BTreeSet<&'a NodeId> = BTreeSet::from([root_id]);
    let mut current_frontier: Vec<&'a NodeId> = Vec::new();
    for &&(caller_id, _) in hop1_ordered {
        if visited.insert(caller_id) {
            current_frontier.push(caller_id);
        }
    }
    for hop in 2..=hops {
        if current_frontier.is_empty() || out.len() >= char_budget {
            break;
        }
        let mut next_entries: Vec<(&'a NodeId, &'a NodeId, &str)> = Vec::new();
        let mut next_frontier: Vec<&'a NodeId> = Vec::new();
        for &via_id in &current_frontier {
            let Some(callers) = index.in_edges.get(via_id) else {
                continue;
            };
            for (caller_id, reason) in callers {
                if visited.insert(caller_id) {
                    next_entries.push((caller_id, via_id, reason.as_str()));
                    next_frontier.push(caller_id);
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
            let Some(caller) = index.decl_by_id.get(caller_id) else {
                continue;
            };
            let via_name = index
                .decl_by_id
                .get(via_id)
                .map_or(via_id.as_str(), |node| node.name.as_str());
            let caller_qual = index.qualified_symbol(caller_id);
            let visibility_label = format_short_visibility(caller.visibility);
            let kind = format_short_decl_kind(caller.kind);
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
    index: &IndexedProjectGraph<'_>,
    decl: &DeclarationNode,
    hops: usize,
    char_budget: usize,
) {
    let callees = index.out_edges.get(&decl.id).map_or(&[][..], Vec::as_slice);
    let _ = writeln!(out, "\n-> uses ({}):", callees.len());
    if callees.is_empty() {
        out.push_str("  (leaf declaration)\n");
        return;
    }
    for (callee_id, reason) in callees {
        if out.len() >= char_budget {
            out.push_str(TRUNCATED_BUDGET_LINE);
            break;
        }
        let Some(callee) = index.decl_by_id.get(callee_id) else {
            continue;
        };
        let callee_qual = index.qualified_symbol(callee_id);
        let visibility_label = format_short_visibility(callee.visibility);
        let kind = format_short_decl_kind(callee.kind);
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
        render_multi_hop_callees(out, index, &decl.id, callees, hops, char_budget);
    }
}

fn render_multi_hop_callees<'a>(
    out: &mut String,
    index: &IndexedProjectGraph<'a>,
    root_id: &'a NodeId,
    hop1_callees: &[(&'a NodeId, String)],
    hops: usize,
    char_budget: usize,
) {
    let mut visited: BTreeSet<&'a NodeId> = BTreeSet::from([root_id]);
    let mut current_frontier: Vec<&'a NodeId> = Vec::new();
    for (callee_id, _) in hop1_callees {
        if visited.insert(callee_id) {
            current_frontier.push(callee_id);
        }
    }
    for hop in 2..=hops {
        if current_frontier.is_empty() || out.len() >= char_budget {
            break;
        }
        let mut next_entries: Vec<(&'a NodeId, &'a NodeId, &str)> = Vec::new();
        let mut next_frontier: Vec<&'a NodeId> = Vec::new();
        for &via_id in &current_frontier {
            let Some(callees) = index.out_edges.get(via_id) else {
                continue;
            };
            for (callee_id, reason) in callees {
                if visited.insert(callee_id) {
                    next_entries.push((callee_id, via_id, reason.as_str()));
                    next_frontier.push(callee_id);
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
            let Some(callee) = index.decl_by_id.get(callee_id) else {
                continue;
            };
            let via_name = index
                .decl_by_id
                .get(via_id)
                .map_or(via_id.as_str(), |node| node.name.as_str());
            let callee_qual = index.qualified_symbol(callee_id);
            let visibility_label = format_short_visibility(callee.visibility);
            let kind = format_short_decl_kind(callee.kind);
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
    index: &IndexedProjectGraph<'_>,
    decl: &DeclarationNode,
    char_budget: usize,
) {
    if out.len() >= char_budget {
        return;
    }
    let Some(parent_id) = index.graph.parent_of(&decl.id) else {
        return;
    };
    let Some(group) = index.group_by_id.get(parent_id) else {
        return;
    };
    let siblings: Vec<&DeclarationNode> = group
        .children
        .iter()
        .filter(|&child_id| child_id != &decl.id)
        .filter_map(|child_id| index.decl_by_id.get(child_id).copied())
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
    for sib in siblings.into_iter().take(MAX_SIBLINGS_IN_SLICE) {
        let visibility_label = format_short_visibility(sib.visibility);
        let kind = format_short_decl_kind(sib.kind);
        let _ = writeln!(
            out,
            "  - [L{} {visibility_label} {kind}] {} :{}..{}",
            sib.level, sib.name, sib.location.line, sib.location.end_line
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
    facts: ProjectFacts,
    graph: AbstractionGraph,
}

fn build_all_lang_graphs(
    sources: &[CollectedSourceFile],
) -> anyhow::Result<Vec<BuiltLanguageGraph>> {
    let mut built_graphs = Vec::new();
    for lang in [Language::Rust, Language::Python] {
        if let Some(built) = build_lang_graph(sources, lang)? {
            built_graphs.push(built);
        }
    }
    Ok(built_graphs)
}

fn build_lang_graph(
    sources: &[CollectedSourceFile],
    lang: Language,
) -> anyhow::Result<Option<BuiltLanguageGraph>> {
    let lang_files: Vec<(&str, &str)> = sources
        .iter()
        .filter(|file| file.lang == lang)
        .map(|file| (file.relative_path.as_str(), file.source.as_str()))
        .collect();
    if lang_files.is_empty() {
        return Ok(None);
    }
    let facts = collect_project_facts_from_sources("project", lang, &lang_files);
    let graph = build_project_graph(&facts, ProjectDepth::Full)?;
    Ok(Some(BuiltLanguageGraph { lang, facts, graph }))
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
        let out = find_in_sources(&sources, "from_source", 10).unwrap();
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
        let out = slice_in_sources(&sources, &["Tracker::from_source".to_owned()], 1, 800).unwrap();
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
