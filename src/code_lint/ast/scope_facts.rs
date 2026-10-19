//! Per-file declaration-scope facts extracted from a [`ParsedFile`].
//!
//! Collects each scope's items, raw reference relations, `callee-before-caller` ordering edges,
//! public-root ownership, recursion groups, topological levels, and ordering findings for a single
//! parsed file. Multi-file project resolution lives in `crate::code_lint::graph`.

use super::{
    DeclarationScope, DeclarationScopeKind, OrderingEdge, OrderingReason, Ownership, ParsedFile,
    PlacementAnchor, RelationFact, ScopeGraph, ScopeItem, ScopeItemKind, Visibility,
    scope_findings,
};
use crate::diagnostic::Language;

const INIT_MODULE: &str = "__init__";
const SELF_SEGMENT: &str = "self";

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
    /// 1-based line of the declaration's name identifier.
    pub line: usize,
    /// 1-based last line of the declaration, including its attached `impl` blocks or grouped
    /// overloads and accessors.
    pub end_line: Option<usize>,
    /// 1-based column of the declaration's name identifier.
    pub column: usize,
    /// Compact one-line signature header (parameters + return type for callables; public fields or
    /// variants for types), when available.
    pub signature: Option<String>,
    /// Sorted indices in [`ScopeFacts::items`] of the public entrypoints that reach this item
    /// through private references (`Roots(f)`). A public item's roots are `[its_own_index]`.
    pub roots: Vec<usize>,
    /// Sorted indices in [`ScopeFacts::items`] of the public entrypoint units that own this item
    /// (`roots` with constructors collapsed to the scope's first constructor).
    pub unit_owners: Vec<usize>,
    /// Public entrypoint unit ownership (`Owner(unit)`, `Shared`, or `Unowned`), derived from
    /// [`Self::unit_owners`].
    pub ownership: Ownership,
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
    /// Rule name (`constructor-after-method`, `private-before-public`, `uncolocated-helper`, or
    /// `callee-before-caller`).
    pub rule: &'static str,
    /// Name of the owning or calling declaration the finding relates to.
    pub related: String,
    /// Why `related` sits above this item (`callee-before-caller` only).
    pub reason: Option<OrderingReason>,
    /// Sibling declaration next to which this item belongs.
    pub anchor: PlacementAnchor,
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
    let graph = ScopeGraph::new(items);
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
                roots: graph.roots[idx].clone(),
                unit_owners: graph.unit_owners[idx].clone(),
                ownership: graph.ownership[idx],
                recursion_group: graph.recursion_group[idx],
                level: graph.level[idx],
                findings: Vec::new(),
            }
        })
        .collect();
    for finding in scope_findings(scope, &graph) {
        item_facts[finding.item].findings.push(FindingFact {
            rule: finding.rule.name(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::ast::RelationKind;
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
                    item.ownership,
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
                    Ownership::Owner(0),
                    3
                ),
                (
                    "measure",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    &[0][..],
                    Ownership::Owner(0),
                    2
                ),
                (
                    "frame_len",
                    ScopeItemKind::Callable,
                    Visibility::Private,
                    &[0][..],
                    Ownership::Owner(0),
                    0
                ),
                (
                    "Frame",
                    ScopeItemKind::Type,
                    Visibility::Private,
                    &[0][..],
                    Ownership::Owner(0),
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
                rule: "callee-before-caller",
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
