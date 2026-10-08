# Phase 3: Design Plan — Declaration Ordering Rule Suite

This document records **Phase 3 (Design Plan)** for the **7-rule Declaration Ordering suite** (`Topic::DECLARATION_ORDER`) in [ROADMAP.md](../../../ROADMAP.md), implementing [decisions/011_colocated_abstraction_ordering.md](../../../decisions/011_colocated_abstraction_ordering.md) and building on [01_understand.md](01_understand.md) and [02_references.md](02_references.md).

> Status: **VALIDATED — Ready for Phase 4 Implementation** (2026-10-08).

---

## 1. Summary of Validated Decisions (`D1`–`D7`)

| ID | Decision | Rationale |
| :--- | :--- | :--- |
| **`D1`** | **Model intra-file function and method ordering on ADR 006's Architectural Isolation & Layering Principles ([decisions/011_colocated_abstraction_ordering.md](../../../decisions/011_colocated_abstraction_ordering.md))** | Once placed after its public callers, a single-user private helper (`Roots(h) = {p}`) may sit either in **Place 1** (immediately after `p` in its contiguous cluster) or in **Place 2** (in the trailing private helper section at the end of the scope, provided `p`'s helpers are not split between both places). Private helpers shared across multiple public entrypoints (`\|Roots(h)\| >= 2`) must sit in **Place 2** at the end of the scope. |
| **`D2`** | **Replace `private-before-public-method` and `call-before-definition` with 3 mutually exclusive rules (`private-before-public-function`, `uncolocated-helper`, `callee-before-caller`) sharing one call-cluster engine** | The three rules evaluate the same per-scope call graph (`Roots(h)`, SCCs, and declaration order) across both module-level functions and class/inherent-`impl` methods in Python and Rust in three disjoint stages so a single misplaced helper is never double-reported. |
| **`D3`** | **Reverse private-helper call ordering from bottom-up (`call-before-definition`) to top-down (`callee-before-caller`)** | A private helper is a lower-abstraction building block than its caller. Among private helpers (`Private -> Private`), callers precede callees so abstraction decreases monotonically top-to-bottom (`_exclusive -> _shared`), while `Public -> Public` peer entrypoints remain unconstrained. |
| **`D4`** | **Enforce 2-tier visibility in `private-before-public-function`** | Exported (`pub`, `pub(crate)`, `pub(super)`, `pub(in ...)`, `fn main`) vs. Private (bare `fn`) in Rust; Public & dunder (`name`, `__dunder__`) vs. Private (`_name`, `__mangled`) in Python. |
| **`D5`** | **Include the 4 structural envelope rules (`field-after-method`, `associated-item-after-method`, `constructor-after-method`, `statement-after-main-guard`)** | Enforces the top-of-scope data/constructor headers and bottom-of-scope `__main__` execution footer around the function/method body. |
| **`D6`** | **Add `{caller}` to the shared placeholder vocabulary in [naming_and_message_style_guide.md](../naming_and_message_style_guide.md) and `tests/registry.rs`** | Allows `uncolocated-helper`, `private-before-public-function`, and `callee-before-caller` to anchor on the misplaced function definition `{function}` while naming the exact `{caller}` entrypoint or caller it relates to. |
| **`D7`** | **Dogfood all 7 rules across Omni's own `src/` with zero suppressions** | Supports both public-first scopes (`InterceptedCommand`, `SuppressionTracker`, `CommentIndex`, `test_utils.rs`, `scopes.rs`) and vertical-slice modules (`ast.rs`, `config.rs`) while reordering misplaced helpers in Omni `src/` (`read_config_file` in `rule_selection.rs` and bottom-up helper clusters) so `test_self_dogfooding_code_lint` passes cleanly. |

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
   - **Visibility Tiers**:
     - `Public` / `Private` visibility for all three rules uses syntactic visibility (`MethodVisibility::Public` vs. `MethodVisibility::Private`). If a scope has zero `MethodVisibility::Public` callables, `private-before-public-function` and `uncolocated-helper` produce no findings in that scope, while `callee-before-caller` checks all non-SCC `Private -> Private` call edges.

2. **Private-Subgraph Root Ownership (`Roots(h)`)**:
   - For each `p in P` (indices of `MethodVisibility::Public` callables in `V`), perform a DFS/BFS starting from the direct `MethodVisibility::Private` callees of `p` and traversing **only** `Private -> Private` edges (`u -> v` where both `u` and `v` are `Private`).
   - Add `p` to `Roots(h)` for every private callable `h` visited.
   - Because traversal stops at `Public` boundaries, if `pub_b` calls `pub_a` and `pub_a` calls `_a1`, `Roots(_a1) = {pub_a}` (single-user helper of `pub_a`).
   - Classification of each private callable `h`:
     - **Single-User (Exclusive) Helper**: `Roots(h) == [p]` (owned by a single public entrypoint `p`).
     - **Multi-User (Shared) Helper**: `Roots(h).len() >= 2` (reached by 2 or more public entrypoints).
     - **Unrooted Private Callable**: `Roots(h).is_empty()` (not reachable from any `Public` callable in the scope).

3. **Strongly Connected Components (`scc_id`)**:
   - Tarjan's algorithm over `(V, E)` assigns each callable `f_i` an `scc_id`. Any call edge `u -> v` with `scc_id[u] == scc_id[v]` (self-recursion or mutual recursion) is exempt from `callee-before-caller`.

4. **Stage 1 — `private-before-public-function` Findings (`pos < max(Roots(h))`)**:
   - For each private callable `h` at index `pos`:
     - **Unrooted Private Callable (`Roots(h).is_empty()`)**: if `last_pub` exists and `pos < last_pub`, flag `h` with `caller = V[last_pub].name` and mark `flagged_p1_or_p2[pos] = true`.
     - **Rooted Helper (`max_root = *Roots(h).last().unwrap()`)**: if `pos < max_root`, flag `h` with `caller = V[max_root].name` and mark `flagged_p1_or_p2[pos] = true`.

5. **Stage 2 — `uncolocated-helper` Findings (`pos > max(Roots(h))`)**:
   - For each rooted private callable `h` at index `pos > max_root`:
     - Check `is_valid_helper_placement(callables, &roots_of, pos, last_pub.unwrap())`:
       - **Place 2 (Trailing private helper section at the end of the scope)**: `pos > last_pub` is valid for any `|Roots(h)| >= 1`, provided that if `Roots(h) == [owner]`, `owner` does not also have another helper placed in Place 1 (`owner < m < last_pub` with `Roots(m) == [owner]`).
       - **Place 1 (Immediately after consumer)**: when `pos < last_pub`, valid only if `Roots(h) == [owner]` and every callable `m` in `(owner + 1)..pos` either has `Roots(m) == [owner]` or (when `V[owner].is_constructor`) belongs to the contiguous constructor cluster at the top of the scope.
     - If neither Place 1 nor Place 2 holds, flag `h` with `caller = V[max_root].name` and mark `flagged_p1_or_p2[pos] = true`.

6. **Stage 3 — `callee-before-caller` Findings (`Private -> Private`)**:
   - For each private callable `callee` not flagged by Stage 1 or Stage 2 (`!flagged_p1_or_p2[callee]` and `V[callee].visibility == MethodVisibility::Private`):
     - Find all direct callers `caller` such that:
       - `caller > callee` (`callee` is declared before `caller`),
       - `!flagged_p1_or_p2[caller]` and `V[caller].visibility == MethodVisibility::Private`,
       - `scc_id[caller] != scc_id[callee]` (not mutually recursive),
       - `!(Roots(caller).is_empty() && !Roots(callee).is_empty())`.
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

### 3.4 `uncolocated-helper` (Python, Rust)
- **File**: [src/code_lint/rules/uncolocated_helper.rs](../../../src/code_lint/rules/uncolocated_helper.rs)
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Opinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Private helper `{function}` is neither colocated after `{caller}` nor placed at the end of the scope."`
  - `rationale`: `"Placing `{function}` between unrelated public functions instead of immediately after `{caller}` or in the trailing helper section fractures the scope's layout."`
  - `suggestion`: `"Move `{function}` either immediately below `{caller}` (if single-use) or to the trailing private helper section after all public functions."`
- **`RuleDoc`**:
  - `summary`: `"Flags private helpers that are neither colocated right after their single public consumer nor placed in the trailing helper section at the end of the scope."`
- **Diagnostic Anchor**: `&finding.name_node` with placeholders `[("function", &finding.function), ("caller", &finding.caller)]`.
- **Named Exemptions**:
  - `E1` (**Place 1: Single-user helpers `[p, _h1, _h2]` immediately after consumer `p`**): When `Roots(h) = {p}` and every callable between `p` and `h` also belongs to `p` (or the constructor cluster when `p` is a constructor), `h` is in Place 1.
  - `E2` (**Place 2: Single-user or multi-user helpers in the trailing helper section `pos(h) > last_pub`**): Any helper `h` (`|Roots(h)| >= 1`) placed after all public functions in the scope is valid, provided a single-user helper does not split its owner's helpers between Place 1 and Place 2.
  - `E3` (**Public-to-public calls `pub_b -> pub_a -> _a1`**): Root propagation stops at public boundaries, so `_a1` has `Roots(_a1) = {pub_a}` (single-user helper of `pub_a`, not shared with `pub_b`).
  - `E4` (**Uncalled private functions `Roots(h) = empty`**): Private functions not reachable from any public entrypoint are governed by `private-before-public-function` (`pos > last_pub`) and are not flagged by `uncolocated-helper`.
  - `E5` (**Helper placed above a public caller `pos(h) < max(Roots(h))`**): Handled by `private-before-public-function`, never double-reported by `uncolocated-helper`.
  - `E6` (**Rust trait `impl` blocks and `#[cfg(test)]` / `#[test]` items**): Trait `impl` blocks and inline test modules/functions are excluded.

### 3.5 `private-before-public-function` (Python, Rust)
- **File**: [src/code_lint/rules/private_before_public_function.rs](../../../src/code_lint/rules/private_before_public_function.rs) (replaces `private_before_public_method.rs`)
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
  - `suggestion`: `"Move `{function}` below `{caller}` (either immediately after `{caller}` if single-use, or into the trailing private helper section)."`
- **`RuleDoc`**:
  - `summary`: `"Flags private functions and methods defined above their owning or calling public entrypoints."`
- **Diagnostic Anchor**: `&finding.name_node` with placeholders `[("function", &finding.function), ("caller", &finding.caller)]`.
- **Named Exemptions**:
  - `E1` (**Colocated single-user helper `[pub_a, _a1, pub_b, _b1]`**): When `Roots(_a1) = {pub_a}` and `_a1` appears below `pub_a` (`pos(_a1) > pos(pub_a)`), `_a1` is **not** flagged by `private-before-public-function` even though `pub_b` appears below `_a1`.
  - `E2` (**Shared helper placed below all of its public callers**): When `Roots(_shared) = {pub_a, pub_b}` and `pos(_shared) > max(pos(pub_a), pos(pub_b))`, `_shared` is not flagged by `private-before-public-function` (and if another `pub_c` follows it, `uncolocated-helper` flags it instead).
  - `E3` (**Python dunder methods `__name__` and Rust restricted visibility `pub(crate)` / `pub(super)`**): Treated as `MethodVisibility::Public`.
  - `E4` (**Python `@overload` and `@property` getter/setter/deleter grouping**): Grouped at the primary definition's position.
  - `E5` (**Scopes with only private functions**): Modules or classes with zero `Public` functions have no public-before-private boundary to violate.
  - `E6` (**Rust trait `impl` blocks and `#[cfg(test)]` / `#[test]` items**): Excluded.

### 3.6 `callee-before-caller` (Python, Rust)
- **File**: [src/code_lint/rules/callee_before_caller.rs](../../../src/code_lint/rules/callee_before_caller.rs)
- **Target**: `RuleTarget::SourceOnly`
- **Languages**: `&[Language::Python, Language::Rust]`
- **Options**: `RuleOptions::code_rule(())`
- **Classification**:
  - `topics`: `&[Topic::DECLARATION_ORDER]`
  - `precision`: `Precision::Exact`
  - `consensus`: `Consensus::Opinionated`
  - `impacted_quality`: `ImpactedQuality::Maintainability`
- **`ViolationTemplate`**:
  - `summary`: `"Private callee `{function}` is defined before its private caller `{caller}`."`
  - `rationale`: `"Defining lower-abstraction private helpers above `{caller}` inverts the top-down reading flow."`
  - `suggestion`: `"Move `{function}` below `{caller}` so higher-level callers precede lower-level callees."`
- **`RuleDoc`**:
  - `summary`: `"Flags private functions and methods defined before their private callers."`
- **Diagnostic Anchor**: `&finding.name_node` with placeholders `[("function", &finding.function), ("caller", &finding.caller)]`.
- **Named Exemptions**:
  - `E1` (**Public functions and methods (`Public -> Public`) and cross-visibility calls (`Public -> Private`, `Private -> Public`)**): Public entrypoints are peer API items that legitimately order either top-down or core-primitive-first, and `Public -> Private` is governed by `private-before-public-function` and `uncolocated-helper`.
  - `E2` (**Precedence suppression (Stage 1 / Stage 2 already flagged)**): Any callable already flagged by `private-before-public-function` or `uncolocated-helper` is skipped by `callee-before-caller` so a single misplaced helper is never double-reported.
  - `E3` (**Self-recursion and mutual recursion (Strongly Connected Components)**): Call edges within the same Tarjan SCC (`f <-> g`) are exempt.
  - `E4` (**Local variable / parameter shadowing**): Calls to a local variable or parameter that shadows a sibling function name (`check = ...; check()`) do not create a call edge to the sibling function.
  - `E5` (**Rust trait `impl` blocks and `#[cfg(test)]` / `#[test]` items**): Excluded.

### 3.7 `statement-after-main-guard` (Python) — Already Implemented
- **File**: [src/code_lint/rules/statement_after_main_guard.rs](../../../src/code_lint/rules/statement_after_main_guard.rs)
- **Summary**: `"Top-level statement appears after the `if __name__ == \"__main__\":` guard."`
