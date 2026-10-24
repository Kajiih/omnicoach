//! The intercepted command parser: shell input parsed into [`ParsedCommand`] and
//! [`InterceptedCommand`] invocations, plus CLI argument classification ([`ParsedArgs`],
//! [`ProgramCliSchema`]).

architecture_component!(CommandLintCommand);

mod cli_args;
mod shell;

pub use self::cli_args::{ParsedArgs, ProgramCliSchema};
pub use self::shell::{
    CommandArg, CommandRedirect, EnvAssignment, InterceptedCommand, ParsedCommand, RedirectKind,
    RedirectTarget, ShellScope,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::SourceSpan;

    #[derive(Debug, PartialEq, Eq)]
    struct CommandSummary<'a> {
        program: &'a str,
        args: Vec<&'a str>,
        span: SourceSpan,
        scopes: &'a [ShellScope],
    }

    fn summarize(cmd: &InterceptedCommand) -> CommandSummary<'_> {
        CommandSummary {
            program: cmd.program_name(),
            args: cmd.arguments.iter().map(|arg| arg.text.as_str()).collect(),
            span: cmd.span,
            scopes: &cmd.enclosing_scopes,
        }
    }

    #[test]
    fn test_parse_single_command() {
        let parsed = ParsedCommand::new("jj edit @-");
        let summaries: Vec<_> = parsed.commands().iter().map(summarize).collect();
        assert!(!parsed.has_syntax_error());
        assert_eq!(
            summaries,
            vec![CommandSummary {
                program: "jj",
                args: vec!["edit", "@-"],
                span: SourceSpan::new(0, 10),
                scopes: &[],
            }]
        );
    }

    #[test]
    fn test_parse_compound_commands() {
        let cmds = InterceptedCommand::parse_all("jj edit @- && jj describe");
        let summaries: Vec<_> = cmds.iter().map(summarize).collect();
        assert_eq!(
            summaries,
            vec![
                CommandSummary {
                    program: "jj",
                    args: vec!["edit", "@-"],
                    span: SourceSpan::new(0, 10),
                    scopes: &[ShellScope::ChainedList],
                },
                CommandSummary {
                    program: "jj",
                    args: vec!["describe"],
                    span: SourceSpan::new(14, 25),
                    scopes: &[ShellScope::ChainedList],
                },
            ]
        );
    }

    #[test]
    fn test_parse_nested_subshell() {
        let cmds = InterceptedCommand::parse_all("(cd dir && jj edit @-)");
        let summaries: Vec<_> = cmds.iter().map(summarize).collect();
        assert_eq!(
            summaries,
            vec![
                CommandSummary {
                    program: "cd",
                    args: vec!["dir"],
                    span: SourceSpan::new(1, 7),
                    scopes: &[ShellScope::Subshell, ShellScope::ChainedList],
                },
                CommandSummary {
                    program: "jj",
                    args: vec!["edit", "@-"],
                    span: SourceSpan::new(11, 21),
                    scopes: &[ShellScope::Subshell, ShellScope::ChainedList],
                },
            ]
        );
    }

    #[test]
    fn test_redirects_excluded_from_arguments_and_included_in_span() {
        let cmds = InterceptedCommand::parse_all("jj edit > out.txt 2>&1");
        let cmd = &cmds[0];
        let redirects: Vec<_> = cmd
            .redirects
            .iter()
            .map(|redirect| (redirect.fd, redirect.kind, redirect.span))
            .collect();
        assert_eq!(
            summarize(cmd),
            CommandSummary {
                program: "jj",
                args: vec!["edit"],
                span: SourceSpan::new(0, 22),
                scopes: &[],
            }
        );
        assert_eq!(
            redirects,
            vec![
                (None, RedirectKind::Write, SourceSpan::new(8, 17)),
                (
                    Some(2),
                    RedirectKind::DuplicateOutput,
                    SourceSpan::new(18, 22)
                ),
            ]
        );
    }

    #[test]
    fn test_env_assignments_separated_from_arguments() {
        let cmds = InterceptedCommand::parse_all("JJ_USER=alice jj edit @-");
        let cmd = &cmds[0];
        let envs: Vec<_> = cmd
            .env_assignments
            .iter()
            .map(|env| (env.name.as_str(), env.value.text.as_str(), env.span))
            .collect();
        assert_eq!(
            summarize(cmd),
            CommandSummary {
                program: "jj",
                args: vec!["edit", "@-"],
                span: SourceSpan::new(0, 24),
                scopes: &[],
            }
        );
        assert_eq!(envs, vec![("JJ_USER", "alice", SourceSpan::new(0, 13))]);
    }

    #[test]
    fn test_command_substitution_inside_double_quotes_preserves_outer_byte_span() {
        let raw = r#"echo "result: $(jj edit d123)""#;
        let cmds = InterceptedCommand::parse_all(raw);
        let inner = &cmds[1];
        assert_eq!(
            summarize(inner),
            CommandSummary {
                program: "jj",
                args: vec!["edit", "d123"],
                span: SourceSpan::new(16, 28),
                scopes: &[ShellScope::CommandSubstitution],
            }
        );
        assert_eq!(&raw[inner.span.start..inner.span.end], "jj edit d123");
    }

    #[test]
    fn test_non_ascii_prefix_maps_to_utf8_byte_offsets() {
        let raw = r#"echo "héllo" && jj edit d123"#;
        let cmds = InterceptedCommand::parse_all(raw);
        assert_eq!(cmds.len(), 2);
        assert_eq!(&raw[cmds[1].span.start..cmds[1].span.end], "jj edit d123");
    }

    #[test]
    fn test_syntax_error_reported_on_unterminated_quote() {
        let parsed = ParsedCommand::new(r#"jj edit "unterminated"#);
        assert!(parsed.has_syntax_error());
    }

    #[test]
    fn test_parse_args_basic() {
        let cmd = InterceptedCommand::parse_all("jj edit -R . --config myconf @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R", "--repository", "--config"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(args.get_option("-R"), Some("."));
        assert_eq!(args.get_option("--config"), Some("myconf"));
        assert!(args.has_subcommand_sequence(&["edit"]));
    }

    #[test]
    fn test_parse_args_boolean_long_flag_and_stdin_dash() {
        let cmd = InterceptedCommand::parse_all("jj --no-pager edit -").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "-"]);
        assert!(args.has_flag("--no-pager"));
    }

    #[test]
    fn test_parse_args_equals_syntax() {
        let cmd = InterceptedCommand::parse_all("jj edit --repository=. @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R", "--repository"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(args.get_option("--repository"), Some("."));
    }

    #[test]
    fn test_parse_args_posix_clustering() {
        let cmd = InterceptedCommand::parse_all("jj edit -am").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &[],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit"]);
        assert!(args.has_flag("-a"));
        assert!(args.has_flag("-m"));
    }

    #[test]
    fn test_parse_args_posix_glued_value() {
        let cmd = InterceptedCommand::parse_all("jj edit -R. @-").remove(0);
        let schema = ProgramCliSchema {
            program_name: "jj",
            options_with_values: &["-R"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["edit", "@-"]);
        assert_eq!(args.get_option("-R"), Some("."));
    }

    #[test]
    fn test_parse_args_posix_cluster_with_value() {
        let cmd = InterceptedCommand::parse_all("git commit -am msg").remove(0);
        let schema = ProgramCliSchema {
            program_name: "git",
            options_with_values: &["-m"],
        };
        let args = cmd.parse_args(&schema);
        assert_eq!(args.positionals, vec!["commit"]);
        assert!(args.has_flag("-a"));
        assert_eq!(args.get_option("-m"), Some("msg"));
    }

    #[test]
    fn test_parse_args_posix_terminator() {
        let cmd = InterceptedCommand::parse_all("git log -- -R").remove(0);
        let schema = ProgramCliSchema {
            program_name: "git",
            options_with_values: &["-R"],
        };
        let args_term = cmd.parse_args(&schema);
        assert_eq!(args_term.positionals, vec!["log", "-R"]);
        assert!(!args_term.has_flag("-R"));
        assert_eq!(args_term.get_option("-R"), None);
    }

    #[rstest::rstest]
    #[case::for_loop(
        "for rev in a b; do jj edit d123; done",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::if_else(
        "if true; then jj status; else jj edit d123; fi",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::while_loop(
        "while false; do jj edit d123; done",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::case_clause(
        "case $x in a) jj edit d123 ;; esac",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::brace_group("{ jj edit d123; }", "jj edit d123", ShellScope::CompoundBlock)]
    #[case::function_def(
        "run_it() { jj edit d123; }",
        "jj edit d123",
        ShellScope::CompoundBlock
    )]
    #[case::backtick_substitution(
        "echo `jj edit d123`",
        "jj edit d123",
        ShellScope::CommandSubstitution
    )]
    #[case::process_substitution(
        "diff <(jj edit d123) out.txt",
        "jj edit d123",
        ShellScope::ProcessSubstitution
    )]
    #[case::background_job("jj edit d123 &", "jj edit d123", ShellScope::Background)]
    fn test_compound_and_nested_scopes(
        #[case] script: &str,
        #[case] expected_slice: &str,
        #[case] expected_scope: ShellScope,
    ) {
        let parsed = ParsedCommand::new(script);
        let matched = parsed
            .commands()
            .iter()
            .find(|cmd| cmd.program_name() == "jj" && cmd.arguments[0].text == "edit")
            .expect("expected `jj edit` command to be extracted");
        assert_eq!(
            &parsed.source()[matched.span.start..matched.span.end],
            expected_slice
        );
        assert!(matched.enclosing_scopes.contains(&expected_scope));
    }

    #[rstest::rstest]
    #[case::append("jj log >> out.txt", RedirectKind::Append, ">> out.txt")]
    #[case::clobber("jj log >| out.txt", RedirectKind::Clobber, ">| out.txt")]
    #[case::output_and_error("jj log &> out.txt", RedirectKind::OutputAndError, "&> out.txt")]
    #[case::append_output_and_error(
        "jj log &>> out.txt",
        RedirectKind::AppendOutputAndError,
        "&>> out.txt"
    )]
    #[case::read_input("cat < in.txt", RedirectKind::Read, "< in.txt")]
    #[case::here_string("cat <<< 'hello'", RedirectKind::HereString, "<<< 'hello'")]
    #[case::duplicate_fd("jj log 2>&1", RedirectKind::DuplicateOutput, "2>&1")]
    fn test_redirect_kinds_and_spans(
        #[case] script: &str,
        #[case] expected_kind: RedirectKind,
        #[case] expected_redirect_slice: &str,
    ) {
        let parsed = ParsedCommand::new(script);
        let cmd = &parsed.commands()[0];
        let redirect = &cmd.redirects[0];
        assert_eq!(redirect.kind, expected_kind);
        assert_eq!(
            &parsed.source()[redirect.span.start..redirect.span.end],
            expected_redirect_slice
        );
    }

    #[test]
    fn test_heredoc_and_word_escapes() {
        let script = indoc::indoc! {r"
            cat <<EOF
            hello
            EOF
            echo a\ b $HOME"};
        let parsed = ParsedCommand::new(script);
        assert_eq!(
            parsed.commands()[0].redirects[0].kind,
            RedirectKind::HereDoc
        );
        assert_eq!(parsed.commands()[1].arguments[0].text, "a b");
        assert!(parsed.commands()[1].arguments[1].has_expansion);
    }
}
