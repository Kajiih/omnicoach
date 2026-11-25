//! Two-stage Jujutsu (`jj`) CLI subcommand and option parser.

use std::path::PathBuf;

use super::revset::{
    format_bookmark_revset, format_parent_revset, format_revset_terms, format_single_or_union,
    intersect_revset_with_mutable, is_bounded_jj_revset, join_revset_union,
};
use crate::command_lint::command::{
    CliSpec, FlagName, InterceptedCommand, LastOptionValue, ParsedCli, PreSubcommandSpec,
    SubcommandCliSchema,
};

const JJ_PROGRAM: &str = "jj";
const SUBCOMMAND_GIT: &str = "git";
const SUBCOMMAND_PUSH: &str = "push";

const OPT_REPOSITORY: &str = "repository";
const OPT_REVISION: &str = "revision";
const OPT_REVISIONS: &str = "revisions";
const OPT_FROM: &str = "from";
const OPT_INTO: &str = "into";
const OPT_TO: &str = "to";
const OPT_ONTO: &str = "onto";
const OPT_DESTINATION: &str = "destination";
const OPT_INSERT_AFTER: &str = "insert-after";
const OPT_AFTER: &str = "after";
const OPT_INSERT_BEFORE: &str = "insert-before";
const OPT_BEFORE: &str = "before";
const OPT_MESSAGE: &str = "message";
const OPT_TOOL: &str = "tool";
const OPT_SOURCE: &str = "source";
const OPT_BRANCH: &str = "branch";
const OPT_AUTHOR: &str = "author";
const OPT_LIMIT: &str = "limit";
const OPT_CHANGES_IN: &str = "changes-in";
const OPT_BOOKMARK: &str = "bookmark";
const OPT_TAG: &str = "tag";
const OPT_CHANGE: &str = "change";
const OPT_NAMED: &str = "named";

const REPO_FLAGS: &[FlagName<'_>] = &[FlagName::Short('R'), FlagName::Long(OPT_REPOSITORY)];
const REV_FLAGS: &[FlagName<'_>] = &[FlagName::Short('r'), FlagName::Long(OPT_REVISION)];
const REVS_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('r'),
    FlagName::Long(OPT_REVISION),
    FlagName::Long(OPT_REVISIONS),
];
const FROM_FLAGS: &[FlagName<'_>] = &[FlagName::Short('f'), FlagName::Long(OPT_FROM)];
const INTO_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('t'),
    FlagName::Long(OPT_INTO),
    FlagName::Long(OPT_TO),
];
const DIFFEDIT_TO_FLAGS: &[FlagName<'_>] = &[FlagName::Short('t'), FlagName::Long(OPT_TO)];
const ONTO_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('o'),
    FlagName::Long(OPT_ONTO),
    FlagName::Short('d'),
    FlagName::Long(OPT_DESTINATION),
];
const AFTER_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('A'),
    FlagName::Long(OPT_INSERT_AFTER),
    FlagName::Long(OPT_AFTER),
];
const BEFORE_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('B'),
    FlagName::Long(OPT_INSERT_BEFORE),
    FlagName::Long(OPT_BEFORE),
];
const SQUASH_NEW_COMMIT_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('o'),
    FlagName::Long(OPT_ONTO),
    FlagName::Short('d'),
    FlagName::Long(OPT_DESTINATION),
    FlagName::Short('A'),
    FlagName::Long(OPT_INSERT_AFTER),
    FlagName::Long(OPT_AFTER),
    FlagName::Short('B'),
    FlagName::Long(OPT_INSERT_BEFORE),
    FlagName::Long(OPT_BEFORE),
];
const CHANGES_IN_FLAGS: &[FlagName<'_>] = &[FlagName::Short('c'), FlagName::Long(OPT_CHANGES_IN)];
const SOURCE_FLAGS: &[FlagName<'_>] = &[FlagName::Short('s'), FlagName::Long(OPT_SOURCE)];
const BRANCH_FLAGS: &[FlagName<'_>] = &[FlagName::Short('b'), FlagName::Long(OPT_BRANCH)];
const BOOKMARK_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('b'),
    FlagName::Long(OPT_BOOKMARK),
    FlagName::Long(OPT_BRANCH),
];
const CHANGE_FLAGS: &[FlagName<'_>] = &[FlagName::Short('c'), FlagName::Long(OPT_CHANGE)];
const MESSAGE_FLAGS: &[FlagName<'_>] = &[FlagName::Short('m'), FlagName::Long(OPT_MESSAGE)];
const LIST_FLAGS: &[FlagName<'_>] = &[FlagName::Short('l'), FlagName::Long("list")];
const LIMIT_FLAGS: &[FlagName<'_>] = &[FlagName::Short('n'), FlagName::Long(OPT_LIMIT)];
const INTERACTIVE_OR_TOOL_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('i'),
    FlagName::Long("interactive"),
    FlagName::Long(OPT_TOOL),
];
const SKIP_COMBINED_DESCRIPTION_FLAGS: &[FlagName<'_>] = &[
    FlagName::Short('u'),
    FlagName::Long("use-destination-message"),
    FlagName::Short('k'),
    FlagName::Long("keep-emptied"),
];
const EXPLICIT_PUSH_SELECTOR_FLAGS: &[FlagName<'_>] = &[
    FlagName::Long("deleted"),
    FlagName::Short('t'),
    FlagName::Long(OPT_TAG),
    FlagName::Long(OPT_NAMED),
    FlagName::Short('b'),
    FlagName::Long(OPT_BOOKMARK),
    FlagName::Long(OPT_BRANCH),
];

const JJ_ABORT_LONGS: &[&str] = &["help", "version"];

const WORKING_COPY_REV: &str = "@";
const MUTABLE_ANCESTORS_OF_WORKING_COPY: &str = "::@ & mutable()";

const SUGGEST_DESCRIBE: &str = "jj describe -m '<message>'";
const SUGGEST_COMMIT: &str = "jj commit -m '<message>'";
const SUGGEST_COMMIT_PATHS: &str = "jj commit -m '<message>' <paths>";
const SUGGEST_SPLIT: &str = "jj split -m '<message>' <paths>";
const SUGGEST_SQUASH_PATHS: &str = "jj squash -u <paths>";
const SUGGEST_SQUASH_USE_DEST: &str = "jj squash -u";
const SUGGEST_SQUASH_MESSAGE: &str = "jj squash -m '<message>'";
const SUGGEST_RESTORE_PATHS: &str = "jj restore <paths>";
const SUGGEST_ABSORB_PATHS: &str = "jj absorb <paths>";
const SUGGEST_RESOLVE: &str = "jj resolve --list";

macro_rules! jj_long_values {
    ( $( $extra_long:expr ),* $(,)? ) => {
        &[
            $( $extra_long, )*
            OPT_REPOSITORY,
            "at-operation",
            "at-op",
            "color",
            "config",
            "config-toml",
            "config-file",
        ]
    };
}

macro_rules! jj_spec {
    (shorts: $shorts:literal $(, longs: [ $( $extra_long:expr ),* $(,)? ])? $(,)?) => {
        CliSpec {
            short_values: concat!("R", $shorts),
            short_equals_values: true,
            long_values: jj_long_values!( $( $( $extra_long, )* )? ),
            abort_shorts: "hV",
            abort_longs: JJ_ABORT_LONGS,
            ..CliSpec::DEFAULT
        }
    };
}

const GLOBAL_ONLY_SPEC: CliSpec = jj_spec!(shorts: "");

/// Leading-flag schema for `jj` subcommands (`clap` global options).
pub const JJ_SUBCOMMAND_CLI_SCHEMA: SubcommandCliSchema = SubcommandCliSchema {
    program_name: JJ_PROGRAM,
    pre_subcommand_spec: PreSubcommandSpec {
        short_values: "R",
        long_values: jj_long_values!(),
        abort_shorts: "hV",
        abort_longs: JJ_ABORT_LONGS,
    },
    allows_plus_toolchain: false,
};

const EDIT_SPEC: CliSpec = jj_spec!(shorts: "r", longs: [OPT_REVISION]);

const LOG_SPEC: CliSpec = jj_spec!(
    shorts: "rnT",
    longs: [
        OPT_REVISION,
        OPT_REVISIONS,
        OPT_LIMIT,
        "template",
        OPT_TOOL,
        "context",
    ],
);

const DESCRIBE_SPEC: CliSpec = jj_spec!(
    shorts: "rm",
    longs: [OPT_REVISION, OPT_REVISIONS, OPT_MESSAGE, OPT_AUTHOR],
);

const COMMIT_SPEC: CliSpec = jj_spec!(shorts: "m", longs: [OPT_MESSAGE, OPT_TOOL]);

const SPLIT_SPEC: CliSpec = jj_spec!(
    shorts: "rodABm",
    longs: [
        OPT_REVISION,
        OPT_ONTO,
        OPT_DESTINATION,
        OPT_INSERT_AFTER,
        OPT_AFTER,
        OPT_INSERT_BEFORE,
        OPT_BEFORE,
        OPT_MESSAGE,
        OPT_TOOL,
    ],
);

const SQUASH_SPEC: CliSpec = jj_spec!(
    shorts: "rftodABm",
    longs: [
        OPT_REVISION,
        OPT_FROM,
        OPT_INTO,
        OPT_TO,
        OPT_ONTO,
        OPT_DESTINATION,
        OPT_INSERT_AFTER,
        OPT_AFTER,
        OPT_INSERT_BEFORE,
        OPT_BEFORE,
        OPT_MESSAGE,
        OPT_TOOL,
    ],
);

const DIFFEDIT_SPEC: CliSpec = jj_spec!(
    shorts: "rft",
    longs: [OPT_REVISION, OPT_FROM, OPT_TO, OPT_TOOL],
);

const RESOLVE_SPEC: CliSpec = jj_spec!(shorts: "r", longs: [OPT_REVISION, OPT_TOOL]);

const RESTORE_SPEC: CliSpec = jj_spec!(
    shorts: "ftc",
    longs: [OPT_FROM, OPT_INTO, OPT_TO, OPT_CHANGES_IN, OPT_TOOL],
);

const ABSORB_SPEC: CliSpec = jj_spec!(
    shorts: "ft",
    longs: [OPT_FROM, OPT_INTO, OPT_TO, OPT_TOOL],
);

const ABANDON_SPEC: CliSpec = jj_spec!(shorts: "r", longs: [OPT_REVISION, OPT_REVISIONS]);

const REBASE_SPEC: CliSpec = jj_spec!(
    shorts: "bsrodAB",
    longs: [
        OPT_BRANCH,
        OPT_SOURCE,
        OPT_REVISION,
        OPT_REVISIONS,
        OPT_ONTO,
        OPT_DESTINATION,
        OPT_INSERT_AFTER,
        OPT_AFTER,
        OPT_INSERT_BEFORE,
        OPT_BEFORE,
    ],
);

const FIX_SPEC: CliSpec = jj_spec!(shorts: "s", longs: [OPT_SOURCE]);

const NEW_SPEC: CliSpec = jj_spec!(
    shorts: "rodmAB",
    longs: [
        OPT_MESSAGE,
        OPT_INSERT_AFTER,
        OPT_AFTER,
        OPT_INSERT_BEFORE,
        OPT_BEFORE,
    ],
);

const METAEDIT_SPEC: CliSpec = jj_spec!(
    shorts: "rm",
    longs: [OPT_REVISION, OPT_REVISIONS, OPT_MESSAGE, OPT_AUTHOR, "author-timestamp"],
);

const GIT_PUSH_SPEC: CliSpec = jj_spec!(
    shorts: "btrco",
    longs: [
        "remote",
        OPT_BOOKMARK,
        OPT_BRANCH,
        OPT_TAG,
        OPT_REVISION,
        OPT_REVISIONS,
        OPT_CHANGE,
        OPT_NAMED,
        "option",
    ],
);

/// Repository override parsed from `-R` / `--repository` on a `jj` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjRepository {
    /// No `-R` / `--repository` flag was supplied; inherit the command's working directory.
    Inherited,
    /// A static, non-empty repository path supplied via `-R` / `--repository`.
    Static(PathBuf),
    /// A `-R` / `--repository` flag carried a shell expansion, empty string, or missing operand.
    Unresolvable,
}

/// Whether a Jujutsu invocation launches an interactive text editor, diff selector, or merge tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JjInteractivity {
    /// Syntactically launches an interactive editor or merge session without repository state.
    AlwaysInteractive {
        /// Canonical non-interactive replacement example.
        replacement: &'static str,
    },
    /// Launches `$EDITOR` to combine descriptions when at least two commits in `revset` already
    /// carry non-empty descriptions.
    IfMultipleDescribed {
        /// Union revset of commits whose descriptions Jujutsu would combine.
        revset: String,
        /// Canonical non-interactive replacement example (`-u` for an existing destination, `-m`
        /// when creating a new commit via `-o` / `-A` / `-B`).
        replacement: &'static str,
    },
}

/// Parsed Jujutsu CLI invocation with subcommand-specific option classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjInvocation<'a> {
    subcommand: JjSubcommand,
    args: ParsedCli<'a>,
    repository: JjRepository,
}

impl<'a> JjInvocation<'a> {
    /// Repository override passed via `-R` / `--repository`, or [`JjRepository::Inherited`].
    #[must_use]
    pub const fn repository(&self) -> &JjRepository {
        &self.repository
    }

    /// Target revision of a `jj edit` invocation, defaulting to `"@"` when omitted.
    #[must_use]
    pub fn edit_target_revision(&self) -> Option<&'a str> {
        if self.subcommand != JjSubcommand::Edit {
            return None;
        }
        Some(
            self.args
                .first_value(REV_FLAGS)
                .or_else(|| self.args.positionals().get(1).map(|arg| arg.text.as_str()))
                .unwrap_or(WORKING_COPY_REV),
        )
    }

    /// Suggests a bounded `jj log` command when this invocation is an unbounded `jj log`, or
    /// `None` when it is not `jj log` or is already bounded.
    #[must_use]
    pub fn suggest_bounded_log(&self) -> Option<String> {
        if self.subcommand != JjSubcommand::Log || self.args.has_any_flag(LIMIT_FLAGS) {
            return None;
        }

        let revisions = self.args.values(REVS_FLAGS);
        if revisions.is_empty() {
            let trailing = self.args.positionals().get(1..)?;
            if trailing.is_empty() {
                return None;
            }
            let trailing_paths: Vec<&str> = trailing.iter().map(|arg| arg.text.as_str()).collect();
            return Some(format!(
                "jj log -r 'mutable()' {}",
                trailing_paths.join(" ")
            ));
        }

        let first_unbounded = revisions
            .into_iter()
            .find(|revset| !is_bounded_jj_revset(revset))?;
        let bounded_revset = intersect_revset_with_mutable(first_unbounded);
        Some(format!("jj log -r '{bounded_revset}'"))
    }

    /// Classifies whether this `jj` invocation launches an interactive text, diff, or merge
    /// editor unconditionally or conditionally when combining multiple described commits.
    #[must_use]
    pub fn interactivity(&self) -> Option<JjInteractivity> {
        if self.args.has_flag(FlagName::Long("editor"))
            && let Some(replacement) = self.editor_free_suggestion()
        {
            return Some(JjInteractivity::AlwaysInteractive { replacement });
        }
        if self.subcommand == JjSubcommand::Diffedit {
            return Some(JjInteractivity::AlwaysInteractive {
                replacement: SUGGEST_RESTORE_PATHS,
            });
        }
        if self.subcommand == JjSubcommand::Resolve {
            let is_non_interactive_resolve = self.args.has_any_flag(LIST_FLAGS)
                || matches!(
                    self.args.last_option_value(&[FlagName::Long(OPT_TOOL)]),
                    LastOptionValue::Value {
                        text: ":ours" | ":theirs",
                        is_dynamic: false,
                    }
                );
            return (!is_non_interactive_resolve).then_some(JjInteractivity::AlwaysInteractive {
                replacement: SUGGEST_RESOLVE,
            });
        }
        if self.args.has_any_flag(INTERACTIVE_OR_TOOL_FLAGS)
            && let Some(replacement) = self.non_interactive_diff_suggestion()
        {
            return Some(JjInteractivity::AlwaysInteractive { replacement });
        }
        match self.subcommand {
            JjSubcommand::Describe
                if !self.has_message_flag()
                    && !self
                        .args
                        .has_any_flag(&[FlagName::Long("stdin"), FlagName::Long("no-edit")]) =>
            {
                Some(JjInteractivity::AlwaysInteractive {
                    replacement: SUGGEST_DESCRIBE,
                })
            }
            JjSubcommand::Commit if !self.has_message_flag() => {
                Some(JjInteractivity::AlwaysInteractive {
                    replacement: SUGGEST_COMMIT,
                })
            }
            JjSubcommand::Split
                if !self.has_message_flag() || self.args.positionals().len() <= 1 =>
            {
                Some(JjInteractivity::AlwaysInteractive {
                    replacement: SUGGEST_SPLIT,
                })
            }
            JjSubcommand::Squash => self.squash_combined_description_interactivity(),
            _ => None,
        }
    }

    /// Returns the candidate revset rewritten by this `jj` command, or `None` when the subcommand
    /// does not rewrite existing history.
    #[must_use]
    pub fn rewritten_revset(&self) -> Option<String> {
        match self.subcommand {
            JjSubcommand::Edit => self.edit_target_revision().map(str::to_owned),
            JjSubcommand::Commit => Some(WORKING_COPY_REV.to_owned()),
            JjSubcommand::Describe | JjSubcommand::Abandon | JjSubcommand::Metaedit => {
                Some(self.explicit_or_positional_revisions())
            }
            JjSubcommand::Diffedit => Some(self.diffedit_rewritten_revset()),
            JjSubcommand::Resolve => self.resolve_rewritten_revset(),
            JjSubcommand::New => self.new_rewritten_revset(),
            JjSubcommand::Squash => Some(self.squash_rewritten_revset()),
            JjSubcommand::Split => Some(self.split_rewritten_revset()),
            JjSubcommand::Restore => Some(self.restore_rewritten_revset()),
            JjSubcommand::Absorb => Some(self.absorb_rewritten_revset()),
            JjSubcommand::Fix => Some(self.fix_rewritten_revset()),
            JjSubcommand::Rebase => Some(self.rebase_rewritten_revset()),
            JjSubcommand::Log | JjSubcommand::GitPush | JjSubcommand::Other => None,
        }
    }

    /// Returns the candidate mutable ancestor revset pushed by a `jj git push` invocation, or
    /// `None` when this invocation is not a push or runs in `--dry-run`, delete-only, or tag-only
    /// mode.
    #[must_use]
    pub fn pushed_mutable_revset(&self) -> Option<String> {
        if self.subcommand != JjSubcommand::GitPush || self.args.has_flag(FlagName::Long("dry-run"))
        {
            return None;
        }

        let mut target_terms: Vec<String> = self
            .args
            .values(CHANGE_FLAGS)
            .into_iter()
            .map(str::to_owned)
            .collect();

        for revision in self.args.values(REVS_FLAGS) {
            target_terms.push(format!("({revision}) & (bookmarks() | tags())"));
        }

        for named in self.args.values(&[FlagName::Long(OPT_NAMED)]) {
            if let Some((_, revision)) = named.split_once('=') {
                let trimmed = revision.trim();
                if !trimmed.is_empty() {
                    target_terms.push(trimmed.to_owned());
                }
            }
        }

        for bookmark in self.args.values(BOOKMARK_FLAGS) {
            if let Some(expression) = format_bookmark_revset(bookmark) {
                target_terms.push(expression);
            }
        }

        if self
            .args
            .has_any_flag(&[FlagName::Long("all"), FlagName::Long("tracked")])
        {
            target_terms.push("bookmarks()".to_owned());
        }

        if !target_terms.is_empty() {
            let union = format_revset_terms(&target_terms);
            return Some(format!("::({union}) & mutable()"));
        }

        if self.args.has_any_flag(EXPLICIT_PUSH_SELECTOR_FLAGS) {
            return None;
        }

        Some(MUTABLE_ANCESTORS_OF_WORKING_COPY.to_owned())
    }

    fn squash_combined_description_interactivity(&self) -> Option<JjInteractivity> {
        if self.has_message_flag() || self.args.has_any_flag(SKIP_COMBINED_DESCRIPTION_FLAGS) {
            return None;
        }

        let from_values = self.args.values(FROM_FLAGS);
        if self.has_squash_new_commit_location() {
            return (!from_values.is_empty() && from_values != [WORKING_COPY_REV]).then(|| {
                JjInteractivity::IfMultipleDescribed {
                    revset: join_revset_union(&from_values),
                    replacement: SUGGEST_SQUASH_MESSAGE,
                }
            });
        }

        let into_values = self.args.values(INTO_FLAGS);
        if !from_values.is_empty() || !into_values.is_empty() {
            let sources = if from_values.is_empty() {
                vec![WORKING_COPY_REV]
            } else {
                from_values
            };
            let destinations = if into_values.is_empty() {
                vec![WORKING_COPY_REV]
            } else {
                into_values
            };
            if sources == [WORKING_COPY_REV] && destinations == [WORKING_COPY_REV] {
                return None;
            }
            let mut combined = sources;
            combined.extend(destinations);
            return Some(JjInteractivity::IfMultipleDescribed {
                revset: join_revset_union(&combined),
                replacement: SUGGEST_SQUASH_USE_DEST,
            });
        }

        let source = self.args.first_value(REV_FLAGS).unwrap_or(WORKING_COPY_REV);
        let parent = format_parent_revset(source);
        Some(JjInteractivity::IfMultipleDescribed {
            revset: format!("({source}) | ({parent})"),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    }

    fn explicit_or_positional_revisions(&self) -> String {
        let mut revisions = self.args.values(REVS_FLAGS);
        revisions.extend(
            self.args
                .positionals()
                .iter()
                .skip(1)
                .map(|arg| arg.text.as_str()),
        );
        format_single_or_union(&revisions, WORKING_COPY_REV)
    }

    fn diffedit_rewritten_revset(&self) -> String {
        let to_values = self.args.values(DIFFEDIT_TO_FLAGS);
        if !to_values.is_empty() {
            return format_single_or_union(&to_values, WORKING_COPY_REV);
        }
        let rev_values = self.args.values(REV_FLAGS);
        format_single_or_union(&rev_values, WORKING_COPY_REV)
    }

    fn resolve_rewritten_revset(&self) -> Option<String> {
        if self.args.has_any_flag(LIST_FLAGS) {
            return None;
        }
        let rev_values = self.args.values(REV_FLAGS);
        Some(format_single_or_union(&rev_values, WORKING_COPY_REV))
    }

    fn new_rewritten_revset(&self) -> Option<String> {
        let mut terms = Vec::new();
        self.extend_insert_location_terms(&mut terms);
        (!terms.is_empty()).then(|| format_revset_terms(&terms))
    }

    fn squash_rewritten_revset(&self) -> String {
        let from_values = self.args.values(FROM_FLAGS);
        if self.has_squash_new_commit_location() {
            let mut terms: Vec<String> = if from_values.is_empty() {
                vec![WORKING_COPY_REV.to_owned()]
            } else {
                from_values.into_iter().map(str::to_owned).collect()
            };
            self.extend_insert_location_terms(&mut terms);
            return format_revset_terms(&terms);
        }

        let into_values = self.args.values(INTO_FLAGS);
        if !from_values.is_empty() || !into_values.is_empty() {
            let sources = if from_values.is_empty() {
                vec![WORKING_COPY_REV]
            } else {
                from_values
            };
            let destinations = if into_values.is_empty() {
                vec![WORKING_COPY_REV]
            } else {
                into_values
            };
            let mut combined = sources;
            combined.extend(destinations);
            return join_revset_union(&combined);
        }

        let source = self.args.first_value(REV_FLAGS).unwrap_or(WORKING_COPY_REV);
        let parent = format_parent_revset(source);
        format!("({source}) | ({parent})")
    }

    fn split_rewritten_revset(&self) -> String {
        let rev = self.args.first_value(REV_FLAGS).unwrap_or(WORKING_COPY_REV);
        let mut terms = vec![rev.to_owned()];
        self.extend_insert_location_terms(&mut terms);
        format_revset_terms(&terms)
    }

    fn restore_rewritten_revset(&self) -> String {
        let changes_in = self.args.values(CHANGES_IN_FLAGS);
        if !changes_in.is_empty() {
            return format_single_or_union(&changes_in, WORKING_COPY_REV);
        }
        let into = self.args.values(INTO_FLAGS);
        format_single_or_union(&into, WORKING_COPY_REV)
    }

    fn absorb_rewritten_revset(&self) -> String {
        let from = self
            .args
            .first_value(FROM_FLAGS)
            .unwrap_or(WORKING_COPY_REV);
        let into = self.args.values(INTO_FLAGS);
        if from == WORKING_COPY_REV && into.is_empty() {
            MUTABLE_ANCESTORS_OF_WORKING_COPY.to_owned()
        } else if into.is_empty() {
            format!("::({from}) & mutable()")
        } else {
            let target = format_single_or_union(&into, WORKING_COPY_REV);
            format!("::({from}) & ({target}) & mutable()")
        }
    }

    fn fix_rewritten_revset(&self) -> String {
        let sources = self.args.values(SOURCE_FLAGS);
        if sources.is_empty() {
            "reachable(@, mutable())".to_owned()
        } else {
            format_single_or_union(&sources, WORKING_COPY_REV)
        }
    }

    fn rebase_rewritten_revset(&self) -> String {
        let sources = self.args.values(SOURCE_FLAGS);
        let revisions = self.args.values(REVS_FLAGS);
        let mut branches = self.args.values(BRANCH_FLAGS);
        if sources.is_empty() && revisions.is_empty() && branches.is_empty() {
            branches.push(WORKING_COPY_REV);
        }

        let mut terms: Vec<String> = sources
            .into_iter()
            .chain(revisions)
            .map(str::to_owned)
            .collect();

        if !branches.is_empty() {
            let branch_union = format_single_or_union(&branches, WORKING_COPY_REV);
            if let Some(new_parents) = self.rebase_new_parent_revset() {
                terms.push(format!("(({new_parents})..({branch_union})) & mutable()"));
            } else {
                terms.push(format!("::({branch_union}) & mutable()"));
            }
        }

        self.extend_insert_location_terms(&mut terms);
        format_revset_terms(&terms)
    }

    /// Computes the `new_parent_ids` revset matching Jujutsu's `compute_commit_location`:
    /// `-o`/`-d` takes precedence, then `-A` (even when paired with `-B`), then `({before})-`.
    fn rebase_new_parent_revset(&self) -> Option<String> {
        let onto = self.args.values(ONTO_FLAGS);
        if !onto.is_empty() {
            return Some(format_single_or_union(&onto, WORKING_COPY_REV));
        }
        let after = self.args.values(AFTER_FLAGS);
        if !after.is_empty() {
            return Some(format_single_or_union(&after, WORKING_COPY_REV));
        }
        let before = self.args.values(BEFORE_FLAGS);
        (!before.is_empty())
            .then(|| format_parent_revset(&format_single_or_union(&before, WORKING_COPY_REV)))
    }

    fn extend_insert_location_terms(&self, terms: &mut Vec<String>) {
        let before_values = self.args.values(BEFORE_FLAGS);
        if !before_values.is_empty() {
            for before in before_values {
                terms.push(before.to_owned());
            }
            return;
        }
        for after in self.args.values(AFTER_FLAGS) {
            terms.push(format!("({after})+ & mutable()"));
        }
    }

    fn editor_free_suggestion(&self) -> Option<&'static str> {
        match self.subcommand {
            JjSubcommand::Describe => Some(SUGGEST_DESCRIBE),
            JjSubcommand::Commit => Some(SUGGEST_COMMIT),
            JjSubcommand::Split => Some(SUGGEST_SPLIT),
            JjSubcommand::Squash if self.has_squash_new_commit_location() => {
                Some(SUGGEST_SQUASH_MESSAGE)
            }
            JjSubcommand::Squash => Some(SUGGEST_SQUASH_USE_DEST),
            _ => None,
        }
    }

    const fn non_interactive_diff_suggestion(&self) -> Option<&'static str> {
        match self.subcommand {
            JjSubcommand::Commit => Some(SUGGEST_COMMIT_PATHS),
            JjSubcommand::Split => Some(SUGGEST_SPLIT),
            JjSubcommand::Squash => Some(SUGGEST_SQUASH_PATHS),
            JjSubcommand::Restore => Some(SUGGEST_RESTORE_PATHS),
            JjSubcommand::Absorb => Some(SUGGEST_ABSORB_PATHS),
            _ => None,
        }
    }

    fn has_message_flag(&self) -> bool {
        self.args.has_any_flag(MESSAGE_FLAGS)
    }

    fn has_squash_new_commit_location(&self) -> bool {
        self.args.has_any_flag(SQUASH_NEW_COMMIT_FLAGS)
    }
}

/// Parses `command` as a Jujutsu subcommand invocation using two-stage subcommand dispatch,
/// returning `None` for non-`jj` commands or `--help` / `--version` queries.
#[must_use]
pub fn parse_jj_invocation(command: &InterceptedCommand) -> Option<JjInvocation<'_>> {
    if command.program.is_dynamic() || command.program_base_name() != JJ_PROGRAM {
        return None;
    }

    let subcommand_index = JJ_SUBCOMMAND_CLI_SCHEMA.find_subcommand_index(&command.arguments, 0)?;
    let subcommand_name = command.arguments[subcommand_index].text.as_str();
    let secondary_name = (subcommand_name == SUBCOMMAND_GIT)
        .then(|| {
            JJ_SUBCOMMAND_CLI_SCHEMA
                .find_subcommand_index(&command.arguments, subcommand_index + 1)
                .map(|index| command.arguments[index].text.as_str())
        })
        .flatten();
    let subcommand = JjSubcommand::from_tokens(subcommand_name, secondary_name);
    let args = subcommand.spec().parse(&command.arguments)?;

    let repository = match args.last_option_value(REPO_FLAGS) {
        LastOptionValue::Absent => JjRepository::Inherited,
        LastOptionValue::MissingValue => JjRepository::Unresolvable,
        LastOptionValue::Value { text, is_dynamic } => {
            if is_dynamic || text.is_empty() {
                JjRepository::Unresolvable
            } else {
                JjRepository::Static(PathBuf::from(text))
            }
        }
    };

    Some(JjInvocation {
        subcommand,
        args,
        repository,
    })
}

/// Recognized Jujutsu subcommands inspected by command lint rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JjSubcommand {
    Edit,
    Log,
    Describe,
    Commit,
    Split,
    Squash,
    Diffedit,
    Resolve,
    Restore,
    Absorb,
    Abandon,
    Rebase,
    Fix,
    New,
    Metaedit,
    GitPush,
    Other,
}

impl JjSubcommand {
    fn from_tokens(primary: &str, secondary: Option<&str>) -> Self {
        match (primary, secondary) {
            ("edit", _) => Self::Edit,
            ("log", _) => Self::Log,
            ("describe" | "desc", _) => Self::Describe,
            ("commit" | "ci", _) => Self::Commit,
            ("split", _) => Self::Split,
            ("squash", _) => Self::Squash,
            ("diffedit", _) => Self::Diffedit,
            ("resolve", _) => Self::Resolve,
            ("restore", _) => Self::Restore,
            ("absorb", _) => Self::Absorb,
            ("abandon", _) => Self::Abandon,
            ("rebase", _) => Self::Rebase,
            ("fix", _) => Self::Fix,
            ("new", _) => Self::New,
            ("metaedit", _) => Self::Metaedit,
            (SUBCOMMAND_GIT, Some(SUBCOMMAND_PUSH)) => Self::GitPush,
            _ => Self::Other,
        }
    }

    const fn spec(self) -> &'static CliSpec {
        match self {
            Self::Edit => &EDIT_SPEC,
            Self::Log => &LOG_SPEC,
            Self::Describe => &DESCRIBE_SPEC,
            Self::Commit => &COMMIT_SPEC,
            Self::Split => &SPLIT_SPEC,
            Self::Squash => &SQUASH_SPEC,
            Self::Diffedit => &DIFFEDIT_SPEC,
            Self::Resolve => &RESOLVE_SPEC,
            Self::Restore => &RESTORE_SPEC,
            Self::Absorb => &ABSORB_SPEC,
            Self::Abandon => &ABANDON_SPEC,
            Self::Rebase => &REBASE_SPEC,
            Self::Fix => &FIX_SPEC,
            Self::New => &NEW_SPEC,
            Self::Metaedit => &METAEDIT_SPEC,
            Self::GitPush => &GIT_PUSH_SPEC,
            Self::Other => &GLOBAL_ONLY_SPEC,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::edit_positional("jj edit kpqxywon", Some("kpqxywon"), JjRepository::Inherited)]
    #[case::edit_default_working_copy("jj edit", Some("@"), JjRepository::Inherited)]
    #[case::edit_short_rev_and_repo(
        "jj edit -R ../ws2 -r main",
        Some("main"),
        JjRepository::Static(PathBuf::from("../ws2"))
    )]
    #[case::edit_leading_repo_flag(
        "jj --repository=/abs/ws edit @-",
        Some("@-"),
        JjRepository::Static(PathBuf::from("/abs/ws"))
    )]
    #[case::last_repo_flag_wins_across_spellings(
        "jj --repository /first edit -R /second @",
        Some("@"),
        JjRepository::Static(PathBuf::from("/second"))
    )]
    #[case::expanded_repo_flag_unresolvable(
        r#"jj -R "$OTHER_WS" edit @"#,
        Some("@"),
        JjRepository::Unresolvable
    )]
    #[case::tilde_repo_flag_unresolvable(
        "jj -R ~/other edit @",
        Some("@"),
        JjRepository::Unresolvable
    )]
    #[case::empty_repo_flag_unresolvable(
        r#"jj -R "" edit @"#,
        Some("@"),
        JjRepository::Unresolvable
    )]
    #[case::dangling_repo_flag_unresolvable("jj edit @ -R", Some("@"), JjRepository::Unresolvable)]
    #[case::repo_flag_after_terminator_ignored(
        "jj status -- -R /other",
        None,
        JjRepository::Inherited
    )]
    #[case::non_edit_subcommand(
        "jj status -R ../ws2",
        None,
        JjRepository::Static(PathBuf::from("../ws2"))
    )]
    #[case::edit_dynamic_lone_dash_flag_ignored(
        r#"jj edit -"$FLAG""#,
        Some("@"),
        JjRepository::Inherited
    )]
    #[case::edit_dynamic_double_dash_flag_ignored(
        r#"jj edit --"$FLAG""#,
        Some("@"),
        JjRepository::Inherited
    )]
    fn test_parse_jj_edit_and_repository(
        #[case] raw: &str,
        #[case] expected_edit_rev: Option<&str>,
        #[case] expected_repo: JjRepository,
    ) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(invocation.edit_target_revision(), expected_edit_rev);
        assert_eq!(invocation.repository(), &expected_repo);
    }

    #[rstest::rstest]
    #[case::unbounded_ancestors("jj log -r ::@", Some("jj log -r '::@ & mutable()'"))]
    #[case::summary_flag_does_not_swallow_fileset(
        "jj log -s src/lib.rs",
        Some("jj log -r 'mutable()' src/lib.rs")
    )]
    #[case::bounded_with_limit("jj log -s -n 5 src/lib.rs", None)]
    #[case::bare_log_allowed("jj log -s", None)]
    fn test_suggest_bounded_log_preserves_boolean_summary_flag(
        #[case] raw: &str,
        #[case] expected_suggestion: Option<&str>,
    ) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(
            invocation.suggest_bounded_log().as_deref(),
            expected_suggestion
        );
    }

    #[rstest::rstest]
    #[case::describe_missing_message(
        "jj describe -r @-",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_DESCRIBE })
    )]
    #[case::describe_no_edit_allowed("jj describe --no-edit --reset-author", None)]
    #[case::desc_alias_editor(
        "jj desc -m feat --editor",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_DESCRIBE })
    )]
    #[case::commit_missing_message(
        "jj commit src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_COMMIT })
    )]
    #[case::commit_editor(
        "jj commit -m feat --editor",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_COMMIT })
    )]
    #[case::commit_interactive(
        "jj commit -i -m feat",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_COMMIT_PATHS })
    )]
    #[case::split_missing_fileset(
        "jj split -m feat",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SPLIT })
    )]
    #[case::split_missing_message(
        "jj split src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SPLIT })
    )]
    #[case::split_editor(
        "jj split -m feat --editor src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SPLIT })
    )]
    #[case::split_interactive(
        "jj split -i -m feat src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SPLIT })
    )]
    #[case::split_diff_tool(
        "jj split --tool :builtin -m feat src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SPLIT })
    )]
    #[case::squash_editor_into_existing(
        "jj squash -u --editor",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SQUASH_USE_DEST })
    )]
    #[case::squash_editor_onto_new(
        "jj squash -f a -o main --editor",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SQUASH_MESSAGE })
    )]
    #[case::squash_interactive(
        "jj squash -i",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_SQUASH_PATHS })
    )]
    #[case::restore_non_interactive_allowed("jj restore src/lib.rs", None)]
    #[case::restore_tool(
        "jj restore --tool meld",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_RESTORE_PATHS })
    )]
    #[case::absorb_non_interactive_allowed("jj absorb", None)]
    #[case::absorb_interactive(
        "jj absorb --interactive",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_ABSORB_PATHS })
    )]
    #[case::diffedit(
        "jj diffedit -r @",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_RESTORE_PATHS })
    )]
    #[case::resolve_interactive(
        "jj resolve src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_RESOLVE })
    )]
    #[case::resolve_editor_tool_flagged(
        "jj resolve --tool :editor src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_RESOLVE })
    )]
    #[case::resolve_ours_overridden_by_meld(
        "jj resolve --tool :ours --tool meld src/lib.rs",
        Some(JjInteractivity::AlwaysInteractive { replacement: SUGGEST_RESOLVE })
    )]
    #[case::resolve_ours_allowed("jj resolve --tool :ours src/lib.rs", None)]
    #[case::diff_tool_formatter_allowed("jj diff --tool git", None)]
    #[case::new_message_editor_text_allowed("jj new -m --editor", None)]
    #[case::file_search_pattern_short_i_allowed("jj file search -p -i", None)]
    #[case::piper_findreviewers_short_i_allowed("jj piper findreviewers -i alice", None)]
    #[case::bare_squash(
        "jj squash",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(@) | (@-)".to_owned(),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    )]
    #[case::squash_long_revision(
        "jj squash --revision empty_src",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(empty_src) | (empty_src-)".to_owned(),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    )]
    #[case::squash_range_rev(
        "jj squash -r 'main..@'",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(main..@) | ((main..@)-)".to_owned(),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    )]
    #[case::squash_short_into(
        "jj squash -t @--",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(@) | (@--)".to_owned(),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    )]
    #[case::squash_to_alias(
        "jj squash --to @--",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(@) | (@--)".to_owned(),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    )]
    #[case::squash_from_only(
        "jj squash --from @-",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(@-) | (@)".to_owned(),
            replacement: SUGGEST_SQUASH_USE_DEST,
        })
    )]
    #[case::squash_from_self_noop("jj squash --from @", None)]
    #[case::squash_onto_default_single_source("jj squash -o main", None)]
    #[case::squash_onto_explicit_working_copy_single_source("jj squash -f @ -o main", None)]
    #[case::squash_onto_single_flag_multi_commit_revset(
        "jj squash -f 'a | b' -o main",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(a | b)".to_owned(),
            replacement: SUGGEST_SQUASH_MESSAGE,
        })
    )]
    #[case::squash_insert_after_multi_source(
        "jj squash -f a -f b -A main",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(a) | (b)".to_owned(),
            replacement: SUGGEST_SQUASH_MESSAGE,
        })
    )]
    #[case::squash_insert_before_multi_source(
        "jj squash -f a -f b --insert-before main",
        Some(JjInteractivity::IfMultipleDescribed {
            revset: "(a) | (b)".to_owned(),
            replacement: SUGGEST_SQUASH_MESSAGE,
        })
    )]
    #[case::squash_use_dest_exempt("jj squash -u", None)]
    #[case::squash_keep_emptied_exempt("jj squash -k", None)]
    fn test_jj_interactivity(#[case] raw: &str, #[case] expected: Option<JjInteractivity>) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(invocation.interactivity(), expected);
    }

    #[rstest::rstest]
    #[case::edit("jj edit kpqxywon", Some("kpqxywon"))]
    #[case::commit("jj commit -m feat", Some("@"))]
    #[case::describe_default("jj describe -m feat", Some("@"))]
    #[case::describe_multi("jj desc -r a b -m feat", Some("(a) | (b)"))]
    #[case::metaedit("jj metaedit -r kpqxywon --update-author", Some("kpqxywon"))]
    #[case::abandon("jj abandon kpqxywon", Some("kpqxywon"))]
    #[case::diffedit_default("jj diffedit", Some("@"))]
    #[case::diffedit_to("jj diffedit --to kpqxywon", Some("kpqxywon"))]
    #[case::resolve_ours("jj resolve --tool :ours -r kpqxywon", Some("kpqxywon"))]
    #[case::resolve_list_exempt("jj resolve --list -r kpqxywon", None)]
    #[case::new_plain_exempt("jj new main", None)]
    #[case::new_insert_after("jj new -A main -m 'fixup!'", Some("(main)+ & mutable()"))]
    #[case::new_insert_before("jj new -B target -m 'feat: prep'", Some("target"))]
    #[case::new_insert_between("jj new -A main -B target", Some("target"))]
    #[case::squash_default("jj squash -u", Some("(@) | (@-)"))]
    #[case::squash_revision("jj squash -r kpqxywon", Some("(kpqxywon) | (kpqxywon-)"))]
    #[case::squash_from_default_dest("jj squash -f @-", Some("(@-) | (@)"))]
    #[case::squash_into("jj squash -u --into @--", Some("(@) | (@--)"))]
    #[case::squash_before_default_source("jj squash -B main -m feat", Some("(@) | (main)"))]
    #[case::squash_between_does_not_expand_after(
        "jj squash -f @ -A main -B target -m feat",
        Some("(@) | (target)")
    )]
    #[case::split_default("jj split -m part1 src/a.rs", Some("@"))]
    #[case::split_after(
        "jj split -r @- -A main -m part1 src/a.rs",
        Some("(@-) | ((main)+ & mutable())")
    )]
    #[case::restore_default("jj restore src/a.rs", Some("@"))]
    #[case::restore_into("jj restore --into @- src/a.rs", Some("@-"))]
    #[case::restore_changes_in("jj restore -c kpqxywon", Some("kpqxywon"))]
    #[case::absorb_default("jj absorb", Some("::@ & mutable()"))]
    #[case::absorb_custom_from("jj absorb -f @-", Some("::(@-) & mutable()"))]
    #[case::absorb_custom_into(
        "jj absorb -f @- -t main..@",
        Some("::(@-) & (main..@) & mutable()")
    )]
    #[case::fix_default("jj fix", Some("reachable(@, mutable())"))]
    #[case::fix_source("jj fix -s @-", Some("@-"))]
    #[case::rebase_source("jj rebase -s feat -o main", Some("feat"))]
    #[case::rebase_branch_default("jj rebase -o main", Some("((main)..(@)) & mutable()"))]
    #[case::rebase_insert_after(
        "jj rebase -r feat -A main",
        Some("(feat) | ((main)+ & mutable())")
    )]
    #[case::rebase_insert_between_revision(
        "jj rebase -r feat -A main -B target",
        Some("(feat) | (target)")
    )]
    #[case::rebase_insert_between_branch(
        "jj rebase -b @ -A main -B target",
        Some("(((main)..(@)) & mutable()) | (target)")
    )]
    #[case::rebase_insert_before_branch(
        "jj rebase -b @ -B target",
        Some("(((target-)..(@)) & mutable()) | (target)")
    )]
    #[case::read_only_log("jj log", None)]
    fn test_rewritten_revset(#[case] raw: &str, #[case] expected: Option<&str>) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(invocation.rewritten_revset().as_deref(), expected);
    }

    #[rstest::rstest]
    #[case::bare_push("jj git push", Some("::@ & mutable()"))]
    #[case::repo_flag_between_git_and_push("jj git -R ../ws2 push", Some("::@ & mutable()"))]
    #[case::short_change("jj git push -c kpqxywon", Some("::(kpqxywon) & mutable()"))]
    #[case::long_change("jj git push --change kpqxywon", Some("::(kpqxywon) & mutable()"))]
    #[case::short_revision(
        "jj git push -r 'mutable()'",
        Some("::((mutable()) & (bookmarks() | tags())) & mutable()")
    )]
    #[case::long_revision(
        "jj git push --revision feat",
        Some("::((feat) & (bookmarks() | tags())) & mutable()")
    )]
    #[case::plural_revisions(
        "jj git push --revisions main..@",
        Some("::((main..@) & (bookmarks() | tags())) & mutable()")
    )]
    #[case::named_bookmark("jj git push --named feat=@-", Some("::(@-) & mutable()"))]
    #[case::named_empty_revision_ignored("jj git push --named feat=", None)]
    #[case::named_missing_equals_ignored("jj git push --named broken", None)]
    #[case::short_bookmark_glob("jj git push -b feat-*", Some("::(bookmarks(feat-*)) & mutable()"))]
    #[case::long_bookmark_union(
        "jj git push --bookmark 'a | b'",
        Some("::(bookmarks(a | b)) & mutable()")
    )]
    #[case::bookmark_negation("jj git push -b '~main'", Some("::(bookmarks(~main)) & mutable()"))]
    #[case::bookmark_exact_prefix(
        "jj git push -b exact:main",
        Some("::(bookmarks(exact:main)) & mutable()")
    )]
    #[case::branch_alias("jj git push --branch feat", Some("::(bookmarks(feat)) & mutable()"))]
    #[case::empty_short_bookmark_ignored(r#"jj git push -b """#, None)]
    #[case::empty_long_bookmark_ignored(r#"jj git push --bookmark """#, None)]
    #[case::empty_branch_alias_ignored(r#"jj git push --branch """#, None)]
    #[case::all_bookmarks("jj git push --all", Some("::(bookmarks()) & mutable()"))]
    #[case::tracked_bookmarks("jj git push --tracked", Some("::(bookmarks()) & mutable()"))]
    #[case::deleted_only_exempt("jj git push --deleted", None)]
    #[case::deleted_with_tracked(
        "jj git push --deleted --tracked",
        Some("::(bookmarks()) & mutable()")
    )]
    #[case::short_tag_only_exempt("jj git push -t v1.0.0", None)]
    #[case::long_tag_only_exempt("jj git push --tag v1.0.0", None)]
    #[case::tag_with_change("jj git push -t v1.0.0 -c kpqxywon", Some("::(kpqxywon) & mutable()"))]
    #[case::multiple_targets(
        "jj git push -c kpqxywon -b feat",
        Some("::((kpqxywon) | (bookmarks(feat))) & mutable()")
    )]
    #[case::dry_run_exempt("jj git push --dry-run", None)]
    #[case::git_fetch_exempt("jj git fetch", None)]
    #[case::git_remote_add_push_exempt("jj git remote add push https://example.com/repo.git", None)]
    #[case::git_fetch_remote_named_push_exempt("jj git fetch --remote push", None)]
    #[case::non_git_subcommand_exempt("jj status", None)]
    fn test_pushed_mutable_revset(#[case] raw: &str, #[case] expected: Option<&str>) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(invocation.pushed_mutable_revset().as_deref(), expected);
    }

    #[test]
    fn test_git_push_parses_repository_flag_between_subcommand_tokens() {
        let command = InterceptedCommand::parse_all("jj git -R ../ws2 push").remove(0);
        let invocation = parse_jj_invocation(&command).expect("valid jj invocation");
        assert_eq!(
            invocation.repository(),
            &JjRepository::Static(PathBuf::from("../ws2"))
        );
    }

    #[rstest::rstest]
    #[case::non_jj("git status")]
    #[case::top_level_help("jj --help")]
    #[case::subcommand_help("jj edit --help")]
    #[case::git_help("jj git --help")]
    #[case::git_push_help("jj git push --help")]
    #[case::clustered_short_version("jj -qV status")]
    #[case::flags_only("jj -R /tmp/ws --no-pager")]
    fn test_parse_jj_invocation_skips_non_subcommands(#[case] raw: &str) {
        let command = InterceptedCommand::parse_all(raw).remove(0);
        assert_eq!(parse_jj_invocation(&command), None);
    }
}
