//! Per-file declaration-scope and reference facts extracted from a [`ParsedFile`].
//!
//! Collects each scope's items, raw reference relations, `callee-before-caller` ordering edges,
//! recursion groups, and topological levels for a single parsed file. Multi-file project
//! resolution lives in `crate::code_lint::graph`.

use super::{
    CachedCallClusterFindings, DeclarationScope, DeclarationScopeKind, MethodVisibility,
    ParsedFile, ScopeItem, ScopeItemKind, analyze_declaration_scope, compute_public_roots,
    compute_stage3_callees, compute_tarjan_sccs,
};
use crate::diagnostic::Language;

const INIT_MODULE: &str = "__init__";
const SELF_SEGMENT: &str = "self";

/// Everything the analyzer knows about one scope (`Module`, `Class`, or `Impl`) in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFacts {
    /// Which syntactic container this scope covers.
    pub scope: ScopeLabel,
    /// Declarations in source order. Every index in [`ScopeFacts`] (`roots`, `source`, `target`,
    /// `higher`, `lower`) indexes into this slice.
    pub items: Vec<ItemFacts>,
    /// Direct intra-scope references, labeled by where the reference occurs.
    pub relations: Vec<RelationFact>,
    /// Downward `callee-before-caller` ordering edges (`higher -> lower`), labeled by why `higher`
    /// sits above `lower`.
    pub ordering: Vec<OrderingFact>,
}

/// Which syntactic container a [`ScopeFacts`] covers.
pub type ScopeLabel = DeclarationScopeKind;

/// One declaration in [`ScopeFacts::items`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFacts {
    /// Identifier name of the declaration.
    pub name: String,
    /// Callable (`fn` / `def`) or type (`struct`, `enum`, `union`, `trait`, `class`).
    pub kind: ItemKind,
    /// Public or private visibility tier.
    pub visibility: Visibility,
    /// True if this callable is a lifecycle constructor (`__init__`, `pub fn new`, etc.).
    pub is_constructor: bool,
    /// 1-based line of the declaration's name identifier.
    pub line: usize,
    /// 1-based last line of the declaration, when available.
    pub end_line: Option<usize>,
    /// 1-based column of the declaration's name identifier.
    pub column: usize,
    /// Compact one-line signature header (parameters + return type for callables; public fields or
    /// variants for types), when available.
    pub signature: Option<String>,
    /// Sorted indices in [`ScopeFacts::items`] of the public entrypoints that reach this item
    /// through private references (`Roots(f)`). A public item's roots are `[its_own_index]`.
    pub roots: Vec<usize>,
    /// Strongly connected component id over [`ScopeFacts::ordering`]. Mutual recursion shares an id.
    pub recursion_group: usize,
    /// Topological level in [`ScopeFacts::ordering`]: `0` for leaves, `1 + max(callee.level)` above
    /// them, with a recursion group sharing one level.
    pub level: usize,
    /// Call-cluster ordering findings reported on this declaration, in rule priority order.
    pub findings: Vec<FindingFact>,
}

/// Whether an [`ItemFacts`] is a callable or a type definition.
pub type ItemKind = ScopeItemKind;

/// Visibility tier of an [`ItemFacts`].
pub type Visibility = MethodVisibility;

/// A direct intra-scope reference from `items[source]` to `items[target]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RelationFact {
    /// Referring item index in [`ScopeFacts::items`].
    pub source: usize,
    /// Referenced item index in [`ScopeFacts::items`].
    pub target: usize,
    /// Where in `source` the reference to `target` occurs.
    pub kind: RelationKind,
}

/// Where a [`RelationFact`] occurs in the referring declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RelationKind {
    /// Callable parameter, generic bound, decorator, or `where` clause.
    Parameter,
    /// Callable return annotation.
    Return,
    /// Callable body.
    Body,
    /// Type definition itself: `struct` fields, `enum` variants, base classes, class attributes.
    Composition,
    /// Methods or inherent `impl` blocks attached to a type.
    Behavior,
}

/// A `callee-before-caller` ordering edge requiring `items[higher]` above `items[lower]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderingFact {
    /// Index of the higher-level item (caller, composite type, or signature contract type).
    pub higher: usize,
    /// Index of the lower-level item (callee, component type, or encapsulated detail).
    pub lower: usize,
    /// Why `higher` sits above `lower` in the abstraction order.
    pub reason: OrderingReason,
}

/// Why an [`OrderingFact`] places `higher` above `lower` (ADR 011 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderingReason {
    /// `Callable -> Callable`: `higher` calls `lower`.
    Call,
    /// `Type -> Type`: `higher`'s own fields, variants, or bases contain `lower`.
    Containment,
    /// `Type -> Callable` or `Type -> Type`: `higher`'s methods or `impl` blocks use `lower`.
    ImplBehavior,
    /// `Callable -> Type`: `higher` constructs or uses `lower` inside its body without exposing
    /// `lower` in its signature.
    Encapsulation,
    /// `Type -> Callable`: `lower` names `higher` in its parameter or return signature, so the
    /// type is the vocabulary of `lower`'s contract.
    SignatureContract,
}

/// A call-cluster lint finding attached to an [`ItemFacts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingFact {
    /// Rule name (`private-before-public`, `uncolocated-helper`, or `callee-before-caller`).
    pub rule: &'static str,
    /// Name of the owning or calling declaration the finding relates to.
    pub related: String,
}

/// Collects [`ScopeFacts`] for every declaration scope in `file`, in source order.
#[must_use]
pub fn collect_scope_facts(file: &ParsedFile) -> Vec<ScopeFacts> {
    let spans_and_sigs = collect_item_spans_and_signatures(file);
    let scopes = super::collect_declaration_scopes(file);
    scopes
        .iter()
        .map(|scope| scope_facts(scope, &spans_and_sigs))
        .collect()
}

fn scope_facts(
    scope: &DeclarationScope<'_>,
    spans_and_sigs: &std::collections::HashMap<(usize, String), (usize, String)>,
) -> ScopeFacts {
    let items = &scope.items;
    let count = items.len();
    let mut callers_of = vec![Vec::new(); count];
    for (caller, item) in items.iter().enumerate() {
        for &callee in &item.callees {
            if caller != callee && callee < count && !callers_of[callee].contains(&caller) {
                callers_of[callee].push(caller);
            }
        }
    }
    let stage3 = compute_stage3_callees(items);
    let scc_id = compute_cycle_groups(&stage3);
    let roots_of = compute_public_roots(items, &callers_of);
    let levels = compute_levels(&stage3, &scc_id);
    let mut findings = CachedCallClusterFindings::default();
    analyze_declaration_scope(scope, &mut findings);

    let mut item_facts: Vec<ItemFacts> = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let coordinate = item.name_node.start_coordinate();
            let (end_line, signature) = spans_and_sigs
                .get(&(coordinate.line, item.name.clone()))
                .map_or((None, None), |(end, sig)| (Some(*end), Some(sig.clone())));
            ItemFacts {
                name: item.name.clone(),
                kind: item.kind,
                visibility: item.visibility,
                is_constructor: item.is_constructor,
                line: coordinate.line,
                end_line,
                column: coordinate.column,
                signature,
                roots: roots_of[idx].clone(),
                recursion_group: scc_id[idx],
                level: levels[idx],
                findings: Vec::new(),
            }
        })
        .collect();

    for (rule, list) in [
        ("private-before-public", &findings.private_before_public),
        ("uncolocated-helper", &findings.uncolocated_helpers),
        ("callee-before-caller", &findings.callee_before_caller),
    ] {
        for finding in list {
            if let Some(slot) = items
                .iter()
                .position(|item| item.name_node.span() == finding.span)
            {
                item_facts[slot].findings.push(FindingFact {
                    rule,
                    related: finding.caller.clone(),
                });
            }
        }
    }

    ScopeFacts {
        scope: scope.kind.clone(),
        items: item_facts,
        relations: raw_relations(items),
        ordering: ordering_facts(items, &stage3),
    }
}

fn collect_item_spans_and_signatures(
    file: &ParsedFile,
) -> std::collections::HashMap<(usize, String), (usize, String)> {
    match file.lang() {
        Language::Rust => collect_rust_spans_and_signatures(file),
        Language::Python => collect_python_spans_and_signatures(file),
    }
}

fn collect_rust_spans_and_signatures(
    file: &ParsedFile,
) -> std::collections::HashMap<(usize, String), (usize, String)> {
    use ra_ap_syntax::AstNode as _;
    let mut out = std::collections::HashMap::new();
    let Some(parsed) = file.rs_parsed() else {
        return out;
    };
    for node in parsed.tree().syntax().descendants() {
        if file.is_in_rust_inline_test(node.text_range().start().into()) {
            continue;
        }
        record_rust_item_signature(&node, file, &mut out);
    }
    out
}

fn record_rust_item_signature(
    node: &ra_ap_syntax::SyntaxNode,
    file: &ParsedFile,
    out: &mut std::collections::HashMap<(usize, String), (usize, String)>,
) {
    use ra_ap_syntax::AstNode as _;
    use ra_ap_syntax::ast::{self, HasName as _, HasVisibility as _};

    let source = &file.source;
    let slice_header = |start: ra_ap_syntax::TextSize, end: ra_ap_syntax::TextSize| {
        let start_idx: usize = start.into();
        let end_idx: usize = end.into();
        collapse_ws(source[start_idx..end_idx].trim_end_matches(';'))
    };

    let item: Option<(ast::Name, String)> = match ast::Item::cast(node.clone()) {
        Some(ast::Item::Fn(func)) => {
            let sig_start = func
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| func.const_token().map(|tok| tok.text_range().start()))
                .or_else(|| func.async_token().map(|tok| tok.text_range().start()))
                .or_else(|| func.unsafe_token().map(|tok| tok.text_range().start()))
                .or_else(|| func.abi().map(|abi| abi.syntax().text_range().start()))
                .or_else(|| func.fn_token().map(|tok| tok.text_range().start()))
                .unwrap_or_else(|| func.syntax().text_range().start());
            let sig_end = func.body().map_or_else(
                || func.syntax().text_range().end(),
                |body| body.syntax().text_range().start(),
            );
            func.name()
                .map(|name| (name, slice_header(sig_start, sig_end)))
        }
        Some(ast::Item::Struct(st)) => {
            let head_start = st
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| st.struct_token().map(|tok| tok.text_range().start()))
                .unwrap_or_else(|| st.syntax().text_range().start());
            let head_end = st.field_list().map_or_else(
                || st.syntax().text_range().end(),
                |fl| fl.syntax().text_range().start(),
            );
            let header = slice_header(head_start, head_end);
            let body = rust_fields_summary(st.field_list(), true);
            st.name().map(|name| (name, format!("{header}{body}")))
        }
        Some(ast::Item::Enum(en)) => {
            let head_start = en
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| en.enum_token().map(|tok| tok.text_range().start()))
                .unwrap_or_else(|| en.syntax().text_range().start());
            let head_end = en.variant_list().map_or_else(
                || en.syntax().text_range().end(),
                |vl| vl.syntax().text_range().start(),
            );
            let header = slice_header(head_start, head_end);
            let variants: Vec<String> = en
                .variant_list()
                .into_iter()
                .flat_map(|vl| vl.variants())
                .filter_map(|var| {
                    let vname = var.name()?.text().to_string();
                    let payload = rust_fields_summary(var.field_list(), false);
                    Some(format!("{vname}{payload}"))
                })
                .collect();
            en.name()
                .map(|name| (name, format!("{header} {{ {} }}", variants.join(", "))))
        }
        Some(ast::Item::Trait(tr)) => {
            let head_start = tr
                .visibility()
                .map(|visibility| visibility.syntax().text_range().start())
                .or_else(|| tr.trait_token().map(|tok| tok.text_range().start()))
                .unwrap_or_else(|| tr.syntax().text_range().start());
            let head_end = tr.assoc_item_list().map_or_else(
                || tr.syntax().text_range().end(),
                |al| al.syntax().text_range().start(),
            );
            tr.name()
                .map(|name| (name, slice_header(head_start, head_end)))
        }
        _ => None,
    };

    if let Some((name_tok, sig)) = item {
        let line = file
            .line_index
            .line(name_tok.syntax().text_range().start().into());
        let end_line = file.line_index.line(node.text_range().end().into());
        out.insert((line, name_tok.text().to_string()), (end_line, sig));
    }
}

fn rust_fields_summary(
    fields_node: Option<ra_ap_syntax::ast::FieldList>,
    include_visibility: bool,
) -> String {
    use ra_ap_syntax::AstNode as _;
    use ra_ap_syntax::ast::{FieldList, HasName as _, HasVisibility as _};
    let visibility_prefix = |visibility: Option<ra_ap_syntax::ast::Visibility>| {
        if include_visibility {
            visibility
                .map(|node| format!("{} ", collapse_ws(&node.syntax().text().to_string())))
                .unwrap_or_default()
        } else {
            String::new()
        }
    };
    match fields_node {
        Some(FieldList::RecordFieldList(rec)) => {
            let fields: Vec<String> = rec
                .fields()
                .filter_map(|field| {
                    let fname = field.name()?.text().to_string();
                    let fty = collapse_ws(&field.ty()?.syntax().text().to_string());
                    let fvis = visibility_prefix(field.visibility());
                    Some(format!("{fvis}{fname}: {fty}"))
                })
                .collect();
            format!(" {{ {} }}", fields.join(", "))
        }
        Some(FieldList::TupleFieldList(tup)) => {
            let fields: Vec<String> = tup
                .fields()
                .filter_map(|field| {
                    let fty = collapse_ws(&field.ty()?.syntax().text().to_string());
                    let fvis = visibility_prefix(field.visibility());
                    Some(format!("{fvis}{fty}"))
                })
                .collect();
            format!("({})", fields.join(", "))
        }
        None => String::new(),
    }
}

fn collect_python_spans_and_signatures(
    file: &ParsedFile,
) -> std::collections::HashMap<(usize, String), (usize, String)> {
    use ruff_python_ast::Stmt;
    use ruff_text_size::Ranged as _;

    fn strip_py_header(raw: &str) -> String {
        let cleaned: Vec<&str> = raw
            .lines()
            .map(|line| line.split('#').next().unwrap_or(line).trim())
            .filter(|line| !line.is_empty())
            .collect();
        collapse_ws(cleaned.join(" ").trim_end_matches(':'))
    }

    fn visit_stmts(
        stmts: &[Stmt],
        file: &ParsedFile,
        out: &mut std::collections::HashMap<(usize, String), (usize, String)>,
    ) {
        let source = &file.source;
        for stmt in stmts {
            match stmt {
                Stmt::FunctionDef(func) => {
                    let line = file.line_index.line(func.name.range().start().to_usize());
                    let end = source[..func.range().end().to_usize().min(source.len())]
                        .trim_end()
                        .len();
                    let end_line = file.line_index.line(end);
                    let dec_prefix: String = func
                        .decorator_list
                        .iter()
                        .filter_map(|dec| {
                            let span = dec.expression.range();
                            let text = collapse_ws(
                                &source[span.start().to_usize()..span.end().to_usize()],
                            );
                            matches!(text.as_str(), "classmethod" | "staticmethod" | "property")
                                .then(|| format!("@{text} "))
                        })
                        .collect();
                    let sig_start = func.decorator_list.last().map_or_else(
                        || func.range().start().to_usize(),
                        |dec| dec.range().end().to_usize(),
                    );
                    let sig_end = func.body.first().map_or_else(
                        || func.range().end().to_usize(),
                        |first| first.range().start().to_usize(),
                    );
                    if sig_start <= sig_end && sig_end <= source.len() {
                        let sig = format!(
                            "{dec_prefix}{}",
                            strip_py_header(&source[sig_start..sig_end])
                        );
                        out.insert((line, func.name.to_string()), (end_line, sig));
                    }
                    visit_stmts(&func.body, file, out);
                }
                Stmt::ClassDef(cls) => {
                    let line = file.line_index.line(cls.name.range().start().to_usize());
                    let end = source[..cls.range().end().to_usize().min(source.len())]
                        .trim_end()
                        .len();
                    let end_line = file.line_index.line(end);
                    let sig_start = cls.decorator_list.last().map_or_else(
                        || cls.range().start().to_usize(),
                        |dec| dec.range().end().to_usize(),
                    );
                    let sig_end = cls.body.first().map_or_else(
                        || cls.range().end().to_usize(),
                        |first| first.range().start().to_usize(),
                    );
                    if sig_start <= sig_end && sig_end <= source.len() {
                        let header = strip_py_header(&source[sig_start..sig_end]);
                        let attrs = collect_py_class_public_attrs(&cls.body, source);
                        let full = if attrs.is_empty() {
                            header
                        } else {
                            format!("{header} {{ {} }}", attrs.join(", "))
                        };
                        out.insert((line, cls.name.to_string()), (end_line, full));
                    }
                    visit_stmts(&cls.body, file, out);
                }
                _ => {}
            }
        }
    }

    let mut out = std::collections::HashMap::new();
    if let Some(parsed) = file.py_module() {
        visit_stmts(&parsed.syntax().body, file, &mut out);
    }
    out
}

fn collect_py_class_public_attrs(body: &[ruff_python_ast::Stmt], source: &str) -> Vec<String> {
    use ruff_python_ast::{Expr, Stmt};
    use ruff_text_size::Ranged as _;

    let mut attrs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for stmt in body {
        match stmt {
            Stmt::AnnAssign(ann) => {
                if let Expr::Name(name) = &*ann.target
                    && !name.id.starts_with('_')
                    && seen.insert(name.id.to_string())
                {
                    let span = ann.annotation.range();
                    let annotation_text =
                        collapse_ws(&source[span.start().to_usize()..span.end().to_usize()]);
                    attrs.push(format!("{}: {annotation_text}", name.id));
                }
            }
            Stmt::FunctionDef(func) if matches!(func.name.as_str(), INIT_MODULE | "__new__") => {
                for inner in &func.body {
                    if let Stmt::Assign(assign) = inner {
                        for target in &assign.targets {
                            if let Expr::Attribute(attr) = target
                                && let Expr::Name(recv) = &*attr.value
                                && matches!(recv.id.as_str(), SELF_SEGMENT | "cls")
                                && !attr.attr.starts_with('_')
                                && seen.insert(attr.attr.to_string())
                            {
                                attrs.push(attr.attr.to_string());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    attrs
}

fn collapse_ws(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Raw references as the analyzer records them: by signature part for callables, and by
/// definition versus methods for types.
fn raw_relations(items: &[ScopeItem<'_>]) -> Vec<RelationFact> {
    let mut relations = Vec::new();
    for (source, item) in items.iter().enumerate() {
        let behavior: Vec<usize> = item
            .callees
            .iter()
            .copied()
            .filter(|target| !item.definition_callees.contains(target))
            .collect();
        let parts: &[(&[usize], RelationKind)] = match item.kind {
            ScopeItemKind::Callable => &[
                (&item.param_callees, RelationKind::Parameter),
                (&item.return_callees, RelationKind::Return),
                (&item.body_callees, RelationKind::Body),
            ],
            ScopeItemKind::Type => &[
                (&item.definition_callees, RelationKind::Composition),
                (&behavior, RelationKind::Behavior),
            ],
        };
        for &(targets, kind) in parts {
            for &target in targets {
                let relation = RelationFact {
                    source,
                    target,
                    kind,
                };
                if target != source && target < items.len() && !relations.contains(&relation) {
                    relations.push(relation);
                }
            }
        }
    }
    relations.sort();
    relations
}

/// The `callee-before-caller` ordering edges, each labeled with the ADR 011 §3.4 reason.
fn ordering_facts(items: &[ScopeItem<'_>], stage3: &[Vec<usize>]) -> Vec<OrderingFact> {
    let mut ordering = Vec::new();
    for (higher, lowers) in stage3.iter().enumerate() {
        for &lower in lowers {
            let reason = match (items[higher].kind, items[lower].kind) {
                (ScopeItemKind::Callable, ScopeItemKind::Callable) => OrderingReason::Call,
                (ScopeItemKind::Type, ScopeItemKind::Type) => {
                    if items[higher].definition_callees.contains(&lower) {
                        OrderingReason::Containment
                    } else {
                        OrderingReason::ImplBehavior
                    }
                }
                (ScopeItemKind::Callable, ScopeItemKind::Type) => OrderingReason::Encapsulation,
                (ScopeItemKind::Type, ScopeItemKind::Callable) => {
                    let signature = &items[lower];
                    if signature.param_callees.contains(&higher)
                        || signature.return_callees.contains(&higher)
                    {
                        OrderingReason::SignatureContract
                    } else {
                        OrderingReason::ImplBehavior
                    }
                }
            };
            ordering.push(OrderingFact {
                higher,
                lower,
                reason,
            });
        }
    }
    ordering.sort();
    ordering
}

/// Groups items that reach each other through ordering edges, including cycles through public
/// items. The analyzer's own groups stop at public boundaries, so they can leave cycles that
/// would make levelization loop forever.
fn compute_cycle_groups(stage3: &[Vec<usize>]) -> Vec<usize> {
    compute_tarjan_sccs(stage3.len(), |node, visit| {
        for &next in &stage3[node] {
            visit(next);
        }
    })
}

/// Levelizes the ordering graph: each recursion group is one unit, at level 0 when nothing is
/// below it, otherwise one above the highest group below it.
fn compute_levels(stage3: &[Vec<usize>], scc_id: &[usize]) -> Vec<usize> {
    fn group_level(
        group: usize,
        members: &[Vec<usize>],
        stage3: &[Vec<usize>],
        scc_id: &[usize],
        memo: &mut [Option<usize>],
    ) -> usize {
        if let Some(level) = memo[group] {
            return level;
        }
        let mut level = 0;
        for &member in &members[group] {
            for &lower in &stage3[member] {
                if scc_id[lower] != group {
                    level =
                        level.max(group_level(scc_id[lower], members, stage3, scc_id, memo) + 1);
                }
            }
        }
        memo[group] = Some(level);
        level
    }

    let group_count = scc_id.iter().copied().max().map_or(0, |max| max + 1);
    let mut members = vec![Vec::new(); group_count];
    for (idx, &group) in scc_id.iter().enumerate() {
        members[group].push(idx);
    }
    let mut memo = vec![None; group_count];
    scc_id
        .iter()
        .map(|&group| group_level(group, &members, stage3, scc_id, &mut memo))
        .collect()
}

#[cfg(test)]
mod tests {
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
        let summary: Vec<(&str, ItemKind, Visibility, &[usize], usize)> = facts
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

        assert_eq!(facts.scope, ScopeLabel::Module);
        assert_eq!(
            summary,
            [
                (
                    "parse_config",
                    ItemKind::Callable,
                    Visibility::Public,
                    &[0][..],
                    3
                ),
                (
                    "measure",
                    ItemKind::Callable,
                    Visibility::Private,
                    &[0][..],
                    2
                ),
                (
                    "frame_len",
                    ItemKind::Callable,
                    Visibility::Private,
                    &[0][..],
                    0
                ),
                ("Frame", ItemKind::Type, Visibility::Private, &[0][..], 1),
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
                OrderingFact {
                    higher: 0,
                    lower: 1,
                    reason: OrderingReason::Call
                },
                OrderingFact {
                    higher: 1,
                    lower: 2,
                    reason: OrderingReason::Call
                },
                OrderingFact {
                    higher: 1,
                    lower: 3,
                    reason: OrderingReason::Encapsulation
                },
                OrderingFact {
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
                rule: "callee-before-caller",
                related: "Frame".to_owned()
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
            [OrderingFact {
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
