//! Abstraction graph of one declaration scope (ADR 011): typed ordering edges, public-root
//! ownership, recursion groups, and levels.

use super::{ScopeItem, ScopeItemKind, Visibility};
use std::collections::BTreeMap;

/// A syntactic reference from one declaration in a scope to a sibling declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationFact {
    /// Index of the declaration that contains the reference.
    pub source: usize,
    /// Index of the referenced sibling declaration.
    pub target: usize,
    /// Where in `source` the reference appears.
    pub kind: RelationKind,
}

/// Which syntactic region of a declaration contains a reference to a sibling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationKind {
    /// A callable's parameter type annotation.
    Parameter,
    /// A callable's return type annotation.
    Return,
    /// A callable's body expression or statement.
    Body,
    /// A type's own fields, variants, base classes, or target type.
    Composition,
    /// A type's methods or `impl` blocks.
    Behavior,
}

/// Why an ordering edge places its higher item above its lower item (ADR 011 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderingReason {
    /// `Callable -> Callable`: the higher callable calls the lower one.
    Call,
    /// `Type -> Type`: the higher type's own fields, variants, or bases contain the lower one.
    Containment,
    /// `Type -> Callable` or `Type -> Type`: the higher type's methods or `impl` blocks use the
    /// lower item.
    ImplBehavior,
    /// `Callable -> Type`: the higher callable uses the lower type only inside its body.
    Encapsulation,
    /// `Type -> Callable`: the lower callable names the higher type in its parameter or return
    /// types, so the type is the vocabulary of its contract.
    SignatureContract,
}

/// A downward abstraction edge: `items[higher]` reads before `items[lower]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderingEdge {
    /// Index of the item that reads first (caller, composite type, or contract type).
    pub higher: usize,
    /// Index of the item that reads after it (callee, component, or encapsulated detail).
    pub lower: usize,
    /// Why `higher` sits above `lower`.
    pub reason: OrderingReason,
}

/// Which public entrypoint unit an item belongs to.
///
/// A unit is a public item, except that all constructors of a scope form one unit identified by
/// the first constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::code_lint::ast) enum Ownership {
    /// A public item of unit `unit`, or a private item reached only from that unit.
    Owner(usize),
    /// A private item reached from two or more units.
    Shared,
    /// A private item reached from no public item.
    Unowned,
}

/// The abstraction graph of one declaration scope, indexed like its items.
pub(in crate::code_lint::ast) struct ScopeGraph {
    /// Downward ordering edges, sorted.
    pub(in crate::code_lint::ast) edges: Vec<OrderingEdge>,
    /// `Roots(f)`: sorted public items that reach `f` through private references only.
    pub(in crate::code_lint::ast) roots: Vec<Vec<usize>>,
    /// Sorted public entrypoint unit indices of each item (`Roots(f)` with constructors collapsed
    /// to the first constructor).
    pub(in crate::code_lint::ast) unit_owners: Vec<Vec<usize>>,
    /// Strongly connected component of each item over `edges`.
    pub(in crate::code_lint::ast) recursion_group: Vec<usize>,
    /// Strongly connected component of each item over the `edges` between private items: items
    /// that recurse into each other without going through a public entrypoint.
    pub(in crate::code_lint::ast) private_recursion_group: Vec<usize>,
    /// Level of each item over `edges`: `0` for leaves, one above the highest group below.
    pub(in crate::code_lint::ast) level: Vec<usize>,
}

impl ScopeGraph {
    /// Builds the abstraction graph of a scope's items.
    pub(in crate::code_lint::ast) fn new(items: &[ScopeItem<'_>]) -> Self {
        let edges = ordering_edges(items);
        let is_private = |idx: usize| items[idx].visibility == Visibility::Private;
        let mut lowers = vec![Vec::new(); items.len()];
        let mut private_lowers = vec![Vec::new(); items.len()];
        for edge in &edges {
            lowers[edge.higher].push(edge.lower);
            if is_private(edge.higher) && is_private(edge.lower) {
                private_lowers[edge.higher].push(edge.lower);
            }
        }
        let recursion_group = strongly_connected_components(&lowers);
        let private_recursion_group = strongly_connected_components(&private_lowers);
        let level = levels(&lowers, &recursion_group);
        let roots = public_roots(items);
        let unit_owners = unit_owners(items, &roots);
        Self {
            edges,
            roots,
            unit_owners,
            recursion_group,
            private_recursion_group,
            level,
        }
    }

    /// Returns the [`Ownership`] of item `idx`, derived from [`Self::unit_owners`].
    pub(in crate::code_lint::ast) fn ownership(&self, idx: usize) -> Ownership {
        match self.unit_owners[idx].as_slice() {
            [] => Ownership::Unowned,
            [unit] => Ownership::Owner(*unit),
            _ => Ownership::Shared,
        }
    }
}

/// Computes strongly connected component ids over `successors` with Tarjan's algorithm.
#[must_use]
pub fn strongly_connected_components(successors: &[Vec<usize>]) -> Vec<usize> {
    struct Tarjan<'a> {
        successors: &'a [Vec<usize>],
        next_index: usize,
        index: Vec<Option<usize>>,
        low_link: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        component: Vec<usize>,
        next_component: usize,
    }

    impl Tarjan<'_> {
        fn visit(&mut self, node: usize) {
            self.index[node] = Some(self.next_index);
            self.low_link[node] = self.next_index;
            self.next_index += 1;
            self.stack.push(node);
            self.on_stack[node] = true;
            for &next in &self.successors[node] {
                match self.index[next] {
                    None => {
                        self.visit(next);
                        self.low_link[node] = self.low_link[node].min(self.low_link[next]);
                    }
                    Some(next_index) if self.on_stack[next] => {
                        self.low_link[node] = self.low_link[node].min(next_index);
                    }
                    Some(_) => {}
                }
            }
            if Some(self.low_link[node]) == self.index[node] {
                while let Some(member) = self.stack.pop() {
                    self.on_stack[member] = false;
                    self.component[member] = self.next_component;
                    if member == node {
                        break;
                    }
                }
                self.next_component += 1;
            }
        }
    }

    let count = successors.len();
    let mut tarjan = Tarjan {
        successors,
        next_index: 0,
        index: vec![None; count],
        low_link: vec![0; count],
        on_stack: vec![false; count],
        stack: Vec::new(),
        component: vec![0; count],
        next_component: 0,
    };
    for node in 0..count {
        if tarjan.index[node].is_none() {
            tarjan.visit(node);
        }
    }
    tarjan.component
}

/// Levelizes `successors`: each component of `component` is one unit, at level `0` when nothing
/// is below it, otherwise one above the highest component below it.
#[must_use]
pub fn levels(successors: &[Vec<usize>], component: &[usize]) -> Vec<usize> {
    let component_count = component.iter().copied().max().map_or(0, |max| max + 1);
    let mut component_level = vec![0; component_count];
    // Tarjan numbers components in reverse topological order: every component below another
    // has a smaller id, so one pass in id order sees lower components first.
    let mut members = vec![Vec::new(); component_count];
    for (node, &id) in component.iter().enumerate() {
        members[id].push(node);
    }
    for id in 0..component_count {
        component_level[id] = members[id]
            .iter()
            .flat_map(|&node| &successors[node])
            .filter(|&&lower| component[lower] != id)
            .map(|&lower| component_level[component[lower]] + 1)
            .max()
            .unwrap_or(0);
    }
    component.iter().map(|&id| component_level[id]).collect()
}

/// Labels every downward abstraction edge between `items` with its ADR 011 §3.4 reason.
fn ordering_edges(items: &[ScopeItem<'_>]) -> Vec<OrderingEdge> {
    let is_type = |idx: usize| items[idx].kind == ScopeItemKind::Type;
    let mut reasons = BTreeMap::new();
    for (higher, item) in items.iter().enumerate() {
        match item.kind {
            ScopeItemKind::Type => {
                for &(lower, _) in &item.references {
                    let reason =
                        if is_type(lower) && item.has_reference(lower, RelationKind::Composition) {
                            OrderingReason::Containment
                        } else {
                            OrderingReason::ImplBehavior
                        };
                    reasons.entry((higher, lower)).or_insert(reason);
                }
            }
            ScopeItemKind::Callable => {
                for &(lower, _) in &item.references {
                    if !is_type(lower) {
                        reasons
                            .entry((higher, lower))
                            .or_insert(OrderingReason::Call);
                    }
                }
                for lower in encapsulated_types(items, item) {
                    reasons
                        .entry((higher, lower))
                        .or_insert(OrderingReason::Encapsulation);
                }
            }
        }
    }
    for (callable, item) in items.iter().enumerate() {
        if item.kind == ScopeItemKind::Callable {
            for signature_type in
                item.references_in(&[RelationKind::Parameter, RelationKind::Return])
            {
                if is_type(signature_type) {
                    reasons.insert(
                        (signature_type, callable),
                        OrderingReason::SignatureContract,
                    );
                }
            }
        }
    }
    reasons
        .into_iter()
        .filter(|&((higher, lower), _)| higher != lower)
        .map(|((higher, lower), reason)| OrderingEdge {
            higher,
            lower,
            reason,
        })
        .collect()
}

/// Types that `callable` uses in its body (directly or through the return type of a sibling it
/// calls) without exposing them through its signature or the fields of its signature types.
fn encapsulated_types(items: &[ScopeItem<'_>], callable: &ScopeItem<'_>) -> Vec<usize> {
    let is_type = |idx: usize| items[idx].kind == ScopeItemKind::Type;
    let mut signature = vec![false; items.len()];
    let mut stack: Vec<usize> = callable
        .references_in(&[RelationKind::Parameter, RelationKind::Return])
        .filter(|&idx| is_type(idx))
        .collect();
    while let Some(current) = stack.pop() {
        if !std::mem::replace(&mut signature[current], true) {
            stack.extend(
                items[current]
                    .references
                    .iter()
                    .map(|&(target, _)| target)
                    .filter(|&idx| is_type(idx)),
            );
        }
    }
    (0..items.len())
        .filter(|&candidate| is_type(candidate) && !signature[candidate])
        .filter(|&candidate| {
            callable.has_reference(candidate, RelationKind::Body)
                || callable.references_in(&[RelationKind::Body]).any(|callee| {
                    !is_type(callee) && items[callee].has_reference(candidate, RelationKind::Return)
                })
        })
        .collect()
}

/// Computes `Roots(f)` for every item: the sorted public items that reach it through private
/// references only.
fn public_roots(items: &[ScopeItem<'_>]) -> Vec<Vec<usize>> {
    let is_public = |idx: usize| items[idx].visibility == Visibility::Public;
    let mut callers = vec![Vec::new(); items.len()];
    for (caller, item) in items.iter().enumerate() {
        for &(callee, _) in &item.references {
            if callee != caller && !callers[callee].contains(&caller) {
                callers[callee].push(caller);
            }
        }
    }
    (0..items.len())
        .map(|idx| {
            if is_public(idx) {
                return vec![idx];
            }
            let mut visited = vec![false; items.len()];
            let mut stack = vec![idx];
            let mut roots = Vec::new();
            while let Some(current) = stack.pop() {
                if std::mem::replace(&mut visited[current], true) {
                    continue;
                }
                for &caller in &callers[current] {
                    if is_public(caller) {
                        roots.push(caller);
                    } else {
                        stack.push(caller);
                    }
                }
            }
            roots.sort_unstable();
            roots.dedup();
            roots
        })
        .collect()
}

/// Maps each item's roots to their sorted entrypoint unit indices.
fn unit_owners(items: &[ScopeItem<'_>], roots: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let first_constructor = items.iter().position(|item| item.is_constructor);
    let unit_of = |root: usize| {
        if items[root].is_constructor {
            first_constructor.unwrap_or(root)
        } else {
            root
        }
    };
    roots
        .iter()
        .map(|item_roots| {
            let mut units: Vec<usize> = item_roots.iter().map(|&root| unit_of(root)).collect();
            units.sort_unstable();
            units.dedup();
            units
        })
        .collect()
}
