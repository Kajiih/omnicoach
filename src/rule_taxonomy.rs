//! Rule taxonomy: the facets each rule declares.
//!
//! Pure, constant metadata with no queries. Rules and runners never branch on it.

architecture_component!(RuleTaxonomy);

use strum::{EnumIter, EnumMessage, IntoStaticStr};

/// The subject of a rule: the only multi-valued, hierarchical facet.
///
/// Each topic is declared once, as an associated `const` on [`Topic`]. The consts are
/// `pub(crate)` so that a topic no rule uses fails the `dead_code` lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Topic {
    /// The topic's canonical `kebab-case` label, e.g. `test-timing`.
    pub label: &'static str,
    /// The broader topic, if any. Its type makes a parent in another facet impossible,
    /// and `const` evaluation makes a cycle a compile error (`E0391`).
    pub parent: Option<&'static Self>,
    /// One-line description of the topic.
    pub description: &'static str,
    /// What belongs under the topic and what does not.
    pub scope_note: &'static str,
    /// Other labels that select the same topic. Only the canonical label is ever displayed.
    pub synonyms: &'static [&'static str],
}

impl Topic {
    /// How identifiers are named.
    pub(crate) const NAMING: Self = Self {
        label: "naming",
        parent: None,
        description: "How identifiers are named.",
        scope_note: "Identifier names. Not string contents, file names, formatting or layout.",
        synonyms: &[],
    };

    /// Too short or cryptic names: abbreviations and single letters.
    pub(crate) const ABBREVIATED_NAMES: Self = Self {
        label: "abbreviated-names",
        parent: Some(&Self::NAMING),
        description: "Too short or cryptic names: abbreviations and single letters.",
        scope_note: "Abbreviations and single-letter identifiers. Not type or unit suffixes.",
        synonyms: &[],
    };

    /// Names encoding a type, container or unit.
    pub(crate) const TYPE_ENCODED_NAMES: Self = Self {
        label: "type-encoded-names",
        parent: Some(&Self::NAMING),
        description: "Names encoding a type, container or unit.",
        scope_note: "Type, container or unit encoded in the name (`users_dict`, `timeout_secs`). \
                     Not names that are just short.",
        synonyms: &[],
    };

    /// Practices specific to test code.
    pub(crate) const TESTING: Self = Self {
        label: "testing",
        parent: None,
        description: "Practices specific to test code.",
        scope_note: "How test code is structured and written. Not production code, even when \
                     tested.",
        synonyms: &[],
    };

    /// Count, shape and granularity of test assertions.
    pub(crate) const TEST_ASSERTIONS: Self = Self {
        label: "test-assertions",
        parent: Some(&Self::TESTING),
        description: "Count, shape and granularity of test assertions.",
        scope_note: "Number and shape of test assertions. Not production `assert` or what the \
                     test exercises.",
        synonyms: &[],
    };

    /// Mocks, fakes, stubs, spies and monkeypatching.
    pub(crate) const TEST_DOUBLES: Self = Self {
        label: "test-doubles",
        parent: Some(&Self::TESTING),
        description: "Mocks, fakes, stubs, spies and monkeypatching.",
        scope_note: "Replacing collaborators in tests and asserting on those replacements. Not \
                     fixtures that only build data.",
        synonyms: &[],
    };

    /// Sleeps, clocks and timeouts in tests.
    pub(crate) const TEST_TIMING: Self = Self {
        label: "test-timing",
        parent: Some(&Self::TESTING),
        description: "Sleeps, clocks and timeouts in tests.",
        scope_note: "Sleeping or waiting in tests. Not production retries or duration \
                     representation (see `durations`).",
        synonyms: &[],
    };

    /// What the static type checker can see and verify.
    pub(crate) const STATIC_TYPING: Self = Self {
        label: "static-typing",
        parent: None,
        description: "What the static type checker can see and verify.",
        scope_note: "What static type checking can prove. Not runtime validation or dataclass \
                     mutability (see `record-types`).",
        synonyms: &[],
    };

    /// Code that overrides or routes around the type checker.
    pub(crate) const TYPE_CHECKER_BYPASS: Self = Self {
        label: "type-checker-bypass",
        parent: Some(&Self::STATIC_TYPING),
        description: "Code that overrides or routes around the type checker.",
        scope_note: "Casts and dynamic access (`cast`, `getattr`) that hide types from the \
                     checker. Not `omni:` directives.",
        synonyms: &[],
    };

    /// Meaning carried by position instead of a name.
    pub(crate) const POSITIONAL_MEANING: Self = Self {
        label: "positional-meaning",
        parent: None,
        description: "Meaning carried by position instead of a name.",
        scope_note: "Meaning carried by argument order or tuple position instead of a name. Not \
                     named-field access.",
        synonyms: &[],
    };

    /// Reading sequence elements by literal index instead of unpacking.
    pub(crate) const POSITIONAL_INDEXING: Self = Self {
        label: "positional-indexing",
        parent: Some(&Self::POSITIONAL_MEANING),
        description: "Reading sequence elements by literal index instead of unpacking.",
        scope_note: "Access by literal index (`t[0]`) where destructuring would name the parts. \
                     Not loops over indices or slicing.",
        synonyms: &[],
    };

    /// Version-control operations and history.
    pub(crate) const VCS: Self = Self {
        label: "vcs",
        parent: None,
        description: "Version-control operations and history.",
        scope_note: "Commands and workflows of any version-control system. Not CI or code review.",
        synonyms: &["version-control"],
    };

    /// Rules specific to Jujutsu.
    pub(crate) const JJ: Self = Self {
        label: "jj",
        parent: Some(&Self::VCS),
        description: "Rules specific to Jujutsu.",
        scope_note: "Commands and workflows specific to Jujutsu. Not generic VCS behaviour.",
        synonyms: &["jujutsu"],
    };

    /// How spans of time are represented.
    pub(crate) const DURATIONS: Self = Self {
        label: "durations",
        parent: None,
        description: "How spans of time are represented.",
        scope_note: "Representing lengths of time and their units. Not sleeping/waiting (see \
                     `test-timing`) or wall-clock dates.",
        synonyms: &[],
    };

    /// Declaring named records (`dataclass`, `NamedTuple`, `struct`).
    pub(crate) const RECORD_TYPES: Self = Self {
        label: "record-types",
        parent: None,
        description: "Declaring named records (`dataclass`, `NamedTuple`, `struct`).",
        scope_note: "Field-bundle declarations, mutability and slots. Not enums or protocols.",
        synonyms: &[],
    };

    /// How literal values are written in code.
    pub(crate) const LITERALS: Self = Self {
        label: "literals",
        parent: None,
        description: "How literal values are written in code.",
        scope_note: "Writing string and number literals (multiline strings, magic numbers). Not \
                     identifiers or formatting APIs.",
        synonyms: &[],
    };

    /// Structural size and nesting of code units.
    // TODO: Make it more obvious that it not algorithmic complexity, with a different name
    pub(crate) const COMPLEXITY: Self = Self {
        label: "complexity",
        parent: None,
        description: "Structural size and nesting of code units.",
        scope_note: "Nesting depth and scope structure. Not naming or \"readability\" in general.",
        synonyms: &[],
    };

    /// Process-wide state read or written implicitly.
    pub(crate) const GLOBAL_STATE: Self = Self {
        label: "global-state",
        parent: None,
        description: "Process-wide state read or written implicitly.",
        scope_note: "Reading or writing ambient process-wide state (environment variables, \
                     globals). Not file or network I/O.",
        synonyms: &[],
    };

    /// Raising, catching, swallowing and reporting errors.
    pub(crate) const ERROR_HANDLING: Self = Self {
        label: "error-handling",
        parent: None,
        description: "Raising, catching, swallowing and reporting errors.",
        scope_note: "Exceptions and `Result`s, including `contextlib.suppress`. Not `omni:` \
                     directives (see `suppression-directives`).",
        synonyms: &[],
    };

    /// Use of logging APIs.
    pub(crate) const LOGGING: Self = Self {
        label: "logging",
        parent: None,
        description: "Use of logging APIs.",
        scope_note: "Log calls and their arguments. Not `print` or metrics.",
        synonyms: &[],
    };

    /// `async`/`await`, tasks and event loops.
    // TOODO: WHy not the name "concurrency?"
    pub(crate) const ASYNC: Self = Self {
        label: "async",
        parent: None,
        description: "`async`/`await`, tasks and event loops.",
        scope_note: "Coroutines, tasks and their lifetimes. Not OS threads.",
        synonyms: &[],
    };

    /// Hygiene of `omni:` suppression comments.
    pub(crate) const SUPPRESSION_DIRECTIVES: Self = Self {
        label: "suppression-directives",
        parent: None,
        description: "Hygiene of `omni:` suppression comments.",
        scope_note: "`omni:` directives that silence Omni. Not `contextlib.suppress` \
                     (see `error-handling`).",
        synonyms: &[],
    };
}

/// Precision facet: can the rule flag correct code?
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, IntoStaticStr, EnumMessage,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Precision {
    /// Flags only code that breaks the rule.
    Exact,
    /// Uses a proxy that can flag correct code.
    Heuristic,
}

/// Consensus facet: do reasonable people disagree with the rule?
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, IntoStaticStr, EnumMessage,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Consensus {
    /// There is an ordinary situation where the flagged code is correct and appropriate.
    Opinionated,
    /// The flagged code is wrong in every ordinary situation.
    Unopinionated,
}

/// Impacted quality facet (ISO/IEC 25010): what software quality suffers when violated.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, IntoStaticStr, EnumMessage,
)]
#[strum(serialize_all = "kebab-case")]
pub enum ImpactedQuality {
    /// Prevents a mechanism leading to wrong behaviour (bug, false-green test, wrong suppression).
    Reliability,
    /// Keeps code readable and changeable.
    Maintainability,
}

/// A rule's declared classification: one mandatory field per declared facet.
///
/// Derived facets (languages, analyzed input, file scope) have no field, and `topics` lists
/// only the most specific topics.
///
/// A valid classification:
/// ```
/// use omni::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
/// const TIMING: Topic = Topic {
///     label: "timing",
///     parent: None,
///     description: "Timing.",
///     scope_note: "Timing.",
///     synonyms: &[],
/// };
/// const OK: Classification = Classification {
///     topics: &[TIMING],
///     precision: Precision::Exact,
///     consensus: Consensus::Unopinionated,
///     impacted_quality: ImpactedQuality::Reliability,
/// };
/// ```
///
/// # Compile-time guarantees
///
/// Each example below is a classification mistake that must fail to compile. `cargo test`
/// runs them as `compile_fail` doctests and checks the expected error code.
///
/// A single-valued facet is missing:
/// ```compile_fail,E0063
/// # use omni::rule_taxonomy::{Classification, Consensus, Precision};
/// const MISSING: Classification = Classification {
///     topics: &[],
///     precision: Precision::Exact,
///     consensus: Consensus::Unopinionated,
/// };
/// ```
///
/// A single-valued facet gets two values:
/// ```compile_fail,E0062
/// # use omni::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision};
/// const TWICE: Classification = Classification {
///     topics: &[],
///     precision: Precision::Exact,
///     precision: Precision::Heuristic,
///     consensus: Consensus::Unopinionated,
///     impacted_quality: ImpactedQuality::Reliability,
/// };
/// ```
///
/// A facet value where a topic belongs:
/// ```compile_fail,E0308
/// # use omni::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision};
/// const MISPLACED: Classification = Classification {
///     topics: &[Precision::Heuristic],
///     precision: Precision::Exact,
///     consensus: Consensus::Unopinionated,
///     impacted_quality: ImpactedQuality::Reliability,
/// };
/// ```
///
/// A derived facet declared by hand:
/// ```compile_fail,E0560
/// # use omni::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision};
/// const DERIVED: Classification = Classification {
///     topics: &[],
///     precision: Precision::Exact,
///     consensus: Consensus::Unopinionated,
///     impacted_quality: ImpactedQuality::Reliability,
///     languages: &["python"],
/// };
/// ```
///
/// A topic that does not exist:
/// ```compile_fail,E0599
/// # use omni::rule_taxonomy::{Classification, Consensus, ImpactedQuality, Precision, Topic};
/// const TYPO: Classification = Classification {
///     topics: &[Topic::TESING],
///     precision: Precision::Exact,
///     consensus: Consensus::Unopinionated,
///     impacted_quality: ImpactedQuality::Reliability,
/// };
/// ```
///
/// A cycle in `Topic` parent links:
/// ```compile_fail,E0391
/// # use omni::rule_taxonomy::Topic;
/// const A: Topic = Topic {
///     label: "a",
///     parent: Some(&B),
///     description: "A.",
///     scope_note: "A.",
///     synonyms: &[],
/// };
/// const B: Topic = Topic {
///     label: "b",
///     parent: Some(&A),
///     description: "B.",
///     scope_note: "B.",
///     synonyms: &[],
/// };
/// # let _ = (A, B);
/// ```
///
/// A topic's parent in another facet:
/// ```compile_fail,E0308
/// # use omni::rule_taxonomy::{Precision, Topic};
/// const PARENT: Topic = Topic {
///     label: "bad",
///     parent: Some(&Precision::Exact),
///     description: "Bad.",
///     scope_note: "Bad.",
///     synonyms: &[],
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    /// The most specific topics; never a topic together with its ancestor.
    pub topics: &'static [Topic],
    /// Precision value.
    pub precision: Precision,
    /// Consensus value.
    pub consensus: Consensus,
    /// Impacted quality value.
    pub impacted_quality: ImpactedQuality,
}
