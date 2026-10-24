# Adding a Rule

> [!NOTE]
> How-to for contributors. Each step says what to do and links to where the contract is documented; nothing here repeats it. Why rules are shaped this way: [rule_design_guide.md](rule_design_guide.md). Building several rules in parallel: [rule_batch_playbook.md](rule_batch_playbook.md).

The Rust items below are documented in Rustdoc: hover them in your IDE, or run `cargo doc --no-deps --document-private-items --open`.

## Steps

1. **Check it is one antipattern, and name it.** Split it if two violations differ in why they are bad or how to fix them ([rule_design_guide.md](rule_design_guide.md) §1). Name the pattern, not the verdict ([naming_and_message_style_guide.md](naming_and_message_style_guide.md) §1). If it is a candidate in [ROADMAP.md](../../ROADMAP.md), remove that entry when the rule lands.
2. **Create the rule file.** Add `src/code_lint/rules/<rule_name>.rs`, declare it with `pub mod` in [rules.rs](../../src/code_lint/rules.rs) and add its `CodeRule` to `CODE_RULES`. Start from a rule with the same option shape (`sleep_in_tests.rs` for a list, `too_many_assertions.rs` for a count). Contract: `CodeRule` in [contract.rs](../../src/code_lint/contract.rs). What a rule file may import: the module doc of [architecture.rs](../../src/architecture.rs).
3. **Write the message template.** One `violation_template!`: a factual summary, the failure mode, one fix per language. Contract: `ViolationTemplate` in [diagnostic.rs](../../src/diagnostic.rs). Wording: [naming_and_message_style_guide.md](naming_and_message_style_guide.md) §2–3.
4. **Declare options**, if the rule has a list or a threshold a project should tune ([rule_design_guide.md](rule_design_guide.md) §5). Contract: `RuleOptions`, `CountOption` and `ListOption` in [options.rs](../../src/rule_declaration/options.rs). Key names: [naming_and_message_style_guide.md](naming_and_message_style_guide.md) §4.
5. **Classify it.** Fill `Classification` using the yes/no tests of [tag_guide.md](tag_guide.md) §2; a new topic follows §4.
6. **Document it.** Fill `RuleDoc`, with one executed `Example` per language. The goal is predictability: a reader of `--explain` can tell whether their code is flagged ([rule_design_guide.md](rule_design_guide.md) §6). Contract: `RuleDoc` and `Example` in [documentation.rs](../../src/rule_declaration/documentation.rs). Doc summary wording: [naming_and_message_style_guide.md](naming_and_message_style_guide.md) §2.5.
7. **Implement the check and evolve the shared abstractions** through `code_lint::ast`, `code_lint::semantic`, and `code_lint::policy`. Treat adding or updating a rule as an opportunity to improve the architecture, never as a narrow patch: when an existing extractor bakes in a single rule's exemptions or misses a language fact, refactor the shared model so `ast` exposes pure facts and rules own policy ([ADR 012](decisions/012_module_placement_by_abstraction.md), Phase 5 of [rule_batch_playbook.md](rule_batch_playbook.md)). Contract: `CodeRule`.
8. **Test it** with one `rule_test!` invocation at the end of the file. Contract and case-writing standards: `rule_test!` in [test_utils.rs](../../src/test_utils.rs). Give each named exemption a dedicated `pass` case and verify branch and mutation adequacy with `cargo mutants --cap-lints=true -f src/code_lint/rules/<rule_name>.rs -- --lib` (or `cargo mutants --cap-lints=true --in-diff -- --lib`) and `cargo +nightly llvm-cov --lib --branch`.
9. **Verify.** Run all four checks locally (rustdoc broken-link lints only run under `cargo doc`):

   ```bash
   cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo doc --no-deps --document-private-items
   ```

   `cargo test` includes the registry tests (`tests/registry.rs`), which check names, messages, docs and examples against the guides. `omnicoach --explain <rule-name>` shows the rule as users will see it.

## Command rules

A command rule follows the same steps with `CommandRule` and `CommandEnv` in [contract.rs](../../src/command_lint/contract.rs), registered in `COMMAND_RULES` in [rules.rs](../../src/command_lint/rules.rs), documented with one executed `CommandExample` in [documentation.rs](../../src/rule_declaration/documentation.rs), and tested with `command_rule_test!` in [test_utils.rs](../../src/test_utils.rs).
