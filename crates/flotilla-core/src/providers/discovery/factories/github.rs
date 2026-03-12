//! GitHub factories for code review and issue tracker providers.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::ConfigStore;
use crate::providers::code_review::github::GitHubCodeReview;
use crate::providers::code_review::CodeReview;
use crate::providers::discovery::{
    CodeReviewFactory, EnvironmentBag, HostPlatform, IssueTrackerFactory, ProviderDescriptor,
    UnmetRequirement,
};
use crate::providers::github_api::GhApiClient;
use crate::providers::issue_tracker::github::GitHubIssueTracker;
use crate::providers::issue_tracker::IssueTracker;
use crate::providers::CommandRunner;

// ---------------------------------------------------------------------------
// GitHubCodeReviewFactory
// ---------------------------------------------------------------------------

pub struct GitHubCodeReviewFactory;

#[async_trait]
impl CodeReviewFactory for GitHubCodeReviewFactory {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            name: "github".into(),
            display_name: "GitHub Pull Requests".into(),
            abbreviation: "PR".into(),
            section_label: "Pull Requests".into(),
            item_noun: "pull request".into(),
        }
    }

    async fn probe(
        &self,
        env: &EnvironmentBag,
        _config: &ConfigStore,
        _repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn CodeReview>, Vec<UnmetRequirement>> {
        let mut unmet = vec![];
        if env.find_binary("gh").is_none() {
            unmet.push(UnmetRequirement::MissingBinary("gh".into()));
        }
        let remote = env.find_remote_host(HostPlatform::GitHub);
        if remote.is_none() {
            unmet.push(UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub));
        }
        if !unmet.is_empty() {
            return Err(unmet);
        }
        let (owner, repo, _remote_name) = remote.expect("checked above");
        let repo_slug = format!("{owner}/{repo}");
        let api = Arc::new(GhApiClient::new(runner.clone()));
        Ok(Arc::new(GitHubCodeReview::new(
            "github".into(),
            repo_slug,
            api,
            runner,
        )))
    }
}

// ---------------------------------------------------------------------------
// GitHubIssueTrackerFactory
// ---------------------------------------------------------------------------

pub struct GitHubIssueTrackerFactory;

#[async_trait]
impl IssueTrackerFactory for GitHubIssueTrackerFactory {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            name: "github".into(),
            display_name: "GitHub Issues".into(),
            abbreviation: "#".into(),
            section_label: "Issues".into(),
            item_noun: "issue".into(),
        }
    }

    async fn probe(
        &self,
        env: &EnvironmentBag,
        _config: &ConfigStore,
        _repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn IssueTracker>, Vec<UnmetRequirement>> {
        let mut unmet = vec![];
        if env.find_binary("gh").is_none() {
            unmet.push(UnmetRequirement::MissingBinary("gh".into()));
        }
        let remote = env.find_remote_host(HostPlatform::GitHub);
        if remote.is_none() {
            unmet.push(UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub));
        }
        if !unmet.is_empty() {
            return Err(unmet);
        }
        let (owner, repo, _remote_name) = remote.expect("checked above");
        let repo_slug = format!("{owner}/{repo}");
        let api = Arc::new(GhApiClient::new(runner.clone()));
        Ok(Arc::new(GitHubIssueTracker::new(
            "github".into(),
            repo_slug,
            api,
            runner,
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use crate::config::ConfigStore;
    use crate::providers::discovery::test_support::DiscoveryMockRunner;
    use crate::providers::discovery::{
        CodeReviewFactory, EnvironmentAssertion, EnvironmentBag, HostPlatform, IssueTrackerFactory,
        UnmetRequirement,
    };

    use super::{GitHubCodeReviewFactory, GitHubIssueTrackerFactory};

    fn bag_with_gh_and_github_remote() -> EnvironmentBag {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::BinaryAvailable {
            name: "gh".into(),
            path: PathBuf::from("/usr/bin/gh"),
            version: None,
        });
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "acme".into(),
            repo: "widgets".into(),
            remote_name: "origin".into(),
        });
        bag
    }

    fn bag_with_github_remote_only() -> EnvironmentBag {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "acme".into(),
            repo: "widgets".into(),
            remote_name: "origin".into(),
        });
        bag
    }

    fn bag_with_gh_binary_only() -> EnvironmentBag {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::BinaryAvailable {
            name: "gh".into(),
            path: PathBuf::from("/usr/bin/gh"),
            version: None,
        });
        bag
    }

    // ── GitHubCodeReviewFactory tests ──

    #[tokio::test]
    async fn github_code_review_factory_succeeds() {
        let bag = bag_with_gh_and_github_remote();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubCodeReviewFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn github_code_review_factory_missing_gh() {
        let bag = bag_with_github_remote_only();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubCodeReviewFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        let unmet = result.err().expect("should fail without gh binary");
        assert!(unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
        assert!(!unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
    }

    #[tokio::test]
    async fn github_code_review_factory_missing_remote() {
        let bag = bag_with_gh_binary_only();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubCodeReviewFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        let unmet = result.err().expect("should fail without remote host");
        assert!(unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
        assert!(!unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
    }

    #[tokio::test]
    async fn github_code_review_factory_missing_both() {
        let bag = EnvironmentBag::new();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubCodeReviewFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        let unmet = result.err().expect("should fail with both missing");
        assert!(unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
        assert!(unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
        assert_eq!(unmet.len(), 2);
    }

    #[tokio::test]
    async fn github_code_review_factory_descriptor() {
        let desc = GitHubCodeReviewFactory.descriptor();
        assert_eq!(desc.name, "github");
        assert_eq!(desc.display_name, "GitHub Pull Requests");
        assert_eq!(desc.abbreviation, "PR");
        assert_eq!(desc.section_label, "Pull Requests");
        assert_eq!(desc.item_noun, "pull request");
    }

    // ── GitHubIssueTrackerFactory tests ──

    #[tokio::test]
    async fn github_issue_tracker_factory_succeeds() {
        let bag = bag_with_gh_and_github_remote();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueTrackerFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn github_issue_tracker_factory_missing_gh() {
        let bag = bag_with_github_remote_only();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueTrackerFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        let unmet = result.err().expect("should fail without gh binary");
        assert!(unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
        assert!(!unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
    }

    #[tokio::test]
    async fn github_issue_tracker_factory_missing_remote() {
        let bag = bag_with_gh_binary_only();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueTrackerFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        let unmet = result.err().expect("should fail without remote host");
        assert!(unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
        assert!(!unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
    }

    #[tokio::test]
    async fn github_issue_tracker_factory_missing_both() {
        let bag = EnvironmentBag::new();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueTrackerFactory
            .probe(&bag, &config, Path::new("/repo"), runner)
            .await;
        let unmet = result.err().expect("should fail with both missing");
        assert!(unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
        assert!(unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
        assert_eq!(unmet.len(), 2);
    }

    #[tokio::test]
    async fn github_issue_tracker_factory_descriptor() {
        let desc = GitHubIssueTrackerFactory.descriptor();
        assert_eq!(desc.name, "github");
        assert_eq!(desc.display_name, "GitHub Issues");
        assert_eq!(desc.abbreviation, "#");
        assert_eq!(desc.section_label, "Issues");
        assert_eq!(desc.item_noun, "issue");
    }
}
