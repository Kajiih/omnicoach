//! Declarative CLI schemas for shell built-in wrappers and working-directory transitions.

use super::cli_args::{CliSpec, FlagName, OptionOrder};
use super::model::CommandArg;

const BUILTIN_WRAPPER: &str = "builtin";
const COMMAND_WRAPPER: &str = "command";
const SINGLE_DASH: &str = "-";

const BUILTIN_SPEC: CliSpec = CliSpec {
    order: OptionOrder::StopAtFirstPositional,
    abort_shorts: "h",
    abort_longs: &["help"],
    ..CliSpec::DEFAULT
};

const COMMAND_SPEC: CliSpec = CliSpec {
    order: OptionOrder::StopAtFirstPositional,
    abort_shorts: "vVh",
    abort_longs: &["help"],
    ..CliSpec::DEFAULT
};

const CD_SPEC: CliSpec = CliSpec {
    order: OptionOrder::StopAtFirstPositional,
    ..CliSpec::DEFAULT
};

/// Working-directory transition introduced by a shell command (`cd`, `pushd`, or `popd`).
pub(super) enum CdEffect {
    /// Command does not alter the shell working directory.
    None,
    /// Static directory target passed to `cd`.
    Static(String),
    /// Dynamic or environment-dependent jump (`cd`, `cd -`, `cd "$VAR"`, `pushd`, `popd`, or
    /// malformed flag cluster) that invalidates static directory tracking.
    Unknown,
}

/// Classifies the working-directory effect of a shell command (`cd`, `builtin cd`, `command cd`,
/// `pushd`, or `popd`).
#[must_use]
pub(super) fn classify_cd_effect(program: &CommandArg, arguments: &[CommandArg]) -> CdEffect {
    let Some(peeled) = peel_shell_builtin_wrappers(program, arguments) else {
        return CdEffect::None;
    };
    if peeled.program.text.contains('/') {
        return CdEffect::None;
    }
    let cd_args = match peeled.program.text.as_str() {
        "cd" => peeled.arguments,
        "pushd" | "popd" => return CdEffect::Unknown,
        _ => return CdEffect::None,
    };

    let Some(parsed) = CD_SPEC.parse(cd_args) else {
        return CdEffect::Unknown;
    };
    if !parsed
        .occurrences()
        .iter()
        .all(|occ| matches!(occ.flag, FlagName::Short('L' | 'P' | 'e' | '@')))
    {
        return CdEffect::Unknown;
    }

    let [chosen] = parsed.positionals() else {
        return CdEffect::Unknown;
    };
    if chosen.is_dynamic()
        || chosen.text.is_empty()
        || chosen.text == SINGLE_DASH
        || (chosen.text.starts_with('-') && !parsed.has_double_dash_before_positional(0))
    {
        CdEffect::Unknown
    } else {
        CdEffect::Static(chosen.text.clone())
    }
}

/// Unwrapped shell builtin target after peeling `builtin` and `command` prefixes.
struct PeeledBuiltin<'a> {
    program: &'a CommandArg,
    arguments: &'a [CommandArg],
}

/// Peels zero or more `builtin` / `command` (non-`-v`/`-V`, non-path-qualified) shell builtin
/// wrappers so `builtin cd` and `command -p cd` update the current shell's working directory.
#[must_use]
fn peel_shell_builtin_wrappers<'a>(
    mut program: &'a CommandArg,
    mut arguments: &'a [CommandArg],
) -> Option<PeeledBuiltin<'a>> {
    loop {
        if program.is_dynamic() {
            return None;
        }
        if program.text.contains('/') {
            return Some(PeeledBuiltin { program, arguments });
        }
        let (spec, allowed_short) = match program.text.as_str() {
            BUILTIN_WRAPPER => (&BUILTIN_SPEC, None),
            COMMAND_WRAPPER => (&COMMAND_SPEC, Some('p')),
            _ => return Some(PeeledBuiltin { program, arguments }),
        };
        let parsed = spec.parse(arguments)?;
        let valid_flags = parsed
            .occurrences()
            .iter()
            .all(|occ| allowed_short.is_some_and(|flag| occ.flag == FlagName::Short(flag)));
        if !valid_flags {
            return None;
        }
        let pos_index = parsed.positional_arg_index(0)?;
        let first_pos = &arguments[pos_index];
        if first_pos.text.starts_with('-') && !parsed.has_double_dash_before_positional(0) {
            return None;
        }
        program = first_pos;
        arguments = &arguments[pos_index + 1..];
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParsedCommand;
    use super::*;

    #[rstest::rstest]
    #[case::malformed_double_dash_equals("cd --=foo dir")]
    #[case::single_malformed_double_dash_equals("cd --=foo")]
    #[case::dynamic_long_flag_prefix(r#"cd --foo"$VAR" dir"#)]
    #[case::dynamic_short_flag_tail(r#"cd -L"$VAR" dir"#)]
    #[case::unknown_short_flag("cd -x dir")]
    #[case::unknown_long_flag("cd --unknown dir")]
    #[case::extra_positional_operand("cd dir1 dir2")]
    #[case::bare_cd("cd")]
    #[case::cd_dash("cd -")]
    #[case::cd_after_terminator_dash("cd -- -")]
    fn test_classify_cd_effect_returns_unknown_on_invalid_or_malformed_flags(#[case] raw: &str) {
        let parsed = ParsedCommand::new(raw);
        let cmd = &parsed.commands()[0];
        assert!(matches!(
            classify_cd_effect(&cmd.program, &cmd.arguments),
            CdEffect::Unknown
        ));
    }

    #[rstest::rstest]
    #[case::builtin_unknown_short_flag("builtin -x cd sub")]
    #[case::command_unknown_long_flag("command --unknown cd sub")]
    #[case::command_malformed_double_dash_equals("command --=foo cd sub")]
    #[case::command_dash_prefixed_target_without_double_dash("command - cd")]
    #[case::builtin_dash_prefixed_target_without_double_dash("builtin - cd")]
    fn test_peel_shell_builtin_wrappers_rejects_invalid_or_unknown_flags(#[case] raw: &str) {
        let parsed = ParsedCommand::new(raw);
        let cmd = &parsed.commands()[0];
        assert!(peel_shell_builtin_wrappers(&cmd.program, &cmd.arguments).is_none());
    }

    #[rstest::rstest]
    #[case::command_double_dash_dash_prefixed("command -- -cmd")]
    #[case::builtin_double_dash_dash_prefixed("builtin -- -cmd")]
    fn test_peel_shell_builtin_wrappers_allows_dash_prefixed_after_double_dash(#[case] raw: &str) {
        let parsed = ParsedCommand::new(raw);
        let cmd = &parsed.commands()[0];
        let peeled = peel_shell_builtin_wrappers(&cmd.program, &cmd.arguments)
            .expect("expected `--` to allow dash-prefixed target");
        assert_eq!(peeled.program.text, "-cmd");
        assert_eq!(peeled.arguments, []);
    }

    #[rstest::rstest]
    #[case::plain_cd("cd sub/dir", "sub/dir")]
    #[case::cd_flags_and_double_dash("cd -LP -- sub/dir", "sub/dir")]
    #[case::cd_double_dash_allows_dash_prefixed_dir("cd -- -dir", "-dir")]
    #[case::command_p_cd("command -p cd sub/dir", "sub/dir")]
    fn test_classify_cd_effect_returns_static_target(#[case] raw: &str, #[case] expected: &str) {
        let parsed = ParsedCommand::new(raw);
        let cmd = &parsed.commands()[0];
        let CdEffect::Static(actual) = classify_cd_effect(&cmd.program, &cmd.arguments) else {
            panic!("expected CdEffect::Static for `{raw}`");
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_command_v_cd_does_not_alter_cwd() {
        let parsed = ParsedCommand::new("command -v cd");
        let cmd = &parsed.commands()[0];
        assert!(matches!(
            classify_cd_effect(&cmd.program, &cmd.arguments),
            CdEffect::None
        ));
    }
}
