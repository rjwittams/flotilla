use flotilla_protocol::{Command, HostName};

/// Output of noun resolution — what main.rs dispatches on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A command to send to the daemon for execution.
    Command(Command),
    /// Query: show repo details.
    RepoDetail { slug: String },
    /// Query: show repo providers.
    RepoProviders { slug: String },
    /// Query: show repo work items.
    RepoWork { slug: String },
    /// Query: list all known hosts.
    HostList,
    /// Query: show host status.
    HostStatus { host: String },
    /// Query: show host providers.
    HostProviders { host: String },
}

impl Resolved {
    /// Set the target host on a resolved command or query.
    /// For Command variants, sets Command.host.
    /// For query variants that carry a host field, this is a no-op
    /// (the host is already populated by the noun's resolve).
    pub fn set_host(&mut self, host: String) {
        match self {
            Resolved::Command(cmd) => {
                cmd.host = Some(HostName::new(&host));
            }
            // Query variants with host are already populated
            Resolved::HostStatus { .. } | Resolved::HostProviders { .. } | Resolved::HostList => {}
            // Repo queries routed through a host become commands instead
            // (handled in HostNoun::resolve, not here)
            Resolved::RepoDetail { .. } | Resolved::RepoProviders { .. } | Resolved::RepoWork { .. } => {}
        }
    }
}

/// Two-stage parsing: clap parse produces a partial type, refine produces the full type.
/// Only needed for nouns where clap cannot express the full structure in one pass (e.g. host routing).
pub trait Refinable {
    type Refined;
    fn refine(self) -> Result<Self::Refined, String>;
}
