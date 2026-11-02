use clap::{Command, Subcommand};

use super::{PlaygroundCmd, help_usage};

const COMMAND_GROUPS: &[(&str, &[&str])] = &[
    (
        "Start and open the browser",
        &["start", "open", "init-demo", "status", "restart", "stop"],
    ),
    (
        "Run from the terminal",
        &[
            "chat", "exec", "bind", "session", "sessions", "cancel", "watch",
        ],
    ),
    ("Connect a coding assistant", &["mcp"]),
    (
        "Advanced inspection and testing",
        &[
            "runs",
            "show",
            "explain",
            "compare",
            "audit",
            "changes",
            "recheck",
            "test",
            "rerun",
            "draft-rule",
            "clear",
        ],
    ),
];

const INVOCATION_WIDTH: usize = 24;
const SUMMARY_WIDTH: usize = 48;

pub(super) fn template() -> String {
    let commands = PlaygroundCmd::augment_subcommands(Command::new("playground"));
    let mut help = String::from(
        "{about-with-newline}\n{usage-heading} {usage}\n\n\
         Run `submilli playground` to start and open the browser.\n\
         Use the browser to chat and review runs, decision explanations, and comparisons.\n\n\
         Commands:\n",
    );
    append_groups(&mut help, &commands, COMMAND_GROUPS);

    // New commands remain discoverable even before they have a task group.
    let others: Vec<_> = commands
        .get_subcommands()
        .filter(|command| {
            !command.is_hide_set()
                && !COMMAND_GROUPS
                    .iter()
                    .any(|(_, names)| names.contains(&command.get_name()))
        })
        .collect();
    if !others.is_empty() {
        help.push_str("  Other commands:\n");
        for command in others {
            append_command(&mut help, command);
        }
        help.push('\n');
    }
    help.push_str(
        "Run `submilli playground <COMMAND> --help` for arguments and options.\n\n\
         Start options (also available on `start`):\n{options}\n",
    );
    help
}

fn append_groups(help: &mut String, commands: &Command, groups: &[(&str, &[&str])]) {
    for (heading, names) in groups {
        help.push_str(&format!("  {heading}:\n"));
        for name in *names {
            if let Some(command) = commands.find_subcommand(name) {
                append_command(help, command);
            }
        }
        help.push('\n');
    }
}

fn append_command(help: &mut String, command: &Command) {
    let invocation = help_usage::invocation(command);
    let about = command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut words = about.split_whitespace().peekable();
    let mut line = String::new();
    let mut first = true;

    while words.peek().is_some() {
        line.clear();
        while let Some(word) = words.peek() {
            let separator = usize::from(!line.is_empty());
            if !line.is_empty() && line.len() + separator + word.len() > SUMMARY_WIDTH {
                break;
            }
            if separator == 1 {
                line.push(' ');
            }
            line.push_str(words.next().expect("peeked at the summary word"));
        }

        if first {
            help.push_str(&format!("    {invocation:<INVOCATION_WIDTH$} {line}\n"));
            first = false;
        } else {
            help.push_str(&format!("    {:INVOCATION_WIDTH$} {line}\n", ""));
        }
    }

    if first {
        help.push_str(&format!("    {invocation}\n"));
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    #[test]
    fn overview_lists_visible_commands_once_and_shows_required_arguments() {
        let mut cli = crate::Cli::command();
        let command = cli.find_subcommand_mut("playground").unwrap();
        let help = command.render_long_help().to_string();
        for subcommand in command.get_subcommands().filter(|c| !c.is_hide_set()) {
            if subcommand.get_name() == "help" {
                continue;
            }
            let row = format!("    {} ", subcommand.get_name());
            assert_eq!(help.matches(&row).count(), 1, "{row}: {help}");
        }
        assert!(help.contains("show <RUN>"), "{help}");
        assert!(help.contains("compare <RUN-A> <RUN-B>"), "{help}");
        assert!(help.contains("explain <RUN.N>"), "{help}");
        assert!(!help.contains("chat-hook"), "{help}");
    }

    #[test]
    fn demo_command_keeps_its_detailed_help_and_accepts_the_old_name() {
        let error = crate::Cli::try_parse_from(["submilli", "playground", "init-demo", "--help"])
            .err()
            .unwrap();
        let help = error.to_string();
        assert!(help.contains("signed-in customer"), "{help}");
        assert!(
            help.contains("Usage: submilli playground init-demo"),
            "{help}"
        );
        assert!(crate::Cli::try_parse_from(["submilli", "playground", "init"]).is_ok());
    }
}
