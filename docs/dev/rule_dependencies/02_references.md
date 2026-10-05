# Rule dependencies and soundness: 02 References

> [!NOTE]
> **Status: VERIFIED (2026-10-05).** Evidence behind the relation taxonomy in `01_understand.md` §2.1 and §3.

## 1. How other tools model relations between rules

E = modelled and enforced. D = documented only (prose or metadata). R = detected at run time only. – = absent.

| Tool | Equivalent / subsumes / overlaps | Chain | Cycle | Contradiction | Fix interference | Relies on | Lifecycle | Configuration coupling |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Ruff | D ("derived from"); E redirects | – | R ("Failed to converge after 100 iterations", treated as a bug) | E for hard-coded pairs (D203/D211, D212/D213: warn and ignore one); `pydocstyle.convention` picks a consistent subset; formatter-conflict warnings (COM812, ISC001) | E (overlapping fixes deferred to the next pass) | – | E (redirects; deprecated warns, removed errors) | `convention` option |
| ESLint | – | – | R (`MAX_AUTOFIX_PASSES = 10`; `ESLintCircularFixesWarning` in v9 names the offending rules) | E through `eslint-config-prettier` (off-switch config plus a CLI checker) | E (overlapping fixes deferred) | – | E `meta.deprecated.replacedBy` | – |
| Clippy | D | – | – | D for pairs (`implicit_return`/`needless_return`, `mod_module_files`/`self_named_module_files`); group guard E `blanket_clippy_restriction_lints` | E (via `rustfix`, which rejects overlapping replacement spans in a pass) | – | E (renamed and removed lints) | – |
| RuboCop | – | – | R (infinite-loop detection by source checksum, reports the cops involved) | Avoided by design: one cop with `EnforcedStyle` instead of two opposite cops | E `autocorrect_incompatible_with` | – | E `config/obsoletion.yml` (renamed, removed, split, extracted) | `Lint/RedundantCopDisableDirective` |
| Biome | E/D `sources: RuleSource::X(..).same()` vs `.inspired()` | – | – | – | E (`BatchMutation` skips overlapping text ranges in a pass) | Domains auto-enable rules from project dependencies (a project requirement, not a rule-to-rule one) | E (`RuleMetadata.deprecated`, `biome migrate`) | – |
| Pylint | – | – | – | – | – | – | E `old_names` | – |
| SonarQube | D ("related rules" prose section) | – | – | – | – | – | E (`deprecatedRuleKeys` with replacement key) | Quality profile inheritance |
| PMD | Deprecated references skipped to avoid duplicate runs | – | – | – | – | – | E `ref` + `deprecated` (rename, move) | – |
| golangci-lint | – | – | – | – | – | – | E (`Deprecation.Replacement` warning) | Presets |
| Semgrep | Free-form metadata only | – | – | – | – | – | – | – |

Takeaways:

- No tool models chains, relies-on, delegation or configuration coupling between rules.
- Cycles are only caught at run time, by iterating fixes to a fixed point.
- Contradictions are handled by hard-coded pairs (Ruff), an off-switch config (ESLint), a group guard (Clippy), or designed away with one rule and a style option (RuboCop `EnforcedStyle`, Ruff `convention`). The last is the cleanest pattern.

## 2. Formal frameworks

- **Term rewriting and modularity** (Baader and Nipkow, *Term Rewriting and All That*, Cambridge University Press, 1998; Newman, "On theories with a combinatorial definition of 'equivalence'", *Annals of Mathematics* 43(2): 223–243, 1942, https://doi.org/10.2307/1968867; Huet, "Confluent reductions: abstract properties and applications to term rewriting systems", *JACM* 27(4): 797–821, 1980, https://doi.org/10.1145/322217.322230).
  - Fixes are rewrite rules. A `Cycle` is non-termination; `Divergent advice` is a non-joinable critical pair ($y \leftarrow_a x \to_b z$ with no common normal form), while "They meet" (`01_understand.md` §2.1) is a joinable critical pair.
  - **Modularity of confluence (Hindley–Rosen lemma):** two confluent reduction relations have a confluent union if they commute, which for terminating rules reduces to local commutation at overlapping redexes.
  - **Modularity of termination (Bachmair and Dershowitz, "Commutation, transformation, and termination", *CADE-8*, LNCS 230, pp. 5–20, 1986, https://doi.org/10.1007/3-540-16780-3_76):** two individually terminating rules terminate when combined if the enablement graph of introduced redexes is acyclic (quasi-commutation). Because each lint rule terminates on its own, requiring the `Chain` graph to be a DAG guarantees that combined rules have no cycles (P2).
- **Created versus residual redexes** (Lévy, *Réductions correctes et optimales dans le lambda-calcul*, Thèse d'État, Univ. Paris VII, 1978; Terese, *Term Rewriting Systems*, Cambridge Tracts in Theoretical Computer Science 55, 2003, Ch. 4 and Ch. 8; Baeten, Bergstra, Klop, "Priority rewrite systems", *RTA 1987*, LNCS 256, pp. 83–94, https://doi.org/10.1007/3-540-17220-3_8).
  - After a rewrite step $u \xrightarrow{a} v$, every redex for rule $b$ in $v$ is either *created* by the step (constructed from tokens in $a$'s replacement template, with no ancestor in $u$) or a *residual* of $u$ (user-written tokens already present in $u$, previously masked by surrounding context that $a$ removed).
  - This backs the split in `01_understand.md` §2.1 between *introduced* chains (created redexes: F1 `Sequence`, T1 `y`, T5 `contextlib.suppress`) and *revealed* chains (residual redexes uncovered when a suffix is stripped: `t` in `t_ms`, `res` in `res_list`).
- **Graph transformation critical-pair analysis** (Habel, Heckel, Taentzer, "Graph grammars with negative application conditions", *Fundamenta Informaticae* 26(3–4): 287–313, 1996, https://doi.org/10.3233/FI-1996-263404; Mens, Taentzer, Runge, "Analysing refactoring dependencies using graph transformation", *SoSyM* 6(3): 269–285, 2007, https://doi.org/10.1007/s10270-006-0044-6).
  - Models refactorings as graph rewrite rules with Negative Application Conditions (NACs). Sequential dependencies split into *produce–use* ($a$'s RHS creates what $b$'s LHS matches = introduced chain) and *delete–forbid* ($a$ deletes what triggered $b$'s negative guard = revealed chain). Parallel conflicts split into *delete–use* (divergent advice when not joinable) and *produce–forbid* (contradiction or cycle).
- **Precondition/postcondition composition and assume-guarantee reasoning** (Kniesel and Koch, "Static composition of refactorings", *Science of Computer Programming* 52(1–3): 9–51, 2004, https://doi.org/10.1016/j.scico.2004.03.002; Cousot and Cousot, "Systematic design of program analysis frameworks", *POPL '79*, pp. 269–282, 1979, https://doi.org/10.1145/567752.567778; Christakis, Müller, Wüstholz, "Guiding dynamic symbolic execution toward unverified program executions", *ICSE '16*, pp. 144–155, 2016, https://doi.org/10.1145/2884781.2884843).
  - Distinguishes `Relies on` (P4 Safe) from `Delegates` and `Partitions` (P5 Complete). In `Relies on`, rule $a$'s transformation is only safe under a contextual assumption (e.g., the f-string is not a SQL query) that $a$ does not check itself (D8) and instead discharges to companion rule $b$ (S608). In `Delegates` and `Partitions`, a concern's domain is split across detectors without $a$'s fix introducing a new hazard when $b$ is off.
- **Feature models** (Kang et al., FODA, CMU/SEI-90-TR-21, 1990; Batory, "Feature models, grammars, and propositional formulas", *SPLC 2005*, LNCS 3714, pp. 7–20, https://doi.org/10.1007/11554844_3).
  - `requires` ($a \Rightarrow b$) matches `Relies on` / `Delegates`; `excludes` ($\neg(a \wedge b)$) matches `Contradiction`. Lint relations need soft constraints (warnings), since ADR 007 rules out auto-enabling.
- **Policy conflict and firewall anomaly analysis** (Moffett and Sloman, "Policy conflict analysis in distributed system management", *Journal of Organizational Computing* 4(1): 1–22, 1994, https://doi.org/10.1080/10919399409540214; Lupu and Sloman, "Conflicts in policy-based distributed systems management", *IEEE TSE* 25(6): 852–869, 1999, https://doi.org/10.1109/32.824414; Al-Shaer and Hamed, "Discovery of policy anomalies in distributed firewalls", *IEEE INFOCOM 2004*, Vol. 4, pp. 2605–2616, https://doi.org/10.1109/INFCOM.2004.1354680).
  - Separates modality conflict (what $a$ obligates, $b$ forbids: $F_a \cap V_b \neq \emptyset$) from detector overlap ($V_a \cap V_b \neq \emptyset$). Redundancy (same match, compatible action) is `Duplicates`; correlation (overlapping match, incompatible actions) is `Divergent advice`.
- **Empirical work** (Rutar, Almazan, Foster, "A comparison of bug finding tools for Java", *15th IEEE ISSRE 2004*, pp. 245–256, https://doi.org/10.1109/ISSRE.2004.1).
  - Bug-finder comparisons report little overlap between tools; no paper found defines a taxonomy of lint-rule interactions.

## 3. How relations and symmetric partitions are implemented in code

| Pattern | Precedent | Representation | Tradeoffs |
| :--- | :--- | :--- | :--- |
| **Node-local static slice** | Biome `RuleMetadata.sources: &'static [RuleSourceWithKind]` (`crates/biome_analyze/src/rule.rs`) | Each rule owns a `const` slice of typed outgoing links (`RuleSource::Eslint("...").same()` / `.inspired()`). | Co-located with the rule (matches D1); zero runtime overhead. If used for symmetric $N$-way relations, requires $N(N-1)$ pairwise entries across files. |
| **Node-local method without symmetry check** | RuboCop `Cop::Base.autocorrect_incompatible_with` (`lib/rubocop/cop/base.rb`) | Each cop returns a list of peer cop classes; checked in one direction during autocorrect. | Footgun: forgetting the reverse link makes behavior depend on rule execution order. |
| **Central tuple table** | Ruff `INCOMPATIBLE_RULES: &[(Rule, Rule, &str)]` (`crates/ruff_workspace/src/configuration.rs`), `RULE_REDIRECTS` | Single global table of `(RuleA, RuleB, reason)` tuples. | Avoids duplication, but splits rule metadata away from rule definitions (violates D1). |
| **Named equivalence class / partition constant ($N$-ary)** | OWL 2 `owl:disjointUnionOf` / `owl:AllDisjointClasses` (W3C, 2012); Debian virtual packages (`Provides` + `Conflicts`, Debian Policy §7.5.2); Kconfig `choice` (Linux kernel); Omni `Topic` (`src/rule_declaration/taxonomy.rs`) | Define a shared constant per partition axis (`Partition { concern, condition }`) in `RuleDeclaration`; each member rule lists `partitions: &[Partition::...]`. | Symmetric and transitive by construction; $O(N)$ instead of $O(N^2)$ entries; shared `concern` and `condition` written once; supports orthogonal axes on the same rule (`PARAMETER_COLLECTION_KIND` vs `CONCRETE_COLLECTION_POSITION`); respects sibling-rule module isolation (`src/architecture.rs`). |

Takeaways for Omni:

- **Directed and external relations** (`Chain`, `ReliesOn`, `Delegates`, `Duplicates`) fit Biome's node-local static slice on `Declaration` (`03_design.md` §1.1). For Omni → Omni directed edges ($a \to b$), declaring the edge once on $a$ and letting `rule_catalog.rs` also display incoming edges on $b$ avoids duplicate maintenance. For incoming external chains (T5: Ruff `SIM105` → `suppressed-exception`), a direction field or subject-first constructor lets the Omni rule record the incoming edge locally.
- **Symmetric Omni ↔ Omni relations** in `01_understand.md` §3.3 are all `Partitions` once D9 resolves T3 (`DivergentAdvice`). Modeling them as named `Partition` equivalence-class constants in `src/rule_declaration/` (the exact pattern `Topic` already uses in `src/rule_declaration/taxonomy.rs`) eliminates 30 pairwise back-and-forth declarations across 13 rule files, keeps shared conditions (`when banned lists agree`) in one place, separates orthogonal partition axes on collection rules, and guarantees symmetry by construction.

## 4. Sources

- Ruff:
  - https://docs.astral.sh/ruff/formatter/#conflicting-lint-rules
  - https://docs.astral.sh/ruff/settings/#lint_pydocstyle_convention
  - https://docs.astral.sh/ruff/linter/#fix-safety
  - https://github.com/astral-sh/ruff/blob/main/crates/ruff_linter/src/rule_redirects.rs
  - https://github.com/astral-sh/ruff/blob/main/crates/ruff_workspace/src/configuration.rs (`INCOMPATIBLE_RULES`)
- ESLint:
  - https://eslint.org/docs/latest/extend/custom-rules#rule-structure
  - https://github.com/prettier/eslint-config-prettier#cli-helper-tool
- Clippy:
  - https://rust-lang.github.io/rust-clippy/master/index.html#blanket_clippy_restriction_lints
  - https://rust-lang.github.io/rust-clippy/master/index.html#min_ident_chars
- RuboCop:
  - https://github.com/rubocop/rubocop/blob/master/lib/rubocop/cop/base.rb (`autocorrect_incompatible_with`)
  - https://github.com/rubocop/rubocop/blob/master/lib/rubocop/runner.rb (infinite-loop detection)
  - https://github.com/rubocop/rubocop/blob/master/config/obsoletion.yml
  - https://docs.rubocop.org/rubocop/configuration.html
- Biome:
  - https://biomejs.dev/linter/rules-sources/
  - https://biomejs.dev/linter/domains/
  - https://github.com/biomejs/biome/blob/main/crates/biome_analyze/src/rule.rs (`RuleMetadata`, `RuleSourceWithKind`)
- Equivalence classes and partitions:
  - W3C OWL 2 Web Ontology Language Structural Specification, §9.1.4 (`DisjointUnion`) and §9.1.2 (`DisjointClasses`), https://www.w3.org/TR/owl2-syntax/#Disjoint_Union_of_Class_Expressions
  - Debian Policy Manual, §7.5.2 "Replacing whole packages, forcing their removal" (virtual packages), https://www.debian.org/doc/debian-policy/ch-relationships.html
- PMD: https://docs.pmd-code.org/latest/pmd_userdocs_making_rulesets.html
- Pylint:
  - https://pylint.readthedocs.io/en/latest/user_guide/messages/information/useless-suppression.html
  - https://pylint.readthedocs.io/en/stable/user_guide/messages/warning/dangerous-default-value.html
- golangci-lint: https://golangci-lint.run/usage/linters/
- Semgrep: https://semgrep.dev/docs/writing-rules/rule-syntax

