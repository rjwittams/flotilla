use clap::{CommandFactory, Parser};
use flotilla_session::cli::{Cli, Command};

#[test]
fn help_lists_expected_subcommands() {
    let command = Cli::command();
    let subcommands: Vec<_> = command.get_subcommands().filter(|sub| !sub.is_hide_set()).map(|sub| sub.get_name().to_string()).collect();
    assert_eq!(subcommands, vec!["attach", "create", "list", "kill"]);
}

#[test]
fn attach_command_parses() {
    let cli = Cli::try_parse_from(["flotilla-session", "attach", "--name", "demo"]).expect("attach parses");
    assert_eq!(cli.command, Command::Attach { id: None, name: Some("demo".into()), cwd: None, cmd: None });
}

#[test]
fn attach_command_parses_positional_name() {
    let cli = Cli::try_parse_from(["flotilla-session", "attach", "demo"]).expect("attach positional parses");
    assert_eq!(cli.command, Command::Attach { id: Some("demo".into()), name: None, cwd: None, cmd: None });
}

#[test]
fn create_command_parses() {
    let cli = Cli::try_parse_from(["flotilla-session", "create", "--cmd", "bash"]).expect("create parses");
    assert_eq!(cli.command, Command::Create { id: None, name: None, cwd: None, cmd: Some("bash".into()) });
}

#[test]
fn create_command_parses_positional_name() {
    let cli = Cli::try_parse_from(["flotilla-session", "create", "demo", "--cmd", "bash"]).expect("create positional parses");
    assert_eq!(cli.command, Command::Create { id: Some("demo".into()), name: None, cwd: None, cmd: Some("bash".into()) });
}

#[test]
fn list_command_parses() {
    let cli = Cli::try_parse_from(["flotilla-session", "list"]).expect("list parses");
    assert_eq!(cli.command, Command::List);
}

#[test]
fn kill_command_parses() {
    let cli = Cli::try_parse_from(["flotilla-session", "kill", "session-1"]).expect("kill parses");
    assert_eq!(cli.command, Command::Kill { id: "session-1".into() });
}
