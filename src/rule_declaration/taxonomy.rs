//! Rule taxonomy: the facets each rule declares.
//!
//! Pure, constant metadata with no queries. Rules and runners never branch on it.

use strum::{EnumIter, EnumMessage, IntoStaticStr};

/// The subject of a rule: the only multi-valued, hierarchical facet.
///
/// Each topic is declared once, as an associated `const` on [`Topic`], via [`Self::root`] or
/// [`Self::sub`]. The consts are `pub(crate)` so that a topic no rule uses fails `dead_code`.
///
/// Admit a topic only when it (1) names a subject (not a quality), (2) passes the
/// all-and-some test under its parent, (3) has a `description` and an includes/excludes
/// `scope_note`, (4) has a unique `kebab-case` label, and (5) has at least one rule
/// (`docs/dev/tag_guide.md` §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Topic {
    label: &'static str,
    parent: Option<&'static Self>,
    description: &'static str,
    scope_note: &'static str,
    synonyms: &'static [&'static str],
}

/// What a [`Topic`] covers: both fields are required, so a topic without a scope note does
/// not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopicDoc {
    /// One-line summary of what the topic covers.
    pub description: &'static str,
    /// What belongs under the topic and what does not.
    pub scope_note: &'static str,
}

impl Topic {
    /// A root topic with no broader parent topic.
    #[must_use]
    pub const fn root(label: &'static str, doc: TopicDoc) -> Self {
        Self {
            label,
            parent: None,
            description: doc.description,
            scope_note: doc.scope_note,
            synonyms: &[],
        }
    }

    /// A subtopic under `parent`. Taking a `&'static Topic` keeps parents within the topic
    /// facet (`E0308`), and `const` evaluation makes a parent cycle a compile error (`E0391`).
    #[must_use]
    pub const fn sub(label: &'static str, parent: &'static Self, doc: TopicDoc) -> Self {
        Self {
            label,
            parent: Some(parent),
            description: doc.description,
            scope_note: doc.scope_note,
            synonyms: &[],
        }
    }

    /// Sets alternative labels that resolve to this topic.
    #[must_use]
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.synonyms = aliases;
        self
    }

    /// The topic's canonical `kebab-case` label, e.g. `test-timing`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.label
    }

    /// The broader topic, if any.
    #[must_use]
    pub const fn parent(self) -> Option<Self> {
        match self.parent {
            Some(parent) => Some(*parent),
            None => None,
        }
    }

    /// One-line description of the topic.
    #[must_use]
    pub const fn description(self) -> &'static str {
        self.description
    }

    /// What belongs under the topic and what does not.
    #[must_use]
    pub const fn scope_note(self) -> &'static str {
        self.scope_note
    }

    /// Other labels that select the same topic. Only the canonical label is ever displayed.
    #[must_use]
    pub const fn synonyms(self) -> &'static [&'static str] {
        self.synonyms
    }
}

impl Topic {
    pub(crate) const NAMING: Self = Self::root(
        "naming",
        TopicDoc {
            description: "How identifiers are named.",
            scope_note: "Identifier names. Not string contents, file names, formatting or layout.",
        },
    );

    pub(crate) const ABBREVIATED_NAMES: Self = Self::sub(
        "abbreviated-names",
        &Self::NAMING,
        TopicDoc {
            description: "Too short or cryptic names: abbreviations and single letters.",
            scope_note: "Abbreviations and single-letter identifiers. Not type or unit suffixes.",
        },
    );

    pub(crate) const TYPE_ENCODED_NAMES: Self = Self::sub(
        "type-encoded-names",
        &Self::NAMING,
        TopicDoc {
            description: "Names encoding a type, container or unit.",
            scope_note: "Type, container or unit encoded in the name (`users_dict`, \
                         `timeout_secs`). Not names that are just short.",
        },
    );

    pub(crate) const TESTING: Self = Self::root(
        "testing",
        TopicDoc {
            description: "Practices specific to test code.",
            scope_note: "How test code is structured and written. Not production code, even \
                         when tested.",
        },
    );

    pub(crate) const TEST_ASSERTIONS: Self = Self::sub(
        "test-assertions",
        &Self::TESTING,
        TopicDoc {
            description: "Count, shape and granularity of test assertions.",
            scope_note: "Number and shape of test assertions. Not production `assert` or what \
                         the test exercises.",
        },
    );

    pub(crate) const TEST_DOUBLES: Self = Self::sub(
        "test-doubles",
        &Self::TESTING,
        TopicDoc {
            description: "Mocks, fakes, stubs, spies and monkeypatching.",
            scope_note: "Replacing collaborators in tests and asserting on those replacements. \
                         Not fixtures that only build data.",
        },
    );

    pub(crate) const TEST_TIMING: Self = Self::sub(
        "test-timing",
        &Self::TESTING,
        TopicDoc {
            description: "Sleeps, clocks and timeouts in tests.",
            scope_note: "Sleeping or waiting in tests. Not production retries or duration \
                         representation (see `durations`).",
        },
    );

    pub(crate) const STATIC_TYPING: Self = Self::root(
        "static-typing",
        TopicDoc {
            description: "What the static type checker can see and verify.",
            scope_note: "What static type checking can prove. Not runtime validation or \
                         dataclass mutability (see `record-types`).",
        },
    );

    pub(crate) const TYPE_CHECKER_BYPASS: Self = Self::sub(
        "type-checker-bypass",
        &Self::STATIC_TYPING,
        TopicDoc {
            description: "Code that overrides or routes around the type checker.",
            scope_note: "Casts and dynamic access (`cast`, `getattr`) that hide types from the \
                         checker. Not `omni:` directives.",
        },
    );

    pub(crate) const POSITIONAL_MEANING: Self = Self::root(
        "positional-meaning",
        TopicDoc {
            description: "Meaning carried by position instead of a name.",
            scope_note: "Meaning carried by argument order or tuple position instead of a name. \
                         Not named-field access.",
        },
    );

    pub(crate) const POSITIONAL_INDEXING: Self = Self::sub(
        "positional-indexing",
        &Self::POSITIONAL_MEANING,
        TopicDoc {
            description: "Reading sequence elements by literal index instead of unpacking.",
            scope_note: "Access by literal index (`t[0]`) where destructuring would name the \
                         parts. Not loops over indices or slicing.",
        },
    );

    pub(crate) const VCS: Self = Self::root(
        "vcs",
        TopicDoc {
            description: "Version-control operations and history.",
            scope_note: "Commands and workflows of any version-control system. Not CI or code \
                         review.",
        },
    )
    .aliases(&["version-control"]);

    pub(crate) const JJ: Self = Self::sub(
        "jj",
        &Self::VCS,
        TopicDoc {
            description: "Rules specific to Jujutsu.",
            scope_note: "Commands and workflows specific to Jujutsu. Not generic VCS behaviour.",
        },
    )
    .aliases(&["jujutsu"]);

    pub(crate) const DURATIONS: Self = Self::root(
        "durations",
        TopicDoc {
            description: "How spans of time are represented.",
            scope_note: "Representing lengths of time and their units. Not sleeping/waiting \
                         (see `test-timing`) or wall-clock dates.",
        },
    );

    pub(crate) const RECORD_TYPES: Self = Self::root(
        "record-types",
        TopicDoc {
            description: "Declaring named records (`dataclass`, `NamedTuple`, `struct`).",
            scope_note: "Field-bundle declarations, mutability and slots. Not enums or \
                         protocols.",
        },
    );

    pub(crate) const LITERALS: Self = Self::root(
        "literals",
        TopicDoc {
            description: "How literal values are written in code.",
            scope_note: "Writing string and number literals (multiline strings, format \
                         placeholders, magic numbers). Not identifiers or logging calls (see \
                         `logging`).",
        },
    );

    pub(crate) const NESTING: Self = Self::root(
        "nesting",
        TopicDoc {
            description: "Nesting depth and scope structure of code units.",
            scope_note: "Nested functions, closures and scope depth. Not naming or \
                         \"readability\" in general.",
        },
    );

    pub(crate) const GLOBAL_STATE: Self = Self::root(
        "global-state",
        TopicDoc {
            description: "Process-wide state read or written implicitly.",
            scope_note: "Reading or writing ambient process-wide state (environment variables, \
                         globals). Not file or network I/O.",
        },
    );

    pub(crate) const ERROR_HANDLING: Self = Self::root(
        "error-handling",
        TopicDoc {
            description: "Raising, catching, swallowing and reporting errors.",
            scope_note: "Exceptions and `Result`s, including `contextlib.suppress`. Not `omni:` \
                         directives (see `suppression-directives`).",
        },
    );

    pub(crate) const LOGGING: Self = Self::root(
        "logging",
        TopicDoc {
            description: "Use of logging APIs.",
            scope_note: "Log calls and their arguments. Not `print` or metrics.",
        },
    );

    pub(crate) const CONCURRENCY: Self = Self::root(
        "concurrency",
        TopicDoc {
            description: "Concurrent execution: coroutines, tasks and threads.",
            scope_note: "Spawning, awaiting and joining concurrent work and its lifetimes. Not \
                         sleeping or waiting in tests (see `test-timing`).",
        },
    );

    pub(crate) const SUPPRESSION_DIRECTIVES: Self = Self::root(
        "suppression-directives",
        TopicDoc {
            description: "Hygiene of suppression directives.",
            scope_note: "`omni:` directives that silence Omni, and the reasons given on \
                         third-party Python ones (`# noqa`, `# type: ignore`, ...). Not \
                         `contextlib.suppress` (see `error-handling`).",
        },
    );

    pub(crate) const DECLARATION_ORDER: Self = Self::root(
        "declaration-order",
        TopicDoc {
            description: "Order in which declarations appear in a scope.",
            scope_note: "Ordering of functions, methods and items within a module or class. Not \
                         nesting depth (see `nesting`) or naming.",
        },
    );

    pub(crate) const DOCSTRINGS: Self = Self::root(
        "docstrings",
        TopicDoc {
            description: "Presence, content and sections of docstrings.",
            scope_note: "Whether a docstring is present, what it says and how its Google-style \
                         sections (`Args:`, `Returns:`, `Attributes:`) are written. Not comments \
                         or `omni:` directives.",
        },
    )
    .aliases(&["documentation"]);
}

/// Precision facet: can the rule flag correct code?
///
/// Precision concerns false positives only: an `exact` rule may still miss violations.
///
/// **Test:** does the rule's *design* use a syntactic proxy for the target construct or its
/// exemption boundary, so that it can flag code that is correct? If yes → [`Self::Heuristic`].
/// Worked examples and edge cases: `docs/dev/tag_guide.md` §2.2.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, EnumMessage, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Precision {
    /// Flags only code that breaks the rule; may still miss violations.
    Exact,
    /// Uses a proxy that can flag correct code.
    Heuristic,
}

/// Consensus facet: do reasonable people disagree with the rule?
///
/// **Test:** name one ordinary situation where the flagged code is correct and appropriate.
/// If you can → [`Self::Opinionated`]. Worked examples: `docs/dev/tag_guide.md` §2.3.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, EnumMessage, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Consensus {
    /// There is an ordinary situation where the flagged code is correct and appropriate.
    Opinionated,
    /// The flagged code is wrong in every ordinary situation.
    Unopinionated,
}

/// Impacted quality facet (ISO/IEC 25010): what software quality suffers when violated.
///
/// **Test:** does the flagged code or command have a *mechanism that leads to wrong
/// behaviour* (a bug, a flaky or false-green test, a wrong suppression, a command that
/// corrupts state or history)? If yes → [`Self::Reliability`]. Worked examples and when to
/// add another ISO/IEC 25010 value: `docs/dev/tag_guide.md` §2.4.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumIter, EnumMessage, IntoStaticStr,
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
/// only the most specific topics. How to classify a rule, and when to add a [`Topic`], is
/// in `docs/dev/tag_guide.md` §2 and §4.
///
/// A valid classification:
/// ```
/// use omnicoach::rule_declaration::{
///     Classification, Consensus, ImpactedQuality, Precision, Topic, TopicDoc,
/// };
/// const TIMING: Topic = Topic::root(
///     "timing",
///     TopicDoc { description: "Timing.", scope_note: "Timing." },
/// );
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
/// # use omnicoach::rule_declaration::{Classification, Consensus, Precision};
/// const MISSING: Classification = Classification {
///     topics: &[],
///     precision: Precision::Exact,
///     consensus: Consensus::Unopinionated,
/// };
/// ```
///
/// A single-valued facet gets two values:
/// ```compile_fail,E0062
/// # use omnicoach::rule_declaration::{Classification, Consensus, ImpactedQuality, Precision};
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
/// # use omnicoach::rule_declaration::{Classification, Consensus, ImpactedQuality, Precision};
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
/// # use omnicoach::rule_declaration::{Classification, Consensus, ImpactedQuality, Precision};
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
/// # use omnicoach::rule_declaration::{Classification, Consensus, ImpactedQuality, Precision, Topic};
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
/// # use omnicoach::rule_declaration::{Topic, TopicDoc};
/// const DOC: TopicDoc = TopicDoc { description: "Doc.", scope_note: "Doc." };
/// const A: Topic = Topic::sub("a", &B, DOC);
/// const B: Topic = Topic::sub("b", &A, DOC);
/// # let _ = (A, B);
/// ```
///
/// A topic's parent in another facet:
/// ```compile_fail,E0308
/// # use omnicoach::rule_declaration::{Precision, Topic, TopicDoc};
/// const BAD: Topic = Topic::sub(
///     "bad",
///     &Precision::Exact,
///     TopicDoc { description: "Bad.", scope_note: "Bad." },
/// );
/// # let _ = BAD;
/// ```
///
/// A topic missing its `scope_note`:
/// ```compile_fail,E0063
/// # use omnicoach::rule_declaration::{Topic, TopicDoc};
/// const UNSCOPED: Topic = Topic::root("unscoped", TopicDoc { description: "Unscoped." });
/// # let _ = UNSCOPED;
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
