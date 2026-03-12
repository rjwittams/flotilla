//! Claude CLI host detector.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::providers::discovery::{EnvironmentAssertion, HostDetector};
use crate::providers::CommandRunner;

/// Detects the `claude` CLI, checking PATH first, then known install locations.
pub struct ClaudeDetector;

#[async_trait]
impl HostDetector for ClaudeDetector {
    fn name(&self) -> &str {
        "claude-cli"
    }

    async fn detect(&self, runner: &dyn CommandRunner) -> Vec<EnvironmentAssertion> {
        // 1. Check PATH
        if runner.exists("claude", &["--version"]).await {
            return vec![EnvironmentAssertion::BinaryAvailable {
                name: "claude".into(),
                path: PathBuf::from("claude"),
                version: None,
            }];
        }

        // 2. Check known installation locations
        let known_paths = [dirs::home_dir().map(|h| h.join(".claude/local/claude"))];
        for path in known_paths.into_iter().flatten() {
            if path.is_file() {
                let path_str = path.to_str().unwrap_or("");
                if runner.exists(path_str, &["--version"]).await {
                    return vec![EnvironmentAssertion::BinaryAvailable {
                        name: "claude".into(),
                        path,
                        version: None,
                    }];
                }
            }
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::discovery::test_support::DiscoveryMockRunner;

    #[tokio::test]
    async fn claude_detector_found_on_path() {
        let runner = DiscoveryMockRunner::builder()
            .tool_exists("claude", true)
            .build();
        let assertions = ClaudeDetector.detect(&runner).await;
        assert_eq!(assertions.len(), 1);
        match &assertions[0] {
            EnvironmentAssertion::BinaryAvailable {
                name,
                path,
                version,
            } => {
                assert_eq!(name, "claude");
                assert_eq!(path, &PathBuf::from("claude"));
                assert!(version.is_none());
            }
            other => panic!("expected BinaryAvailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn claude_detector_not_found() {
        let runner = DiscoveryMockRunner::builder()
            .tool_exists("claude", false)
            .build();
        let assertions = ClaudeDetector.detect(&runner).await;
        // May be empty or may find at known path — depends on filesystem.
        // At minimum, verify it doesn't panic and no PATH-based assertion is returned.
        for a in &assertions {
            match a {
                EnvironmentAssertion::BinaryAvailable { path, .. } => {
                    // If something was found, it shouldn't be the bare "claude" name
                    assert_ne!(path, &PathBuf::from("claude"));
                }
                _ => panic!("unexpected assertion type"),
            }
        }
    }
}
