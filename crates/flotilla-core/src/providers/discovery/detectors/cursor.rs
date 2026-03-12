//! Cursor IDE host detector.
//!
//! Checks for the `CURSOR_API_KEY` environment variable and the `agent` binary.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::providers::discovery::{EnvironmentAssertion, HostDetector};
use crate::providers::CommandRunner;

/// Detects Cursor IDE availability via env var and binary.
pub struct CursorDetector;

#[async_trait]
impl HostDetector for CursorDetector {
    fn name(&self) -> &str {
        "cursor"
    }

    async fn detect(&self, runner: &dyn CommandRunner) -> Vec<EnvironmentAssertion> {
        let mut assertions = Vec::new();

        // Check CURSOR_API_KEY env var
        if let Ok(value) = std::env::var("CURSOR_API_KEY") {
            assertions.push(EnvironmentAssertion::EnvVarSet {
                key: "CURSOR_API_KEY".into(),
                value,
            });
        }

        // Check `agent` binary
        if runner.exists("agent", &["--version"]).await {
            assertions.push(EnvironmentAssertion::BinaryAvailable {
                name: "agent".into(),
                path: PathBuf::from("agent"),
                version: None,
            });
        }

        assertions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::discovery::test_support::DiscoveryMockRunner;

    #[tokio::test]
    async fn cursor_detector_binary_found() {
        let runner = DiscoveryMockRunner::builder()
            .tool_exists("agent", true)
            .build();
        let assertions = CursorDetector.detect(&runner).await;
        // Should at least have the binary assertion (env var depends on process state)
        let has_binary = assertions.iter().any(|a| {
            matches!(
                a,
                EnvironmentAssertion::BinaryAvailable { name, .. } if name == "agent"
            )
        });
        assert!(has_binary);
    }

    #[tokio::test]
    async fn cursor_detector_binary_not_found() {
        let runner = DiscoveryMockRunner::builder()
            .tool_exists("agent", false)
            .build();
        let assertions = CursorDetector.detect(&runner).await;
        let has_binary = assertions.iter().any(|a| {
            matches!(
                a,
                EnvironmentAssertion::BinaryAvailable { name, .. } if name == "agent"
            )
        });
        assert!(!has_binary);
    }
}
