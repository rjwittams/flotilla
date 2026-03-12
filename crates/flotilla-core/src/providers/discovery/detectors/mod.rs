pub mod claude;
pub mod cmux;
pub mod codex;
pub mod cursor;
pub mod env;
pub mod git;
pub mod github;
pub mod shpool;
pub mod tmux;

use super::{HostDetector, RepoDetector};

pub fn default_host_detectors() -> Vec<Box<dyn HostDetector>> {
    vec![
        Box::new(git::GitBinaryDetector),
        Box::new(github::GhCliDetector),
        Box::new(claude::ClaudeDetector),
        Box::new(cursor::CursorDetector),
        Box::new(cmux::CmuxDetector),
        Box::new(tmux::TmuxDetector),
        Box::new(env::ZellijDetector),
        Box::new(shpool::ShpoolDetector),
    ]
}

pub fn default_repo_detectors() -> Vec<Box<dyn RepoDetector>> {
    vec![
        Box::new(git::VcsRepoDetector),
        Box::new(git::RemoteHostDetector),
        Box::new(codex::CodexAuthDetector),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_host_detectors_non_empty() {
        assert!(!default_host_detectors().is_empty());
    }

    #[test]
    fn default_repo_detectors_non_empty() {
        assert!(!default_repo_detectors().is_empty());
    }
}
