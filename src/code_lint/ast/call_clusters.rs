//! Call-cluster ordering types and three-stage scope analyzer (`private-before-public`,
//! `uncolocated-helper`, and `callee-before-caller`).

use super::AstNode;
use crate::diagnostic::SourceSpan;
use std::collections::HashSet;

/// Cached call-cluster ordering finding without lifetime ties to `ParsedFile`.
#[derive(Clone)]
pub(in crate::code_lint::ast) struct CachedCallOrderFinding {
    pub(in crate::code_lint::ast) span: SourceSpan,
    pub(in crate::code_lint::ast) function: String,
    pub(in crate::code_lint::ast) caller: String,
    pub(in crate::code_lint::ast) anchor: PlacementAnchor,
}

/// Cached priority-ordered call-cluster findings for a file.
#[derive(Default)]
pub(in crate::code_lint::ast) struct CachedCallClusterFindings {
    pub(in crate::code_lint::ast) uncolocated_helpers: Vec<CachedCallOrderFinding>,
    pub(in crate::code_lint::ast) private_before_public: Vec<CachedCallOrderFinding>,
    pub(in crate::code_lint::ast) callee_before_caller: Vec<CachedCallOrderFinding>,
}

/// Visibility tier of a method or associated function in a Python class or Rust inherent `impl`
/// block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodVisibility {
    /// Public or dunder (`__name__`) method in Python; exported (`pub`, `pub(crate)`, `pub(super)`,
    /// `pub(in ...)`) method or associated function in Rust.
    Public,
    /// Single-underscore (`_name`) or name-mangled (`__name`) method in Python; bare `fn` without
    /// a visibility qualifier in Rust.
    Private,
}

/// A direct method or associated function of a Python `class` or Rust inherent `impl` block.
#[derive(Clone)]
pub struct TypeMethod<'a> {
    /// Identifier AST node of the method declaration.
    pub name_node: AstNode<'a>,
    /// Method identifier name.
    pub name: String,
    /// 1-based first line of the method declaration (including decorators or attributes).
    pub start_line: usize,
    /// Visibility tier (`Public` vs. `Private`).
    pub visibility: MethodVisibility,
    /// True if this method is a lifecycle constructor in Python (`__prepare__`,
    /// `__init_subclass__`, `__new__`, `__init__`, `__post_init__`, `__attrs_pre_init__`,
    /// `__attrs_post_init__`) or an exported `new` / `try_new` / `new_*` / `try_new_*` associated
    /// function without a `self` receiver in a Rust inherent `impl` block.
    pub is_constructor: bool,
}

/// A Python `class` or Rust inherent `impl` block and its direct methods in source order.
pub struct TypeMethodScope<'a> {
    /// Name of the enclosing class or implemented type.
    pub type_name: String,
    /// Direct methods in source order (with Python `@overload` signatures and
    /// `@<prop>.setter` / `@<prop>.deleter` accessors grouped at their first declaration).
    pub methods: Vec<TypeMethod<'a>>,
}

/// Whether a [`ScopeItem`] is a callable (`fn` / `def`) or a type definition (`struct`, `enum`,
/// `union`, `trait`, `class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeItemKind {
    /// A function or method definition.
    Callable,
    /// A class, struct, enum, union, or trait definition.
    Type,
}

/// A function, method, or type definition inside a module, Python `class`, or Rust inherent
/// `impl` scope.
#[derive(Clone)]
pub(in crate::code_lint::ast) struct ScopeItem<'a> {
    /// Identifier AST node of the declaration.
    pub(in crate::code_lint::ast) name_node: AstNode<'a>,
    /// Declaration identifier name.
    pub(in crate::code_lint::ast) name: String,
    /// 1-based first line of the declaration (including decorators or attributes).
    pub(in crate::code_lint::ast) start_line: usize,
    /// 1-based last line of the declaration (including contiguous overloads/accessors or inherent
    /// `impl` blocks).
    pub(in crate::code_lint::ast) end_line: usize,
    /// Whether this item is a callable or a type definition.
    pub(in crate::code_lint::ast) kind: ScopeItemKind,
    /// Visibility tier (`Public` vs. `Private`).
    pub(in crate::code_lint::ast) visibility: MethodVisibility,
    /// True if this callable is a Tier 0 lifecycle constructor.
    pub(in crate::code_lint::ast) is_constructor: bool,
    /// Indices in the enclosing [`DeclarationScope::items`] slice of all sibling items directly
    /// called or referenced by this item (signature + body).
    pub(in crate::code_lint::ast) callees: Vec<usize>,
    /// Indices of sibling items referenced in this callable's parameter, generic, decorator, or
    /// `where`-clause signature.
    pub(in crate::code_lint::ast) param_callees: Vec<usize>,
    /// Indices of sibling items referenced in this callable's return type signature.
    pub(in crate::code_lint::ast) return_callees: Vec<usize>,
    /// Indices of sibling items referenced in this callable's body (or in a type definition /
    /// its attached `impl` blocks).
    pub(in crate::code_lint::ast) body_callees: Vec<usize>,
    /// For a type, indices of sibling items referenced by its own definition (fields,
    /// variants, bases, class-level statements), excluding its methods and `impl` blocks.
    /// Empty for callables.
    pub(in crate::code_lint::ast) definition_callees: Vec<usize>,
    /// Smallest index in [`DeclarationScope::items`] before which this item must remain due to
    /// top-to-bottom runtime evaluation (e.g. Python import-time or class-definition-time uses).
    pub(in crate::code_lint::ast) pinned_before: Option<usize>,
}

/// Which syntactic container a [`DeclarationScope`] covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationScopeKind {
    /// Top-level items of a module (or of an inline Rust `mod`).
    Module,
    /// Direct methods of a Python `class`.
    Class {
        /// The class name.
        name: String,
    },
    /// Direct methods of a Rust inherent `impl`.
    Impl {
        /// The self type name.
        type_name: String,
    },
}

/// A module, Python `class`, or Rust inherent `impl` scope with its direct declarations in
/// source order and their intra-scope reference edges.
pub(in crate::code_lint::ast) struct DeclarationScope<'a> {
    /// Which container this scope covers.
    pub(in crate::code_lint::ast) kind: DeclarationScopeKind,
    /// Direct declarations in source order.
    pub(in crate::code_lint::ast) items: Vec<ScopeItem<'a>>,
}

/// Splits `declarations` into consecutive segments, starting a new segment whenever `name`
/// returns a name already declared in the current segment.
pub(in crate::code_lint::ast) fn split_at_redefinitions<T>(
    declarations: Vec<T>,
    name: impl Fn(&T) -> &str,
) -> Vec<Vec<T>> {
    let mut segments = Vec::new();
    let mut current_segment = Vec::new();
    let mut seen_names = HashSet::new();
    for declaration in declarations {
        if !seen_names.insert(name(&declaration).to_owned()) {
            segments.push(std::mem::take(&mut current_segment));
            seen_names.clear();
            seen_names.insert(name(&declaration).to_owned());
        }
        current_segment.push(declaration);
    }
    if !current_segment.is_empty() {
        segments.push(current_segment);
    }
    segments
}

/// Appends `item` to `vec` if it is not already present.
pub(in crate::code_lint::ast) fn push_unique<T: PartialEq>(vec: &mut Vec<T>, item: T) {
    if !vec.contains(&item) {
        vec.push(item);
    }
}

/// Appends each element of `items` to `vec` if it is not already present.
pub(in crate::code_lint::ast) fn extend_unique<T: PartialEq>(
    vec: &mut Vec<T>,
    items: impl IntoIterator<Item = T>,
) {
    for item in items {
        push_unique(vec, item);
    }
}

/// A sibling declaration next to which a misplaced item belongs, along with its 1-based boundary
/// line number (its starting line when moving above it, or its ending line when moving below it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementAnchor {
    /// Identifier name of the anchor declaration (`{anchor}`).
    pub name: String,
    /// 1-based boundary line number of the anchor declaration (`{line}`).
    pub line: usize,
}

impl PlacementAnchor {
    /// Anchor for moving an item directly above `declaration_node` (records `start_line()`).
    #[must_use]
    pub fn before(name: impl Into<String>, declaration_node: AstNode<'_>) -> Self {
        Self {
            name: name.into(),
            line: declaration_node.start_line(),
        }
    }

    /// Anchor for moving an item immediately below a declaration ending at `end_line`.
    #[must_use]
    pub fn after(name: impl Into<String>, end_line: usize) -> Self {
        Self {
            name: name.into(),
            line: end_line,
        }
    }
}

/// A non-method member (a type-annotated Python class/instance attribute or a Rust associated
/// `type` / `const` item) declared after a method definition in the same `class`, `impl`, or
/// `trait` body.
pub struct MemberAfterMethod<'a> {
    /// Name of the enclosing Python `class` or Rust `impl` self-type / `trait`.
    pub container_name: String,
    /// Member identifier name.
    pub name: String,
    /// First method in the enclosing container, directly above which the member belongs.
    pub first_method: PlacementAnchor,
    /// Diagnostic target AST node (full `Stmt::AnnAssign` in Python; identifier in Rust).
    pub node: AstNode<'a>,
}

/// A misplaced declaration reported by the call-cluster ordering analyzer.
pub struct CallOrderFinding<'a> {
    /// Identifier AST node of the misplaced declaration.
    pub name_node: AstNode<'a>,
    /// Name of the misplaced declaration (`{function}`).
    pub function: String,
    /// Name of the owning or calling declaration it relates to (`{caller}`).
    pub caller: String,
    /// Sibling declaration immediately below which `function` belongs.
    pub anchor: PlacementAnchor,
}

/// Priority-ordered findings for the three declaration-cluster ordering rules across a file:
/// 1. `private-before-public` (private helper above its public caller `pos < max(Roots(h))`,
///    or unrooted private declaration above `last_pub`)
/// 2. `uncolocated-helper` (private helper after its public callers `pos > max(Roots(h))` that is
///    neither in Place 1 immediately after its single consumer nor in Place 2 in the trailing
///    private helper section after `last_pub`)
/// 3. `callee-before-caller` (private callee before private caller among remaining items)
///
/// Evaluating all three together guarantees mutual exclusion so a single misplaced declaration
/// is never double-reported across rules.
pub struct CallClusterFindings<'a> {
    /// Findings for `uncolocated-helper` (private helpers after their public callers that are
    /// neither in Place 1 nor in Place 2).
    pub uncolocated_helpers: Vec<CallOrderFinding<'a>>,
    /// Findings for `private-before-public` (private helpers declared before their
    /// public callers).
    pub private_before_public: Vec<CallOrderFinding<'a>>,
    /// Findings for `callee-before-caller` (private callees declared before their private callers).
    pub callee_before_caller: Vec<CallOrderFinding<'a>>,
}

/// Analyzes a single [`DeclarationScope`], resolves the target placement anchor for each finding,
/// and appends the findings to `out`.
pub(in crate::code_lint::ast) fn analyze_declaration_scope(
    scope: &DeclarationScope<'_>,
    out: &mut CachedCallClusterFindings,
) {
    let mut raw = RawScopeFindings::default();
    collect_raw_scope_findings(scope, &mut raw);
    if raw.is_empty() {
        return;
    }
    let mut orig_flagged = vec![false; scope.items.len()];
    for finding in raw.all() {
        orig_flagged[finding.item_idx] = true;
    }
    let to_cached = |finding: &RawScopeFinding| -> CachedCallOrderFinding {
        let anchor_idx =
            find_best_anchor(scope, finding.item_idx, finding.caller_idx, &orig_flagged);
        let item = &scope.items[finding.item_idx];
        let anchor_item = &scope.items[anchor_idx];
        CachedCallOrderFinding {
            span: item.name_node.span(),
            function: item.name.clone(),
            caller: scope.items[finding.caller_idx].name.clone(),
            anchor: PlacementAnchor::after(&anchor_item.name, anchor_item.end_line),
        }
    };
    out.uncolocated_helpers
        .extend(raw.uncolocated_helpers.iter().map(to_cached));
    out.private_before_public
        .extend(raw.private_before_public.iter().map(to_cached));
    out.callee_before_caller
        .extend(raw.callee_before_caller.iter().map(to_cached));
}

/// Finds the index in `scope.items` immediately after which item `from` can be moved to clear its
/// finding without introducing new findings, preferring the slot closest to `from`.
fn find_best_anchor(
    scope: &DeclarationScope<'_>,
    from: usize,
    caller_idx: usize,
    orig_flagged: &[bool],
) -> usize {
    let count = scope.items.len();
    let pinned_before = scope.items[from].pinned_before;
    (0..count)
        .filter_map(|after| {
            if after == from || after + 1 == from || pinned_before.is_some_and(|pin| after >= pin) {
                return None;
            }
            let reordered = reorder_scope_after(scope, from, after);
            let mut candidate = RawScopeFindings::default();
            collect_raw_scope_findings(&reordered.scope, &mut candidate);
            if candidate
                .all()
                .any(|finding| reordered.orig_idx_of[finding.item_idx] == from)
            {
                return None;
            }
            let creates_new_finding = candidate
                .all()
                .any(|finding| !orig_flagged[reordered.orig_idx_of[finding.item_idx]]);
            let new_pos = if from < after { after } else { after + 1 };
            Some(CandidateSlot {
                creates_new_finding,
                distance: new_pos.abs_diff(from),
                new_pos,
                after,
            })
        })
        .min()
        .map_or(caller_idx, |slot| slot.after)
}

/// Ranking key for a candidate placement slot in [`find_best_anchor`], ordered by:
/// 1. whether moving the item introduces a new finding elsewhere in the scope,
/// 2. vertical distance (`|new_pos - from|`),
/// 3. earlier position in the scope (`new_pos`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateSlot {
    creates_new_finding: bool,
    distance: usize,
    new_pos: usize,
    after: usize,
}

struct ReorderedScope<'a> {
    scope: DeclarationScope<'a>,
    orig_idx_of: Vec<usize>,
}

/// Returns a copy of `scope` with item `from` moved immediately after item `after`, paired with
/// the mapping from each reordered item index back to its original index in `scope.items`.
fn reorder_scope_after<'a>(
    scope: &DeclarationScope<'a>,
    from: usize,
    after: usize,
) -> ReorderedScope<'a> {
    let count = scope.items.len();
    let mut orig_idx_of: Vec<usize> = (0..count).filter(|&idx| idx != from).collect();
    let new_pos = if from < after { after } else { after + 1 };
    orig_idx_of.insert(new_pos, from);

    let mut new_idx_of = vec![0; count];
    for (new_idx, &orig_idx) in orig_idx_of.iter().enumerate() {
        new_idx_of[orig_idx] = new_idx;
    }

    let remap =
        |indices: &[usize]| -> Vec<usize> { indices.iter().map(|&idx| new_idx_of[idx]).collect() };
    let items = orig_idx_of
        .iter()
        .map(|&orig_idx| {
            let item = &scope.items[orig_idx];
            ScopeItem {
                name_node: item.name_node,
                name: item.name.clone(),
                start_line: item.start_line,
                end_line: item.end_line,
                kind: item.kind,
                visibility: item.visibility,
                is_constructor: item.is_constructor,
                callees: remap(&item.callees),
                param_callees: remap(&item.param_callees),
                return_callees: remap(&item.return_callees),
                body_callees: remap(&item.body_callees),
                definition_callees: remap(&item.definition_callees),
                pinned_before: item
                    .pinned_before
                    .map(|pin| new_idx_of.get(pin).copied().unwrap_or(pin)),
            }
        })
        .collect();

    ReorderedScope {
        scope: DeclarationScope {
            kind: scope.kind.clone(),
            items,
        },
        orig_idx_of,
    }
}

/// Index-based call-cluster findings for a single [`DeclarationScope`] before anchor resolution.
#[derive(Default)]
struct RawScopeFindings {
    uncolocated_helpers: Vec<RawScopeFinding>,
    private_before_public: Vec<RawScopeFinding>,
    callee_before_caller: Vec<RawScopeFinding>,
}

impl RawScopeFindings {
    const fn is_empty(&self) -> bool {
        self.uncolocated_helpers.is_empty()
            && self.private_before_public.is_empty()
            && self.callee_before_caller.is_empty()
    }

    fn all(&self) -> impl Iterator<Item = &RawScopeFinding> {
        self.uncolocated_helpers
            .iter()
            .chain(&self.private_before_public)
            .chain(&self.callee_before_caller)
    }
}

/// Evaluates the three call-cluster ordering stages on `scope` and appends raw findings to `out`.
fn collect_raw_scope_findings(scope: &DeclarationScope<'_>, out: &mut RawScopeFindings) {
    let callables = &scope.items;
    let count = callables.len();
    if count < 2 {
        return;
    }

    let mut callers_of = vec![Vec::new(); count];
    for (caller_idx, callable) in callables.iter().enumerate() {
        for &callee_idx in &callable.callees {
            if caller_idx != callee_idx
                && callee_idx < count
                && !callers_of[callee_idx].contains(&caller_idx)
            {
                callers_of[callee_idx].push(caller_idx);
            }
        }
    }

    let stage3_callees = compute_stage3_callees(callables);
    let scc_id = compute_tarjan_scc(callables, &stage3_callees);
    let roots_of = compute_public_roots(callables, &callers_of);

    let last_pub = (0..count)
        .rev()
        .find(|&idx| callables[idx].visibility == MethodVisibility::Public);
    let mut flagged_p1_or_p2 = vec![false; count];

    for pos in 0..count {
        if callables[pos].visibility == MethodVisibility::Public {
            continue;
        }
        let pinned_before = callables[pos].pinned_before;
        let roots = &roots_of[pos];
        let Some(&max_root) = roots.last() else {
            if let Some(last_pub_idx) = last_pub
                && pinned_before.is_none_or(|pin| pin > last_pub_idx)
                && pos < last_pub_idx
            {
                out.private_before_public.push(RawScopeFinding {
                    item_idx: pos,
                    caller_idx: last_pub_idx,
                });
                flagged_p1_or_p2[pos] = true;
            }
            continue;
        };
        if pos < max_root {
            if pinned_before.is_none_or(|pin| pin > max_root) {
                out.private_before_public.push(RawScopeFinding {
                    item_idx: pos,
                    caller_idx: max_root,
                });
                flagged_p1_or_p2[pos] = true;
            }
        } else if let Some(last_pub_idx) = last_pub
            && !is_valid_helper_placement(callables, &roots_of, pos, last_pub_idx)
            && (roots.len() == 1 || pinned_before.is_none_or(|pin| pin > last_pub_idx))
        {
            out.uncolocated_helpers.push(RawScopeFinding {
                item_idx: pos,
                caller_idx: max_root,
            });
            flagged_p1_or_p2[pos] = true;
        }
    }

    for callee in 0..count {
        if flagged_p1_or_p2[callee] || callables[callee].visibility == MethodVisibility::Public {
            continue;
        }
        let pinned_before = callables[callee].pinned_before;
        let last_caller = ((callee + 1)..count)
            .filter(|&caller| {
                !flagged_p1_or_p2[caller]
                    && callables[caller].visibility == MethodVisibility::Private
                    && stage3_callees[caller].contains(&callee)
                    && scc_id[caller] != scc_id[callee]
                    && pinned_before.is_none_or(|pin| pin > caller)
                    // Do not let an unrooted private caller pull a rooted helper below its
                    // public root's cluster.
                    && (!roots_of[caller].is_empty() || roots_of[callee].is_empty())
            })
            .max();
        if let Some(caller_idx) = last_caller {
            out.callee_before_caller.push(RawScopeFinding {
                item_idx: callee,
                caller_idx,
            });
        }
    }
}

#[derive(Clone, Copy)]
struct RawScopeFinding {
    item_idx: usize,
    caller_idx: usize,
}

/// Returns true if a private helper at `pos` (already after all of its `roots_of[pos]`) is
/// either immediately after its single consumer or in the trailing helper section after
/// `last_pub_idx`.
fn is_valid_helper_placement(
    callables: &[ScopeItem<'_>],
    roots_of: &[Vec<usize>],
    pos: usize,
    last_pub_idx: usize,
) -> bool {
    let roots = &roots_of[pos];
    if roots.len() != 1 {
        return pos > last_pub_idx;
    }
    let owner = roots[0];
    let in_owner_cluster = |idx: usize| -> bool {
        roots_of[idx] == *roots
            || (callables[owner].is_constructor
                && (callables[idx].is_constructor
                    || (roots_of[idx].len() == 1 && callables[roots_of[idx][0]].is_constructor)))
    };
    let is_just_after_owner = ((owner + 1)..pos).all(in_owner_cluster);
    let is_at_scope_end =
        pos > last_pub_idx && ((owner + 1)..last_pub_idx).all(|mid| roots_of[mid] != *roots);
    is_just_after_owner || is_at_scope_end
}

/// Computes Strongly Connected Component IDs for `callables` using Tarjan's algorithm over
/// private-to-private Stage-3 edges (stopping at public boundaries).
fn compute_tarjan_scc(callables: &[ScopeItem<'_>], stage3_callees: &[Vec<usize>]) -> Vec<usize> {
    let count = callables.len();
    compute_tarjan_sccs(count, |node, visit| {
        if callables[node].visibility == MethodVisibility::Private {
            for &callee in &stage3_callees[node] {
                if callee < count && callables[callee].visibility == MethodVisibility::Private {
                    visit(callee);
                }
            }
        }
    })
}

/// Computes the Stage-3 (`callee-before-caller`) downward abstraction edges for each item in
/// `callables`:
/// - `Callable -> Callable`, `Type -> Type`, and `Type -> Callable` include all `callees`.
/// - `Callable -> Type` includes a type `callee` iff `caller` encapsulates `callee` inside its
///   body (`EffectiveBody(caller) \ SigClosure(caller)`).
/// - `Type -> Callable` additionally includes every callable whose parameter or return signature
///   names the type directly: the type is the vocabulary of that callable's contract.
pub(in crate::code_lint::ast) fn compute_stage3_callees(
    callables: &[ScopeItem<'_>],
) -> Vec<Vec<usize>> {
    let count = callables.len();
    let mut out = vec![Vec::new(); count];
    for (caller, item) in callables.iter().enumerate() {
        if item.kind == ScopeItemKind::Type {
            out[caller] = item
                .callees
                .iter()
                .copied()
                .filter(|&callee| callee != caller && callee < count)
                .collect();
            continue;
        }

        let mut sig_closure = vec![false; count];
        let mut stack: Vec<usize> = item
            .param_callees
            .iter()
            .chain(&item.return_callees)
            .copied()
            .filter(|&idx| idx < count && callables[idx].kind == ScopeItemKind::Type)
            .collect();
        while let Some(cur) = stack.pop() {
            if sig_closure[cur] {
                continue;
            }
            sig_closure[cur] = true;
            for &next in &callables[cur].callees {
                if next < count && callables[next].kind == ScopeItemKind::Type {
                    stack.push(next);
                }
            }
        }

        let mut edges = Vec::new();
        for &callee in &item.callees {
            if callee == caller || callee >= count {
                continue;
            }
            if callables[callee].kind == ScopeItemKind::Callable {
                edges.push(callee);
            }
        }
        for callee in 0..count {
            if callee == caller
                || callables[callee].kind != ScopeItemKind::Type
                || sig_closure[callee]
            {
                continue;
            }
            let in_body = item.body_callees.contains(&callee);
            let in_inferred_return = item.body_callees.iter().any(|&callee_fn| {
                callee_fn < count
                    && callables[callee_fn].kind == ScopeItemKind::Callable
                    && callables[callee_fn].return_callees.contains(&callee)
            });
            if in_body || in_inferred_return {
                edges.push(callee);
            }
        }
        out[caller] = edges;
    }
    for (callable, item) in callables.iter().enumerate() {
        if item.kind != ScopeItemKind::Callable {
            continue;
        }
        for &sig_type in item.param_callees.iter().chain(&item.return_callees) {
            if sig_type < count
                && callables[sig_type].kind == ScopeItemKind::Type
                && !out[sig_type].contains(&callable)
            {
                out[sig_type].push(callable);
            }
        }
    }
    out
}

/// Computes `Roots(f)` for each item in `callables`: the sorted indices of public entrypoints
/// that reach `f` through private reference paths (stopping at public boundaries).
pub(in crate::code_lint::ast) fn compute_public_roots(
    callables: &[ScopeItem<'_>],
    callers_of: &[Vec<usize>],
) -> Vec<Vec<usize>> {
    let count = callables.len();
    let mut roots_of = vec![Vec::new(); count];
    for idx in 0..count {
        if callables[idx].visibility == MethodVisibility::Public {
            roots_of[idx] = vec![idx];
        } else {
            let mut visited = vec![false; count];
            let mut stack = vec![idx];
            let mut pub_roots = Vec::new();
            while let Some(cur) = stack.pop() {
                if visited[cur] {
                    continue;
                }
                visited[cur] = true;
                for &caller in &callers_of[cur] {
                    if callables[caller].visibility == MethodVisibility::Public {
                        pub_roots.push(caller);
                    } else {
                        stack.push(caller);
                    }
                }
            }
            pub_roots.sort_unstable();
            pub_roots.dedup();
            roots_of[idx] = pub_roots;
        }
    }
    roots_of
}

/// Computes Strongly Connected Component IDs for `0..count` using Tarjan's algorithm over the
/// directed edges yielded by `for_each_neighbor`.
pub(in crate::code_lint::ast) fn compute_tarjan_sccs(
    count: usize,
    mut for_each_neighbor: impl FnMut(usize, &mut dyn FnMut(usize)),
) -> Vec<usize> {
    struct TarjanState {
        next_index: usize,
        next_scc: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        scc_id: Vec<usize>,
    }

    impl TarjanState {
        fn strongconnect(
            &mut self,
            node: usize,
            for_each_neighbor: &mut impl FnMut(usize, &mut dyn FnMut(usize)),
        ) {
            let current_index = self.next_index;
            self.next_index += 1;
            self.indices[node] = Some(current_index);
            self.lowlink[node] = current_index;
            self.stack.push(node);
            self.on_stack[node] = true;

            let mut neighbors = Vec::new();
            for_each_neighbor(node, &mut |neighbor| neighbors.push(neighbor));
            for callee in neighbors {
                if callee >= self.indices.len() {
                    continue;
                }
                if self.indices[callee].is_none() {
                    self.strongconnect(callee, for_each_neighbor);
                    self.lowlink[node] = self.lowlink[node].min(self.lowlink[callee]);
                } else if self.on_stack[callee]
                    && let Some(callee_index) = self.indices[callee]
                {
                    self.lowlink[node] = self.lowlink[node].min(callee_index);
                }
            }

            if Some(self.lowlink[node]) == self.indices[node] {
                let id = self.next_scc;
                self.next_scc += 1;
                while let Some(member) = self.stack.pop() {
                    self.on_stack[member] = false;
                    self.scc_id[member] = id;
                    if member == node {
                        break;
                    }
                }
            }
        }
    }

    let mut state = TarjanState {
        next_index: 0,
        next_scc: 0,
        indices: vec![None; count],
        lowlink: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        scc_id: vec![0; count],
    };
    for node in 0..count {
        if state.indices[node].is_none() {
            state.strongconnect(node, &mut for_each_neighbor);
        }
    }
    state.scc_id
}
