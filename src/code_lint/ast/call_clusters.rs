//! Declaration-scope model and the declarative call-cluster placement rules
//! (`constructor-after-method`, `private-before-public`, `uncolocated-helper`, and
//! `callee-before-caller`, ADR 011).
//!
//! Each rule is a set of [`Placement`] constraints generated from the scope's [`ScopeGraph`];
//! one evaluator checks them against the source order and finds where a misplaced item belongs.

mod scope_graph;

use self::scope_graph::Ownership;
pub(in crate::code_lint::ast) use self::scope_graph::ScopeGraph;
pub use self::scope_graph::{
    OrderingEdge, OrderingReason, RelationFact, RelationKind, levels, strongly_connected_components,
};

use super::AstNode;
use crate::diagnostic::SourceSpan;
use std::collections::HashSet;

/// Visibility tier of a declaration within its scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Visibility {
    /// Public or dunder (`__name__`) item in Python; exported (`pub`, `pub(crate)`, `pub(super)`,
    /// `pub(in ...)`) item or `fn main` in Rust.
    Public,
    /// Single-underscore (`_name`) or name-mangled (`__name`) item in Python; item without a
    /// visibility qualifier in Rust.
    Private,
}

/// Whether a [`ScopeItem`] is a callable (`fn` / `def`) or a type definition (`struct`, `enum`,
/// `union`, `trait`, `class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// Compact one-line signature: the header of a callable, or the header and public fields or
    /// variants of a type, when the language module renders one.
    pub(in crate::code_lint::ast) signature: Option<String>,
    /// Whether this item is a callable or a type definition.
    pub(in crate::code_lint::ast) kind: ScopeItemKind,
    /// Visibility tier (`Public` vs. `Private`).
    pub(in crate::code_lint::ast) visibility: Visibility,
    /// True if this callable is a Tier 0 lifecycle constructor.
    pub(in crate::code_lint::ast) is_constructor: bool,
    /// Direct references from this declaration to sibling items in [`DeclarationScope::items`],
    /// paired with the [`RelationKind`] where each reference occurs.
    pub(in crate::code_lint::ast) references: Vec<(usize, RelationKind)>,
    /// Smallest index in [`DeclarationScope::items`] before which this item must remain due to
    /// top-to-bottom runtime evaluation (e.g. Python import-time or class-definition-time uses).
    pub(in crate::code_lint::ast) pinned_before: Option<usize>,
}

impl ScopeItem<'_> {
    /// Returns true if this item references `target` in `kind`.
    pub(in crate::code_lint::ast) fn has_reference(
        &self,
        target: usize,
        kind: RelationKind,
    ) -> bool {
        self.references.contains(&(target, kind))
    }

    /// Yields the sibling item indices referenced in any of `kinds`.
    pub(in crate::code_lint::ast) fn references_in<'a>(
        &'a self,
        kinds: &'a [RelationKind],
    ) -> impl Iterator<Item = usize> + 'a {
        self.references
            .iter()
            .filter(move |&&(_, relation_kind)| kinds.contains(&relation_kind))
            .map(|&(target, _)| target)
    }
}

/// Appends `(target, kind)` for each `target` in `targets` to `references` if not already present.
pub(in crate::code_lint::ast) fn extend_references(
    references: &mut Vec<(usize, RelationKind)>,
    targets: impl IntoIterator<Item = usize>,
    kind: RelationKind,
) {
    for target in targets {
        if kind == RelationKind::Behavior
            && references.contains(&(target, RelationKind::Composition))
        {
            continue;
        }
        push_unique(references, (target, kind));
    }
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

/// A call-cluster placement rule, in priority order: an item violating several rules is
/// reported only under the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallClusterRule {
    /// A constructor sits above every non-constructor method in its class or `impl`.
    ConstructorAfterMethod,
    /// A private item sits below every public item that reaches it, and an unreached private item
    /// sits below all public items.
    PrivateBeforePublic,
    /// A private item reached from one entrypoint unit sits in that unit's cluster or in the
    /// trailing section (never split between both); a shared item sits in the trailing section.
    UncolocatedHelper,
    /// A private item sits below the private items above it in the abstraction order.
    CalleeBeforeCaller,
}

impl CallClusterRule {
    /// The rule's name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ConstructorAfterMethod => "constructor-after-method",
            Self::PrivateBeforePublic => "private-before-public",
            Self::UncolocatedHelper => "uncolocated-helper",
            Self::CalleeBeforeCaller => "callee-before-caller",
        }
    }
}

/// A misplaced declaration reported by a call-cluster placement rule.
pub struct CallOrderFinding<'a> {
    /// Identifier AST node of the misplaced declaration.
    pub name_node: AstNode<'a>,
    /// Name of the misplaced declaration.
    pub item: String,
    /// Name of the declaration or enclosing type the violated placement refers to.
    pub related: String,
    /// Why `related` sits above `item` in the abstraction order (`callee-before-caller` only).
    pub reason: Option<OrderingReason>,
    /// Sibling declaration next to which `item` belongs.
    pub anchor: PlacementAnchor,
}

/// A [`CallOrderFinding`] cached on its file without lifetime ties to it.
pub(in crate::code_lint::ast) struct CachedCallOrderFinding {
    pub(in crate::code_lint::ast) rule: CallClusterRule,
    pub(in crate::code_lint::ast) span: SourceSpan,
    pub(in crate::code_lint::ast) item: String,
    pub(in crate::code_lint::ast) related: String,
    pub(in crate::code_lint::ast) reason: Option<OrderingReason>,
    pub(in crate::code_lint::ast) anchor: PlacementAnchor,
}

/// A call-cluster finding within one [`DeclarationScope`], by item index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code_lint::ast) struct ScopeFinding {
    pub(in crate::code_lint::ast) rule: CallClusterRule,
    /// The misplaced item.
    pub(in crate::code_lint::ast) item: usize,
    /// The item the violated placement refers to.
    pub(in crate::code_lint::ast) related: usize,
    /// Sibling declaration next to which `item` belongs.
    pub(in crate::code_lint::ast) anchor: PlacementAnchor,
    pub(in crate::code_lint::ast) reason: Option<OrderingReason>,
}

/// Finds the call-cluster findings of one scope from its graph.
pub(in crate::code_lint::ast) fn scope_findings(
    scope: &DeclarationScope<'_>,
    graph: &ScopeGraph,
) -> Vec<ScopeFinding> {
    let placements = generate_placements(&scope.items, graph);
    let source_order: Vec<usize> = (0..scope.items.len()).collect();
    let source = Layout::new(&source_order, &scope.items, graph);
    let held: Vec<bool> = placements
        .iter()
        .map(|placement| source.holds(placement))
        .collect();
    let mut violated: Vec<&Placement> = placements
        .iter()
        .zip(&held)
        .filter(|&(placement, &held)| !held && placement.strength != Strength::Hard)
        .map(|(placement, _)| placement)
        .collect();
    violated.sort_by_key(|placement| {
        (
            placement.subject,
            placement.strength,
            std::cmp::Reverse(source.pos[placement.related]),
        )
    });
    // One finding per item: its first violation, in rule priority, that a move can fix.
    violated
        .chunk_by(|left, right| left.subject == right.subject)
        .filter_map(|subject_violations| {
            subject_violations.iter().find_map(|placement| {
                let Strength::Rule(rule) = placement.strength else {
                    return None;
                };
                Some(ScopeFinding {
                    rule,
                    item: placement.subject,
                    related: placement.related,
                    anchor: best_anchor(placement, &placements, &held, &scope.items, graph)?,
                    reason: placement.reason,
                })
            })
        })
        .collect()
}

/// An order of a scope's items with the regions each item falls in.
struct Layout {
    /// Position of each item.
    pos: Vec<usize>,
    /// Whether each item sits in the cluster of its unit: the contiguous run of the unit's items
    /// that starts at one of its public items.
    in_cluster: Vec<bool>,
    /// Whether each unit has a private item in its cluster, by unit id.
    unit_has_cluster_helper: Vec<bool>,
    /// Position of the last public item, after which the trailing section starts.
    last_public: Option<usize>,
}

impl Layout {
    fn new(order: &[usize], items: &[ScopeItem<'_>], graph: &ScopeGraph) -> Self {
        let count = order.len();
        let mut pos = vec![0; count];
        let mut in_cluster = vec![false; count];
        let mut unit_has_cluster_helper = vec![false; count];
        let mut last_public = None;
        let mut open_unit = None;
        for (position, &idx) in order.iter().enumerate() {
            pos[idx] = position;
            let unit = match graph.ownership(idx) {
                Ownership::Owner(unit) => Some(unit),
                Ownership::Shared | Ownership::Unowned => None,
            };
            if items[idx].visibility == Visibility::Public {
                last_public = Some(position);
                open_unit = unit;
            } else if unit.is_some() && unit == open_unit {
                in_cluster[idx] = true;
                if let Some(unit) = unit {
                    unit_has_cluster_helper[unit] = true;
                }
            } else {
                open_unit = None;
            }
        }
        Self {
            pos,
            in_cluster,
            unit_has_cluster_helper,
            last_public,
        }
    }

    fn holds(&self, placement: &Placement) -> bool {
        let subject = placement.subject;
        match placement.constraint {
            Constraint::Above(other) => self.pos[subject] < self.pos[other],
            Constraint::Below(other) => self.pos[subject] > self.pos[other],
            Constraint::InRegion(unit) => {
                (unit.is_some() && self.in_cluster[subject]) || self.is_trailing(subject)
            }
            Constraint::NotSplit(unit) => {
                self.in_cluster[subject]
                    || !(self.is_trailing(subject) && self.unit_has_cluster_helper[unit])
            }
        }
    }

    fn is_trailing(&self, idx: usize) -> bool {
        self.last_public.is_some_and(|last| self.pos[idx] > last)
    }
}

/// A placement constraint on one item, generated from the scope graph.
#[derive(Debug, Clone, Copy)]
struct Placement {
    subject: usize,
    constraint: Constraint,
    strength: Strength,
    /// The item the constraint refers to, named in diagnostics.
    related: usize,
    reason: Option<OrderingReason>,
}

/// How binding a [`Placement`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Strength {
    /// Required for the code to run (Python import- or class-time evaluation order).
    Hard,
    /// Required by a style rule.
    Rule(CallClusterRule),
}

/// Where `subject` may sit relative to the rest of its scope.
#[derive(Debug, Clone, Copy)]
enum Constraint {
    /// `pos(subject) < pos(other)`.
    Above(usize),
    /// `pos(subject) > pos(other)`.
    Below(usize),
    /// `subject` sits in the cluster of its unit (when `Some`) or in the trailing section.
    InRegion(Option<usize>),
    /// `subject` sits in its unit's cluster, or not in the trailing section while another private
    /// item of its unit sits in the cluster.
    NotSplit(usize),
}

/// Generates the placement constraints of a scope (ADR 011 §3.3 as a table).
fn generate_placements(items: &[ScopeItem<'_>], graph: &ScopeGraph) -> Vec<Placement> {
    let is_private = |idx: usize| items[idx].visibility == Visibility::Private;
    let last_public = (0..items.len()).rev().find(|&idx| !is_private(idx));
    let rule = |subject, constraint, rule, related| Placement {
        subject,
        constraint,
        strength: Strength::Rule(rule),
        related,
        reason: None,
    };
    let mut placements = Vec::new();
    if let Some(first_non_constructor) = items.iter().position(|item| !item.is_constructor) {
        for subject in (0..items.len()).filter(|&idx| items[idx].is_constructor) {
            placements.push(rule(
                subject,
                Constraint::Above(first_non_constructor),
                CallClusterRule::ConstructorAfterMethod,
                first_non_constructor,
            ));
        }
    }
    for subject in (0..items.len()).filter(|&idx| is_private(idx)) {
        if let Some(pin) = items[subject].pinned_before {
            placements.push(Placement {
                subject,
                constraint: Constraint::Above(pin),
                strength: Strength::Hard,
                related: pin,
                reason: None,
            });
        }
        let roots = &graph.roots[subject];
        for &root in roots {
            placements.push(rule(
                subject,
                Constraint::Below(root),
                CallClusterRule::PrivateBeforePublic,
                root,
            ));
        }
        let last_root = roots.last().copied();
        match (graph.ownership(subject), last_root, last_public) {
            (Ownership::Unowned, _, Some(last_public)) => placements.push(rule(
                subject,
                Constraint::Below(last_public),
                CallClusterRule::PrivateBeforePublic,
                last_public,
            )),
            (Ownership::Owner(unit), Some(last_root), _) => {
                placements.push(rule(
                    subject,
                    Constraint::InRegion(Some(unit)),
                    CallClusterRule::UncolocatedHelper,
                    last_root,
                ));
                placements.push(rule(
                    subject,
                    Constraint::NotSplit(unit),
                    CallClusterRule::UncolocatedHelper,
                    unit,
                ));
            }
            (Ownership::Shared, Some(last_root), _) => placements.push(rule(
                subject,
                Constraint::InRegion(None),
                CallClusterRule::UncolocatedHelper,
                last_root,
            )),
            _ => {}
        }
    }
    for edge in &graph.edges {
        if is_private(edge.higher)
            && is_private(edge.lower)
            && graph.private_recursion_group[edge.higher]
                != graph.private_recursion_group[edge.lower]
        {
            placements.push(Placement {
                subject: edge.lower,
                constraint: Constraint::Below(edge.higher),
                strength: Strength::Rule(CallClusterRule::CalleeBeforeCaller),
                related: edge.higher,
                reason: Some(edge.reason),
            });
        }
    }
    placements
}

/// Finds the sibling declaration next to which `placement.subject` can move so that `placement`
/// holds without breaking any stronger placement that held, preferring moves that break the fewest
/// placements, then the shortest move. Returns `None` if no single move satisfies it.
fn best_anchor(
    placement: &Placement,
    placements: &[Placement],
    held: &[bool],
    items: &[ScopeItem<'_>],
    graph: &ScopeGraph,
) -> Option<PlacementAnchor> {
    if placement.strength == Strength::Rule(CallClusterRule::ConstructorAfterMethod) {
        let target = &items[placement.related];
        return (placement.subject > placement.related).then(|| PlacementAnchor {
            name: target.name.clone(),
            line: target.start_line,
        });
    }
    let from = placement.subject;
    (0..items.len())
        .filter(|&after| after != from && after + 1 != from)
        .filter_map(|after| {
            let new_pos = if from < after { after } else { after + 1 };
            let mut order: Vec<usize> = (0..items.len()).filter(|&idx| idx != from).collect();
            order.insert(new_pos, from);
            let moved = Layout::new(&order, items, graph);
            if !moved.holds(placement) {
                return None;
            }
            let mut broken = 0;
            for (other, &other_held) in placements.iter().zip(held) {
                if other_held && !moved.holds(other) {
                    if other.strength < placement.strength {
                        return None;
                    }
                    broken += 1;
                }
            }
            Some(((broken, new_pos.abs_diff(from), new_pos), after))
        })
        .min()
        .map(|(_, after)| PlacementAnchor::after(&items[after].name, items[after].end_line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_lint::ast::{ParsedFile, collect_declaration_scopes};
    use crate::diagnostic::Language;
    use std::cmp::Reverse;
    use std::path::Path;

    /// The layout the scope graph describes: each unit's public items followed by its helpers,
    /// then the shared and unowned items, each group from higher to lower level. A recursion
    /// group that cycles through a public item shares one level, so its private items keep their
    /// private recursion order within it.
    fn graph_layout(items: &[ScopeItem<'_>], graph: &ScopeGraph) -> Vec<usize> {
        let is_public = |idx: usize| items[idx].visibility == Visibility::Public;
        let mut private_lowers = vec![Vec::new(); items.len()];
        for edge in &graph.edges {
            if !is_public(edge.higher) && !is_public(edge.lower) {
                private_lowers[edge.higher].push(edge.lower);
            }
        }
        let private_level = levels(&private_lowers, &graph.private_recursion_group);
        let by_level = |mut group: Vec<usize>| {
            group.sort_by_key(|&idx| (Reverse(graph.level[idx]), Reverse(private_level[idx]), idx));
            group
        };
        let mut placed = vec![false; items.len()];
        let mut order = Vec::new();
        let public_order = (0..items.len())
            .filter(|&idx| items[idx].is_constructor)
            .chain((0..items.len()).filter(|&idx| is_public(idx) && !items[idx].is_constructor));
        for first_public in public_order {
            if placed[first_public] {
                continue;
            }
            let unit = graph.ownership(first_public);
            let publics =
                (0..items.len()).filter(|&idx| is_public(idx) && graph.ownership(idx) == unit);
            let helpers: Vec<usize> = (0..items.len())
                .filter(|&idx| !is_public(idx) && graph.ownership(idx) == unit)
                .collect();
            // A helper below a shared or unowned private item follows it into the trailing
            // section, and the unit's other helpers follow so the unit is not split.
            let below_trailing_item = graph.edges.iter().any(|edge| {
                helpers.contains(&edge.lower)
                    && !is_public(edge.higher)
                    && graph.ownership(edge.higher) != unit
            });
            let cluster = if below_trailing_item {
                Vec::new()
            } else {
                by_level(helpers)
            };
            for idx in publics.chain(cluster) {
                placed[idx] = true;
                order.push(idx);
            }
        }
        order.extend(by_level(
            (0..items.len()).filter(|&idx| !placed[idx]).collect(),
        ));
        order
    }

    fn source_files(directory: &Path, extension: &str, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("directory should be readable") {
            let path = entry.expect("entry should be readable").path();
            if path.is_dir() {
                source_files(&path, extension, out);
            } else if path.extension().is_some_and(|found| found == extension) {
                out.push(path);
            }
        }
    }

    /// The style placements of `path`'s scopes that the layout its graph describes violates.
    fn placements_violated_by_graph_layout(path: &Path) -> Vec<String> {
        let lang = if path.extension().is_some_and(|found| found == "rs") {
            Language::Rust
        } else {
            Language::Python
        };
        let source = std::fs::read_to_string(path).expect("source should be readable");
        let file = ParsedFile::new(&source, lang);
        collect_declaration_scopes(&file)
            .iter()
            .flat_map(|scope| {
                let graph = ScopeGraph::new(&scope.items);
                let layout = Layout::new(&graph_layout(&scope.items, &graph), &scope.items, &graph);
                generate_placements(&scope.items, &graph)
                    .into_iter()
                    .filter(|placement| {
                        placement.strength != Strength::Hard && !layout.holds(placement)
                    })
                    .map(|placement| {
                        format!(
                            "{}: `{}` {:?} `{}`",
                            path.display(),
                            scope.items[placement.subject].name,
                            placement.constraint,
                            scope.items[placement.related].name,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Every style placement the rules generate holds in the layout the graph describes, so
    /// the rules never ask for an order the graph cannot draw (ADR 011 §3.3).
    #[test]
    fn graph_layout_satisfies_every_rule_placement() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut paths = Vec::new();
        source_files(&root.join("src"), "rs", &mut paths);
        source_files(&root.join("scripts"), "py", &mut paths);

        let violations: Vec<String> = paths
            .iter()
            .flat_map(|path| placements_violated_by_graph_layout(path))
            .collect();

        assert_eq!(violations, Vec::<String>::new());
    }
}
