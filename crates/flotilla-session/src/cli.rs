use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};

use crate::server::SessionService;

#[derive(Debug, Parser)]
#[command(name = "flotilla-session", version)]
pub struct Cli {
    #[arg(long, hide = true)]
    pub runtime_root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    Attach {
        #[arg(value_name = "ID")]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        cmd: Option<String>,
    },
    Create {
        #[arg(value_name = "ID")]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        cmd: Option<String>,
    },
    List,
    Kill {
        id: String,
    },
    #[command(hide = true)]
    Serve {
        #[arg(long)]
        id: String,
    },
}

pub fn parse() -> Cli {
    Cli::parse()
}

pub fn command() -> clap::Command {
    Cli::command()
}

pub fn execute(cli: Cli, service: &SessionService) -> Result<Option<String>, String> {
    match cli.command {
        Command::Attach { id, name, cwd, cmd } => {
            let (_attached, guard) = service.attach(resolve_session_name(id, name)?, cwd, cmd)?;
            guard.relay_stdio()?;
            Ok(None)
        }
        Command::Create { id, name, cwd, cmd } => {
            let created = service.create(resolve_session_name(id, name)?, cwd, cmd)?;
            serde_json::to_string(&created).map(Some).map_err(|err| format!("serialize create result: {err}"))
        }
        Command::List => {
            let sessions = service.list()?;
            serde_json::to_string(&sessions).map(Some).map_err(|err| format!("serialize list result: {err}"))
        }
        Command::Kill { id } => {
            service.kill(&id)?;
            Ok(None)
        }
        Command::Serve { id } => {
            service.serve(&id)?;
            Ok(None)
        }
    }
}

fn resolve_session_name(id: Option<String>, name: Option<String>) -> Result<Option<String>, String> {
    match (id, name) {
        (Some(_), Some(_)) => Err("session id must be provided either positionally or via --name, not both".into()),
        (Some(id), None) => Ok(Some(id)),
        (None, Some(name)) => Ok(Some(name)),
        (None, None) => Ok(None),
    }
}
