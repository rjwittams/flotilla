use clap::{Parser, Subcommand};
use flotilla_protocol::{Command, CommandAction};

use crate::{
    resolved::{HostResolution, RepoContext},
    Resolved,
};

#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(about = "Manage convoys")]
pub struct ConvoyNoun {
    /// Convoy name
    pub subject: String,

    #[command(subcommand)]
    pub verb: ConvoyVerb,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum ConvoyVerb {
    /// Manage convoy tasks
    Task(ConvoyTaskNoun),
}

#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct ConvoyTaskNoun {
    /// Task name
    pub subject: String,

    #[command(subcommand)]
    pub verb: ConvoyTaskVerb,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum ConvoyTaskVerb {
    /// Mark a convoy task complete
    Complete {
        /// Optional completion message recorded on the task
        #[arg(long)]
        message: Option<String>,
    },
}

impl ConvoyNoun {
    pub fn resolve(self) -> Result<Resolved, String> {
        match self.verb {
            ConvoyVerb::Task(task) => match task.verb {
                ConvoyTaskVerb::Complete { message } => Ok(Resolved::NeedsContext {
                    command: Command {
                        node_id: None,
                        provisioning_target: None,
                        context_repo: None,
                        action: CommandAction::ConvoyTaskComplete { convoy: self.subject, task: task.subject, message },
                    },
                    repo: RepoContext::None,
                    host: HostResolution::Local,
                }),
            },
        }
    }
}

impl std::fmt::Display for ConvoyNoun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "convoy {}", self.subject)?;
        match &self.verb {
            ConvoyVerb::Task(task) => {
                write!(f, " task {}", task.subject)?;
                match &task.verb {
                    ConvoyTaskVerb::Complete { message } => {
                        write!(f, " complete")?;
                        if let Some(message) = message {
                            write!(f, " --message {message}")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use flotilla_protocol::{Command, CommandAction};

    use super::ConvoyNoun;
    use crate::{
        resolved::{HostResolution, RepoContext},
        test_utils::assert_round_trip,
        Resolved,
    };

    fn parse(args: &[&str]) -> ConvoyNoun {
        ConvoyNoun::try_parse_from(args).expect("should parse")
    }

    #[test]
    fn convoy_task_complete_resolves() {
        let resolved = parse(&["convoy", "convoy-a", "task", "implement", "complete"]).resolve().expect("resolve");
        assert_eq!(resolved, Resolved::NeedsContext {
            command: Command {
                node_id: None,
                provisioning_target: None,
                context_repo: None,
                action: CommandAction::ConvoyTaskComplete { convoy: "convoy-a".into(), task: "implement".into(), message: None },
            },
            repo: RepoContext::None,
            host: HostResolution::Local,
        });
    }

    #[test]
    fn convoy_task_complete_with_message_resolves() {
        let resolved = parse(&["convoy", "convoy-a", "task", "implement", "complete", "--message", "done"]).resolve().expect("resolve");
        assert_eq!(resolved, Resolved::NeedsContext {
            command: Command {
                node_id: None,
                provisioning_target: None,
                context_repo: None,
                action: CommandAction::ConvoyTaskComplete {
                    convoy: "convoy-a".into(),
                    task: "implement".into(),
                    message: Some("done".into()),
                },
            },
            repo: RepoContext::None,
            host: HostResolution::Local,
        });
    }

    #[test]
    fn round_trip_complete() {
        assert_round_trip::<ConvoyNoun>(&["convoy", "convoy-a", "task", "implement", "complete"]);
    }
}
