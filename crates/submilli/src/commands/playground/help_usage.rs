use clap::{Arg, Command};

/// Render only the command name and input that Clap requires.
///
/// The overview deliberately leaves optional arguments and options to each
/// command's own help. Reading the model directly also keeps required groups
/// such as `FILE` or `--example` intact instead of parsing rendered usage text.
pub(super) fn invocation(command: &Command) -> String {
    let required_groups: Vec<_> = command
        .get_groups()
        .filter(|group| group.is_required_set())
        .collect();
    let mut parts = vec![command.get_name().to_owned()];

    for arg in command.get_arguments() {
        for group in required_groups
            .iter()
            .filter(|group| group.get_args().next() == Some(arg.get_id()))
        {
            let choices: Vec<_> = group
                .get_args()
                .filter_map(|id| command.get_arguments().find(|member| member.get_id() == id))
                .map(group_choice)
                .collect();
            if !choices.is_empty() {
                parts.push(format!("<{}>", choices.join("|")));
            }
        }

        let belongs_to_required_group = required_groups
            .iter()
            .any(|group| group.get_args().any(|id| id == arg.get_id()));
        if arg.is_required_set() && !belongs_to_required_group {
            parts.push(required_arg(arg));
        }
    }

    if command.is_subcommand_required_set() {
        parts.push(format!(
            "<{}>",
            command.get_subcommand_value_name().unwrap_or("COMMAND")
        ));
    }

    parts.join(" ")
}

fn required_arg(arg: &Arg) -> String {
    if arg.is_positional() {
        return value_names(arg)
            .into_iter()
            .map(|name| format!("<{name}>"))
            .collect::<Vec<_>>()
            .join(" ");
    }

    let option = option_name(arg);
    let values = option_values(arg);
    if values.is_empty() {
        option
    } else {
        format!(
            "{option} {}",
            values
                .into_iter()
                .map(|name| format!("<{name}>"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn group_choice(arg: &Arg) -> String {
    if arg.is_positional() {
        return value_names(arg).join(" ");
    }

    let option = option_name(arg);
    let values = option_values(arg);
    if values.is_empty() {
        option
    } else {
        format!("{option}={}", values.join(" "))
    }
}

fn option_values(arg: &Arg) -> Vec<String> {
    if arg.get_action().takes_values() {
        value_names(arg)
    } else {
        Vec::new()
    }
}

fn option_name(arg: &Arg) -> String {
    if let Some(long) = arg.get_long() {
        format!("--{long}")
    } else if let Some(short) = arg.get_short() {
        format!("-{short}")
    } else {
        arg.get_id().to_string()
    }
}

fn value_names(arg: &Arg) -> Vec<String> {
    arg.get_value_names().map_or_else(
        || {
            if arg.get_action().takes_values() {
                vec![arg.get_id().as_str().to_uppercase()]
            } else {
                Vec::new()
            }
        },
        |names| names.iter().map(ToString::to_string).collect(),
    )
}

#[cfg(test)]
mod tests {
    use clap::{ArgAction, ArgGroup, Subcommand};

    use super::*;
    use crate::commands::playground::PlaygroundCmd;

    #[test]
    fn reflects_the_playground_parser_contract() {
        let commands = PlaygroundCmd::augment_subcommands(Command::new("playground"));
        let expected = [
            ("show", "show <RUN>"),
            ("explain", "explain <RUN.N>"),
            ("compare", "compare <RUN-A> <RUN-B>"),
            ("chat", "chat <MESSAGE>"),
            ("session", "session <COMMAND>"),
            ("exec", "exec <FILE|--example>"),
        ];

        for (name, expected) in expected {
            let command = commands.find_subcommand(name).unwrap();
            assert_eq!(invocation(command), expected);
        }
    }

    #[test]
    fn shows_required_inputs_without_optional_noise() {
        let command = Command::new("show")
            .arg(Arg::new("run").required(true).value_name("RUN"))
            .arg(Arg::new("json").long("json").action(ArgAction::SetTrue));

        assert_eq!(invocation(&command), "show <RUN>");
    }

    #[test]
    fn keeps_a_required_mixed_group_as_one_choice() {
        let command = Command::new("exec")
            .arg(Arg::new("file").value_name("FILE"))
            .arg(
                Arg::new("example")
                    .long("example")
                    .action(ArgAction::SetTrue),
            )
            .group(
                ArgGroup::new("program")
                    .required(true)
                    .args(["file", "example"]),
            );

        assert_eq!(invocation(&command), "exec <FILE|--example>");
    }

    #[test]
    fn shows_a_required_subcommand_placeholder() {
        let command = Command::new("session")
            .subcommand_required(true)
            .subcommand(Command::new("start"));

        assert_eq!(invocation(&command), "session <COMMAND>");
    }
}
