//! GitHub factory for the issue query service.

use std::sync::Arc;

use async_trait::async_trait;

use super::github::github_repo_slug;
use crate::{
    config::ConfigStore,
    path_context::ExecutionEnvironmentPath,
    providers::{
        discovery::{EnvironmentBag, Factory, ServiceCategory, ServiceDescriptor, UnmetRequirement},
        issue_query::IssueQueryService,
        CommandRunner,
    },
};

pub struct GitHubIssueQueryServiceFactory;

#[async_trait]
impl Factory for GitHubIssueQueryServiceFactory {
    type Descriptor = ServiceDescriptor;
    type Output = dyn IssueQueryService;

    fn descriptor(&self) -> ServiceDescriptor {
        ServiceDescriptor {
            category: ServiceCategory::IssueQuery,
            backend: "github".into(),
            implementation: "github".into(),
            display_name: "GitHub Issues".into(),
        }
    }

    async fn probe(
        &self,
        env: &EnvironmentBag,
        _config: &ConfigStore,
        _repo_root: &ExecutionEnvironmentPath,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn IssueQueryService>, Vec<UnmetRequirement>> {
        let repo_slug = github_repo_slug(env)?;
        let api = Arc::new(crate::providers::github_api::GhApiClient::new(runner.clone()));
        Ok(Arc::new(crate::providers::issue_query::github::GitHubIssueQueryService::new(repo_slug, api, runner)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::GitHubIssueQueryServiceFactory;
    use crate::{
        config::ConfigStore,
        path_context::ExecutionEnvironmentPath,
        providers::discovery::{
            test_support::DiscoveryMockRunner, EnvironmentAssertion, EnvironmentBag, Factory, HostPlatform, UnmetRequirement,
        },
    };

    fn bag_with_gh_and_github_remote() -> EnvironmentBag {
        EnvironmentBag::new().with(EnvironmentAssertion::binary("gh", "/usr/bin/gh")).with(EnvironmentAssertion::remote_host(
            HostPlatform::GitHub,
            "acme",
            "widgets",
            "origin",
        ))
    }

    #[tokio::test]
    async fn github_issue_query_factory_succeeds() {
        let bag = bag_with_gh_and_github_remote();
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueQueryServiceFactory.probe(&bag, &config, &ExecutionEnvironmentPath::new("/repo"), runner).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn github_issue_query_factory_missing_gh() {
        let bag = EnvironmentBag::new().with(EnvironmentAssertion::remote_host(HostPlatform::GitHub, "acme", "widgets", "origin"));
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueQueryServiceFactory.probe(&bag, &config, &ExecutionEnvironmentPath::new("/repo"), runner).await;
        let unmet = result.err().expect("should fail without gh binary");
        assert!(unmet.contains(&UnmetRequirement::MissingBinary("gh".into())));
    }

    #[tokio::test]
    async fn github_issue_query_factory_missing_remote() {
        let bag = EnvironmentBag::new().with(EnvironmentAssertion::binary("gh", "/usr/bin/gh"));
        let dir = tempfile::tempdir().expect("failed to create tempdir");
        let config = ConfigStore::with_base(dir.path());
        let runner = Arc::new(DiscoveryMockRunner::builder().build());
        let result = GitHubIssueQueryServiceFactory.probe(&bag, &config, &ExecutionEnvironmentPath::new("/repo"), runner).await;
        let unmet = result.err().expect("should fail without remote host");
        assert!(unmet.contains(&UnmetRequirement::MissingRemoteHost(HostPlatform::GitHub)));
    }

    #[tokio::test]
    async fn github_issue_query_factory_descriptor() {
        let desc = GitHubIssueQueryServiceFactory.descriptor();
        assert_eq!(desc.backend, "github");
        assert_eq!(desc.implementation, "github");
        assert_eq!(desc.display_name, "GitHub Issues");
    }
}
