# Phase 7: Learn — Declaration Ordering (`Topic::DECLARATION_ORDER`)

> [!IMPORTANT]
> **Status**: **COMPLETED**

---

## Reusable Engineering Lessons

1. **Test Call-Graph vs. Tier Ordering as a Formal Compatibility Proof Before Implementing**:
   - The retired `call-before-definition` rule failed in practice because a call-graph partial order ($\text{pos}(v) < \text{pos}(u)$ for every call $u \to v$) is mathematically incompatible with visibility-tier ordering ($\text{pos}(\text{Public}) < \text{pos}(\text{Private})$) whenever a public entrypoint calls a private helper.
   - Framing candidate ordering rules as partial orders over a scope and checking compatibility on cross-tier edges, same-tier abstraction gradients, and multi-entrypoint DAGs (`36%–48%` of real-world scopes have both forward and backward same-tier calls) eliminated an entire class of noisy rules (`call-before-definition`, `stepdown`, and rigid module-level item-kind buckets) before writing rule code.

2. **2-Tier Visibility (`Exported` vs. `Private`) Preserves Restricted-Constructor Encapsulation in Rust**:
   - In Rust inherent `impl` blocks, types are frequently constructed via subsystem-scoped constructors (`pub(crate) fn new`, `pub(in crate::...) fn from_span`, `pub(super) fn new_from_str`) while exposing `pub fn` accessors.
   - A 3-tier visibility rule (`pub` $\to$ `pub(...)` $\to$ `fn`) would force `pub(crate) fn new` below `pub fn` getters, directly contradicting `constructor-after-method`. Partitioning inherent `impl` methods into 2 tiers (`Exported`: any `visibility().is_some()` vs. `Private`: bare `fn`) aligns visibility ordering with constructor-first lifecycle ordering.

3. **Rust `ra_ap_syntax` Outer Doc Comments Precede Outer Attributes Inside `SyntaxNode::text_range()`**:
   - Unlike `tree-sitter-rust` (where outer attributes and doc comments are preceding siblings of an item), `ra_ap_syntax` attaches both `///` doc comments and `#[...]` outer attributes as leading children inside the item's `SyntaxNode`.
   - Any range collector over `ra_ap_syntax` nodes (such as `collect_inline_test_ranges_rec` in [src/code_lint/ast/rust.rs](../../../src/code_lint/ast/rust.rs)) must start at `node.text_range().start()` rather than the first `ast::Attr`'s start offset so that `item.syntax().text_range().start()` on documented `#[cfg(test)]` items is covered by the range.

4. **Rust Constructor Detection Requires Checking the Return Type for `Self`**:
   - Matching exported `new_*` / `try_new_*` associated functions without a `self` receiver is necessary but not sufficient to identify constructors in Rust inherent `impl` blocks: factory functions that produce a different type (such as `pub fn new_request_id() -> u64`) share the `new_*` prefix. Checking that `function.ret_type()` contains a `Self` (`SELF_TYPE_KW`) or self-type identifier token eliminates false positives on non-`Self` factory functions.

5. **`rule_test!` Doubled-Snippet Check on Module-Level Boundary Rules**:
   - Because `assert_rule_fail` in [src/test_utils.rs](../../../src/test_utils.rs) concatenates `{code}\n\n{code}` into a single file and expects exactly the two copies' diagnostic spans, a module-level boundary rule like `statement-after-main-guard` will flag any setup statement that precedes the first `if __name__ == "__main__":` guard when that setup statement reappears in the second copy after the first guard. Starting `fail` snippets in `rule_test!` with the boundary statement itself (and testing pre-boundary statements in unit tests on the AST collector) keeps the doubled-file check strict without distorting rule semantics.
