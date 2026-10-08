# Phase 3: Design Plan — Declaration Ordering Rule Suite

This document records **Phase 3 (Design Plan)** for the **7-rule Declaration Ordering suite** (`Topic::DECLARATION_ORDER`) in [ROADMAP.md](../../../ROADMAP.md), implementing [decisions/011_colocated_abstraction_ordering.md](../../../decisions/011_colocated_abstraction_ordering.md) and building on [01_understand.md](01_understand.md) and [02_references.md](02_references.md).

> Status: **VALIDATED — Ready for Phase 4 Implementation** (2026-10-08).

---

## 1. Summary of Validated Decisions (`D1`–`D7`)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **`D1`** | **Model intra-file function and method ordering on ADR 006's Architectural Isolation & Layering Principles ([decisions/011_colocated_abstraction_ordering.md](../../../decisions/011_colocated_abstraction_ordering.md))** | Each public entrypoint `p` and its exclusive private helpers (`Roots(h) = {p}`) form an in-file **component unit** that stays contiguous. Private helpers shared across multiple public entrypoints (`\|Roots(h)\| >= 2`) belong to a lower level of abstraction and form the **shared helper layer** at the end of the scope. |
| **`D2`** | **Replace `private-before-public-method` and `call-before-definition` with 3 precedence-linked rules (`uncolocated-helper`, `private-before-public-function`, `callee-before-caller`) sharing one call-cluster engine** | The three rules evaluate the same per-scope call graph (`Roots(h)`, SCCs, and declaration order) across both module-level functions and class/inherent-`impl` methods in Python and Rust, with strict priority `P1 -> P2 -> P3` so a single misplaced helper is never double-reported. |
| **`D3`** | **Reverse intra-unit call ordering from bottom-up (`call-before-definition`) to top-down (`callee-before-caller`)** | A helper is a lower-abstraction building block than its caller. Within each component unit (`pub -> pub`, exclusive `priv -> priv` of the same owner) and within the shared helper layer (`shared priv -> shared priv`), callers precede callees so abstraction decreases monotonically top-to-bottom. |
| **`D4`** | **Enforce 2-tier visibility in `private-before-public-function`** | Exported (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)`) vs. Private (bare `fn`) in Rust; Public & dunder (`name`, `__dunder__`) vs. Private (`_name`, `__mangled`) in Python. In modules without explicit `pub fn` items (such as Rust rule modules exposing `pub const RULE` or trait `impl`s, or Python scripts), private functions referenced outside top-level function bodies or having zero in-scope callers act as the top-level entrypoints. |
| **`D5`** | **Include the 4 structural envelope rules (`field-after-method`, `associated-item-after-method`, `constructor-after-method`, `statement-after-main-guard`)** | Enforces the top-of-scope data/constructor headers and bottom-of-scope `__main__` execution footer around the function/method body. |
| **`D6`** | **Add `{caller}` to the shared placeholder vocabulary in [naming_and_message_style_guide.md](../naming_and_message_style_guide.md) and `tests/registry.rs`** | Allows `uncolocated-helper`, `private-before-public-function`, and `callee-before-caller` to anchor on the misplaced function definition `{function}` while naming the exact `{caller}` entrypoint or caller it must move below. |
| **`D7`** | **Dogfood all 7 rules across Omni's own `src/` with zero suppressions** | Reorders misplaced helpers in Omni `src/` (including `SuppressionTracker::parse_comment_text`, `InterceptedCommand::parse_single`, and bottom-up helper clusters in `src/code_lint/ast/` and `src/diff.rs`) so `test_self_dogfooding_code_lint` passes cleanly. |

---

## 2. Shared Call-Cluster & Abstraction-Order Engine (`src/code_lint/ast.rs`)

### 2.1 Core Data Structures & Definitions

For every module scope (Python `ModModule`, Rust `SourceFile` / inline `ast::Module`), Python `StmtClassDef`, and Rust inherent `ast::Impl`:

1. **Ordered Callables (`V = [f_0, f_1, ..., f_{k-1}]`)**:
   - Each callable records:
     - `name: String`
     - `name_node: AstNode<'a>` (diagnostic anchor)
     - `visibility: MethodVisibility` (`Public` vs. `Private`)
     - `is_constructor: bool` (Tier 0 constructor methods in classes/inherent `impl`s)
     - `callees: Vec<usize>` (indices in `V` of sibling callables directly called by `f_i`)
     - ` referenced_externally: bool` (true if `f_i` is referenced from module-level non-function items such as `pub const RULE`, `static`, `impl` blocks, or module-level statements/`__main__` guards)
   - **Effective Entrypoints (`is_entrypoint`)**:
     - If the scope contains at least one `MethodVisibility::Public` callable, the entrypoints `P` are all `MethodVisibility::Public` callables (except in a module scope where a private function is referenced from a module-level exported item like `pub const RULE = CodeRule { check_file, ... }` and has no `Public` caller, or when the module has zero `Public` functions, in which case `referenced_externally` functions or zero-indegree roots serve as entrypoints for intra-module `callee-before-caller`).
     - Specifically:
       - `Public` / `Private` visibility for **Priority 1 (`uncolocated-helper`)** and **Priority 2 (`private-before-public-function`)** uses syntactic visibility (`MethodVisibility::Public` vs. `MethodVisibility::Private`). If a scope has zero `MethodVisibility::Public` callables, P1 and P2 produce no findings in that scope.
       - For **Priority 3 (`callee-before-caller`)**, within a scope where all callables are `Private`, all callables belong to the same visibility tier and same unrooted group (`Roots(h) = empty`), so `callee-before-caller` still checks every non-SCC `caller -> callee` edge!

2. **Private-Subgraph Root Ownership (`Roots(h)`)**:
   - For each `p in P` (indices of `MethodVisibility::Public` callables in `V`), perform a DFS/BFS starting from the direct `MethodVisibility::Private` callees of `p` and traversing **only** `Private -> Private` edges (`u -> v` where both `u` and `v` are `Private`).
   - Add `p` to `Roots(h)` for every private callable `h` visited.
   - Because traversal stops at `Public` boundaries, if `pub_b` calls `pub_a` and `pub_a` calls `_a1`, `Roots(_a1) = {pub_a}` (exclusive to `pub_a`).
   - Classification of each private callable `h`:
     - **Exclusive Helper**: `Roots(h) == [p]` (owned by a single public entrypoint `p`).
     - **Shared Helper**: `Roots(h).len() >= 2` (reached by 2 or more public entrypoints).
     - **Unrooted Private Callable**: `Roots(h).is_empty()` (not reachable from any `Public` callable in the scope).

3. **Strongly Connected Components (`scc_id`)**:
   - Tarjan's algorithm over `(V, E)` assigns each callable `f_i` an `scc_id`. Any call edge `u -> v` with `scc_id[u] == scc_id[v]` (self-recursion or mutual recursion) is exempt from `callee-before-caller`.

4. **Priority 1 — `uncolocated-helper` Findings**:
   - For each private callable `h` with **exclusive** ownership `Roots(h) == [p]`:
     - If `h > p` (i.e., `h` is declared after `p`, since `h < p` is handled by Priority 2) and there exists any callable `m` with `p < m < h` that does **not** belong to `Cluster(p)` (meaning `m != p` and `Roots(m) != [p]`):
       - Flag `h` with `caller = V[p].name` and mark `h` as `flagged_p1_or_p2 = true`.

5. **Priority 2 — `private-before-public-function` Findings**:
   - For each private callable `h` not flagged by Priority 1:
     - **Exclusive Helper (`Roots(h) == [p]`)**: if `h < p`, flag `h` with `caller = V[p].name` and mark `flagged_p1_or_p2 = true`.
     - **Shared Helper (`Roots(h).len() >= 2`)**: let `last_pub_caller = *Roots(h).iter().max().unwrap()`. If `h < last_pub_caller`, flag `h` with `caller = V[last_pub_caller].name` and mark `flagged_p1_or_p2 = true`.
     - **Unrooted Private Callable (`Roots(h).is_empty()`)**: if the scope has at least one `Public` callable, let `last_pub = Public.iter().max().unwrap()`. If `h < last_pub`, flag `h` with `caller = V[last_pub].name` and mark `flagged_p1_or_p2 = true`.

6. **Priority 3 — `callee-before-caller` Findings**:
   - For each callable `callee` not flagged by Priority 1 or Priority 2 (`!flagged_p1_or_p2[callee]` and `!V[callee].is_constructor`):
     - Find all direct callers `caller` such that:
       - `caller > callee` (`callee` is declared before `caller`),
       - `!flagged_p1_or_p2[caller]` and `!V[caller].is_constructor`,
       - `scc_id[caller] != scc_id[callee]` (not mutually recursive),
       - `V[caller].visibility == V[callee].visibility` (same visibility tier),
       - If `Private`: `same_private_group(Roots(caller), Roots(callee))` holds, meaning either:
         - both are exclusive to the same public entrypoint (`Roots(caller) == [p] && Roots(callee) == [p]`),
         - both are shared helpers (`Roots(caller).len() >= 2 && Roots(callee).len() >= 2`), or
         - both are unrooted (`Roots(caller).is_empty() && Roots(callee).is_empty()`).
     - If at least one such `caller` exists, let `last_caller = callers.max()` and flag `callee` once with `caller = V[last_caller].name`.

### 2.2 Scope Extraction Details for Python and Rust

- **Python (`src/code_lint/ast/python/classes.rs`)**:
  - Extracts scopes for:
    1. `ModModule.body` (module-level functions),
    2. Each `StmtClassDef.body` (class methods, recursively visiting nested classes as independent scopes).
  - Groups `@overload` stubs and `@<prop>.getter` / `@<prop>.setter` / `@<prop>.deleter` accessors into a single `CallableItem` at the primary definition's position, merging calls made across all grouped bodies.
  - Collects intra-scope calls inside each callable body (without descending into nested `def`, `class`, or `lambda` scopes for local assignments, while walking expressions inside the callable):
    - In a **module scope**: bare `Expr::Call` with `func = Expr::Name(id)` where `id` is a sibling module-level function name and `id` is **not** bound as a parameter or local variable (`Assign`, `AnnAssign`, `AugAssign`, `For`, `With`, `NamedExpr`, `Import`, `ImportFrom`, `ExceptHandler`) in that function.
    - In a **class scope**: `Expr::Call` with `func = Expr::Attribute { value: Expr::Name("self" | "cls" | <ClassName>), attr }` where `attr` is a sibling method name on the class (filtering out `self` if shadowed).

- **Rust (`src/code_lint/ast/rust.rs`)**:
  - Extracts scopes for:
    1. `ast::SourceFile` top-level `ast::Item::Fn` items and non-test inline `ast::Module` item lists, excluding any item inside `collect_inline_test_ranges` (`#[cfg(test)]` / `#[test]`).
    2. Each inherent `ast::Impl` block (`impl_item.trait_().is_none()`), excluding `#[cfg(test)]` / `#[test]` items.
  - Collects intra-scope calls inside each `ast::Fn` body (without descending into nested `ast::Item` definitions such as inner `fn` or `impl` blocks, while descending into closures and blocks):
    - In a **module scope**:
      - `ast::CallExpr` whose callee is a single-segment `ast::PathExpr` (`foo(...)`) matching a sibling `fn` in the same module (not shadowed by a parameter or `let` binding in that `fn`), AND
      - Single-segment `ast::PathExpr` references passed as function pointers (`check_file` in `CodeRule { check_file }` or `.map(helper)`) matching a sibling `fn` in the same module!
    - In an **inherent `impl` scope**:
      - `ast::MethodCallExpr` where receiver is `self` (`self.helper(...)`) matching a sibling method in the `impl` block,
      - `ast::CallExpr` where callee path is `Self::helper(...)` or `<TypeName>::helper(...)` matching a sibling method in the `impl` block, and
      - `Self::helper` / `<TypeName>::helper` path references passed as function values (`Option::map(Self::helper)`).

---

## 3. Per-Rule Specifications (The 7-Rule Suite)

### 3.1 `field-after-method` (Python) — Already Implemented
- **File**: [src/code_lint/rules/field_after_method.rs](../../../src/code_lint/rules/field_after_method.rs)
- **Summary**: `"Attribute `{name}` of `{class}` is declared after a method definition."`

### 3.2 `associated-item-after-method` (Rust) — Already Implemented
- **File**: [src/code_lint/rules/associated_item_after_method.rs](../../../src/code_lint/rules/associated_item_after_method.rs)
- **Summary**: `"Associated item `{name}` of `{class}` is declared after a `fn` item."`

### 3.3 `constructor-after-method` (Python, Rust) — Already Implemented
- **File**: [src/code_lint/rules/constructor_after_method.rs](../../../src/code_lint/rules/constructor_after_method.rs)
- **Summary**: `"Constructor `{function}` of `{class}` is defined after a non-constructor method."`

### 3.4 `uncolocated-helper` (Python, Rust) — Priority 1
- **File**: `src/code_lint/rules/uncolocated_helper.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Opinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Exclusive helper `{function}` is separated from its owning public entrypoint `{caller}` by an unrelated function."`
  - `rationale`: `"Splitting `{function}` away from `{caller}` fractures the component unit of `{caller}` across the scope and forces readers to jump over unrelated definitions."`
  - `suggestion`: `"Move `{function}` into the contiguous helper cluster immediately below `{caller}`."`
- **`RuleDoc`**:
  - `summary`: `"Flags exclusive private helpers separated from their owning public function or method by unrelated definitions."`
- **Diagnostic Anchor**: `&finding.name_node` with placeholders `[("function", &finding.function), ("caller", &finding.caller)]`.
- **Named Exemptions**:
  - `E1` (**Shared private helpers `|Roots(h)| >= 2`**): A private helper reachable from two or more public entrypoints belongs to the trailing shared helper layer, not to any single public entrypoint's contiguous cluster.
  - `E2` (**Transitive exclusive helper chains `p -> _h1 -> _h2`**): When `p` calls `_h1` and `_h1` calls `_h2`, both `_h1` and `_h2` have `Roots = {p}`, so `[p, _h1, _h2]` is a contiguous cluster and `_h1` does not separate `_h2` from `p`.
  - `E3` (**Public-to-public calls `pub_b -> pub_a -> _a1`**): Root propagation stops at public boundaries, so `_a1` has `Roots(_a1) = {pub_a}` (exclusive to `pub_a`, not shared with `pub_b`).
  - `E4` (**Uncalled private functions `Roots(h) = empty`**): Private functions not reachable from any public entrypoint have no owning public entrypoint and are not flagged by `uncolocated-helper`.
  - `E5` (**Helper placed above its owning public entrypoint `pos(h) < pos(p)`**): Handled by Priority 2 (`private-before-public-function`), never double-reported by `uncolocated-helper`.
  - `E6` (**Rust trait `impl` blocks and `#[cfg(test)]` / `#[test]` items**): Trait `impl` blocks and inline test modules/functions are excluded.

### 3.5 `private-before-public-function` (Python, Rust) — Priority 2
- **File**: `src/code_lint/rules/private_before_public_function.rs` (replaces `private_before_public_method.rs`)
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Opinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Private helper `{function}` is defined before public entrypoint `{caller}`."`
  - `rationale`: `"Placing lower-abstraction private helpers above `{caller}` buries the public contract of the scope behind implementation details."`
  - `suggestion`: `"Move `{function}` below `{caller}` into its owning component unit or the trailing shared helper layer."`
- **`RuleDoc`**:
  - `summary`: `"Flags private functions and methods defined above their owning or calling public entrypoints."`
- **Diagnostic Anchor**: `&finding.name_node` with placeholders `[("function", &finding.function), ("caller", &finding.caller)]`.
- **Named Exemptions**:
  - `E1` (**Colocated exclusive helper `[pub_a, _a1, pub_b, _b1]`**): When `Roots(_a1) = {pub_a}` and `_a1` appears below `pub_a` (`pos(_a1) > pos(pub_a)`), `_a1` is **not** flagged even though `pub_b` appears below `_a1`.
  - `E2` (**Shared helper placed below all of its public callers `[pub_a, pub_b, _shared, pub_c]`**): When `Roots(_shared) = {pub_a, pub_b}` and `pos(_shared) > max(pos(pub_a), pos(pub_b))`, `_shared` is below all of its public callers and is not flagged.
  - `E3` (**Python dunder methods `__name__` and Rust restricted visibility `pub(crate)` / `pub(super)`**): Treated as `MethodVisibility::Public`.
  - `E4` (**Python `@overload` and `@property` getter/setter/deleter grouping**): Grouped at the primary definition's position.
  - `E5` (**Scopes with only private functions**): Modules or classes with zero `Public` functions have no public-before-private boundary to violate.
  - `E6` (**Rust trait `impl` blocks and `#[cfg(test)]` / `#[test]` items**): Excluded.

### 3.6 `callee-before-caller` (Python, Rust) — Priority 3
- **File**: `src/code_lint/rules/callee_before_caller.rs`
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Opinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Callee `{function}` is defined before its caller `{caller}` within the same abstraction tier."`
  - `rationale`: `"Defining lower-abstraction callees above `{caller}` inverts the top-down reading flow inside the component unit or shared helper layer."`
  - `suggestion`: `"Move `{function}` below `{caller}` so higher-level callers precede lower-level callees."`
- **`RuleDoc`**:
  - `summary`: `"Flags functions and methods defined before their callers within the same visibility tier and component cluster."`
- **Diagnostic Anchor**: `&finding.name_node` with placeholders `[("function", &finding.function), ("caller", &finding.caller)]`.
- **Named Exemptions**:
  - `E1` (**Cross-visibility calls `pub -> priv` or `priv -> pub`**): Governed by `private-before-public-function` (Priority 2), never checked by `callee-before-caller`.
  - `E2` (**Cross-cluster calls `exclusive_priv -> shared_priv`**): An exclusive helper (`|Roots| = 1`) is at a higher abstraction layer than a shared helper (`|Roots| >= 2`); calls between different private groups are not same-cluster edges.
  - `E3` (**Precedence suppression (`P1` / `P2` already flagged)**): Any callable already flagged by `uncolocated-helper` or `private-before-public-function` is skipped by `callee-before-caller` so a single misplaced helper is never double-reported.
  - `E4` (**Self-recursion and mutual recursion (Strongly Connected Components)**): Call edges within the same Tarjan SCC (`f <-> g`) are exempt.
  - `E5` (**Constructors (`__init__`, `pub fn new`)**): Constructors belong to Tier 0 at the top of a class/`impl` (`constructor-after-method`), so calls between a public method and a constructor (`reset() -> __init__()` or `with_capacity() -> new()`) are exempt.
  - `E6` (**Local variable / parameter shadowing**): Calls to a local variable or parameter that shadows a sibling function name (`check = ...; check()`) do not create a call edge to the sibling function.
  - `E7` (**Rust trait `impl` blocks and `#[cfg(test)]` / `#[test]` items**): Excluded.

### 3.7 `statement-after-main-guard` (Python) — Already Implemented
- **File**: [src/code_lint/rules/statement_after_main_guard.rs](../../../src/code_lint/rules/statement_after_main_guard.rs)
- **Summary**: `"Top-level statement appears after the `if __name__ == \"__main__\":` guard."`
