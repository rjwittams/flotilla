use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::info;

use crate::providers::types::*;
use crate::providers::CommandRunner;

pub struct WtCheckoutManager {
    runner: Arc<dyn CommandRunner>,
}

#[derive(Debug, Deserialize)]
struct WtWorktree {
    branch: String,
    path: PathBuf,
    #[serde(default)]
    is_main: bool,
    #[serde(default)]
    #[allow(dead_code)]
    is_current: bool,
    #[serde(default)]
    main: Option<WtAheadBehind>,
    #[serde(default)]
    remote: Option<WtRemote>,
    #[serde(default)]
    working_tree: Option<WtWorkingTree>,
    #[serde(default)]
    commit: Option<WtCommit>,
}

#[derive(Debug, Deserialize)]
struct WtAheadBehind {
    ahead: i64,
    behind: i64,
}

#[derive(Debug, Deserialize)]
struct WtRemote {
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    branch: Option<String>,
    ahead: i64,
    behind: i64,
}

#[derive(Debug, Deserialize)]
struct WtWorkingTree {
    #[serde(default)]
    staged: bool,
    #[serde(default)]
    modified: bool,
    #[serde(default)]
    untracked: bool,
}

#[derive(Debug, Deserialize)]
struct WtCommit {
    short_sha: Option<String>,
    message: Option<String>,
}

impl WtWorktree {
    fn into_checkout(self) -> (PathBuf, Checkout) {
        let path = self.path;
        let correlation_keys = vec![
            CorrelationKey::Branch(self.branch.clone()),
            CorrelationKey::CheckoutPath(path.clone()),
        ];
        (
            path,
            Checkout {
                branch: self.branch,
                is_trunk: self.is_main,
                trunk_ahead_behind: self.main.map(|m| AheadBehind {
                    ahead: m.ahead,
                    behind: m.behind,
                }),
                remote_ahead_behind: self.remote.map(|r| AheadBehind {
                    ahead: r.ahead,
                    behind: r.behind,
                }),
                working_tree: self.working_tree.map(|w| WorkingTreeStatus {
                    staged: if w.staged { 1 } else { 0 },
                    modified: if w.modified { 1 } else { 0 },
                    untracked: if w.untracked { 1 } else { 0 },
                }),
                last_commit: self.commit.map(|c| CommitInfo {
                    short_sha: c.short_sha.unwrap_or_default(),
                    message: c.message.unwrap_or_default(),
                }),
                correlation_keys,
                association_keys: Vec::new(),
            },
        )
    }
}

impl WtCheckoutManager {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }

    /// Strip ANSI escape codes that `wt` may append after JSON output.
    fn strip_to_json(output: &str) -> &str {
        let end = output.rfind(']').map(|i| i + 1).unwrap_or(output.len());
        &output[..end]
    }
}

#[async_trait]
impl super::CheckoutManager for WtCheckoutManager {
    fn display_name(&self) -> &str {
        "wt"
    }

    fn section_label(&self) -> &str {
        "Worktrees"
    }
    fn item_noun(&self) -> &str {
        "worktree"
    }
    fn abbreviation(&self) -> &str {
        "WT"
    }

    async fn list_checkouts(&self, repo_root: &Path) -> Result<Vec<(PathBuf, Checkout)>, String> {
        let output = self
            .runner
            .run("wt", &["list", "--format=json"], repo_root)
            .await?;
        let json = Self::strip_to_json(&output);
        let worktrees: Vec<WtWorktree> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut checkouts: Vec<(PathBuf, Checkout)> =
            worktrees.into_iter().map(|wt| wt.into_checkout()).collect();

        // Enrich with issue links from git config
        let futures: Vec<_> = checkouts
            .iter()
            .map(|(_, co)| super::read_branch_issue_links(repo_root, &co.branch, &*self.runner))
            .collect();
        let all_links = futures::future::join_all(futures).await;
        for ((_, co), links) in checkouts.iter_mut().zip(all_links) {
            co.association_keys = links;
        }

        Ok(checkouts)
    }

    async fn create_checkout(
        &self,
        repo_root: &Path,
        branch: &str,
        create_branch: bool,
    ) -> Result<(PathBuf, Checkout), String> {
        info!("wt: creating worktree for {branch} (create_branch={create_branch})");
        if create_branch {
            self.runner
                .run("wt", &["switch", "--create", branch, "--no-cd"], repo_root)
                .await?;
        } else {
            self.runner
                .run("wt", &["switch", branch, "--no-cd", "--yes"], repo_root)
                .await?;
        }

        // Look up the path of the newly created worktree
        let list_output = self
            .runner
            .run("wt", &["list", "--format=json"], repo_root)
            .await?;
        let json = Self::strip_to_json(&list_output);
        let worktrees: Vec<WtWorktree> = serde_json::from_str(json).map_err(|e| e.to_string())?;

        for wt in worktrees {
            if wt.branch == branch || wt.branch.ends_with(&format!("/{branch}")) {
                info!("wt: created {branch} at {}", wt.path.display());
                return Ok(wt.into_checkout());
            }
        }

        Err("Could not find worktree path after creation".to_string())
    }

    async fn remove_checkout(&self, repo_root: &Path, branch: &str) -> Result<(), String> {
        info!("wt: removing worktree {branch}");
        self.runner
            .run("wt", &["remove", branch], repo_root)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::replay::{Masks, ReplaySession};
    use crate::providers::vcs::CheckoutManager;

    fn repo_masks() -> Masks {
        let mut m = Masks::new();
        m.add("/test/repo", "{repo}");
        m
    }

    #[tokio::test]
    async fn replay_list_checkouts() {
        let session = ReplaySession::from_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/providers/vcs/fixtures/wt_list.yaml"
            ),
            repo_masks(),
        );
        let runner = Arc::new(session.command_runner());
        let mgr = WtCheckoutManager::new(runner);
        let checkouts = mgr.list_checkouts(Path::new("/test/repo")).await.unwrap();

        assert_eq!(checkouts.len(), 2);

        // First worktree: main
        let (path0, co0) = &checkouts[0];
        assert_eq!(path0, Path::new("/test/repo"));
        assert_eq!(co0.branch, "main");
        assert!(co0.is_trunk);
        assert!(co0.trunk_ahead_behind.is_none());
        assert!(co0.remote_ahead_behind.is_none());
        assert!(co0.working_tree.is_none());
        let commit0 = co0.last_commit.as_ref().unwrap();
        assert_eq!(commit0.short_sha, "abc1234");
        assert_eq!(commit0.message, "Initial commit");
        // No issue links configured
        assert!(co0.association_keys.is_empty());

        // Second worktree: feature/foo
        let (path1, co1) = &checkouts[1];
        assert_eq!(path1, Path::new("/test/repo.wt/feature-foo"));
        assert_eq!(co1.branch, "feature/foo");
        assert!(!co1.is_trunk);
        let trunk_ab = co1.trunk_ahead_behind.as_ref().unwrap();
        assert_eq!(trunk_ab.ahead, 3);
        assert_eq!(trunk_ab.behind, 1);
        let remote_ab = co1.remote_ahead_behind.as_ref().unwrap();
        assert_eq!(remote_ab.ahead, 0);
        assert_eq!(remote_ab.behind, 0);
        let wt_status = co1.working_tree.as_ref().unwrap();
        assert_eq!(wt_status.staged, 1);
        assert_eq!(wt_status.modified, 0);
        assert_eq!(wt_status.untracked, 1);
        let commit1 = co1.last_commit.as_ref().unwrap();
        assert_eq!(commit1.short_sha, "def5678");
        assert_eq!(commit1.message, "Add feature");
        // No issue links configured
        assert!(co1.association_keys.is_empty());

        // Correlation keys present
        assert!(co1
            .correlation_keys
            .contains(&CorrelationKey::Branch("feature/foo".to_string())));
        assert!(co1
            .correlation_keys
            .contains(&CorrelationKey::CheckoutPath(PathBuf::from(
                "/test/repo.wt/feature-foo"
            ))));

        session.assert_complete();
    }

    #[tokio::test]
    async fn replay_create_checkout() {
        let session = ReplaySession::from_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/providers/vcs/fixtures/wt_create.yaml"
            ),
            repo_masks(),
        );
        let runner = Arc::new(session.command_runner());
        let mgr = WtCheckoutManager::new(runner);
        let (path, checkout) = mgr
            .create_checkout(Path::new("/test/repo"), "new-feature", true)
            .await
            .unwrap();

        assert_eq!(path, Path::new("/test/repo.wt/new-feature"));
        assert_eq!(checkout.branch, "new-feature");
        assert!(!checkout.is_trunk);
        let trunk_ab = checkout.trunk_ahead_behind.as_ref().unwrap();
        assert_eq!(trunk_ab.ahead, 0);
        assert_eq!(trunk_ab.behind, 0);
        assert!(checkout.remote_ahead_behind.is_none());
        let wt_status = checkout.working_tree.as_ref().unwrap();
        assert_eq!(wt_status.staged, 0);
        assert_eq!(wt_status.modified, 0);
        assert_eq!(wt_status.untracked, 0);
        // association_keys empty since create_checkout doesn't call read_branch_issue_links
        assert!(checkout.association_keys.is_empty());

        session.assert_complete();
    }

    #[tokio::test]
    async fn replay_remove_checkout() {
        let session = ReplaySession::from_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/providers/vcs/fixtures/wt_remove.yaml"
            ),
            repo_masks(),
        );
        let runner = Arc::new(session.command_runner());
        let mgr = WtCheckoutManager::new(runner);
        mgr.remove_checkout(Path::new("/test/repo"), "feature/foo")
            .await
            .unwrap();

        session.assert_complete();
    }
}
