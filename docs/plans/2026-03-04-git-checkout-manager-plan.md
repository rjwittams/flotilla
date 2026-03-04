# Git Checkout Manager Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a plain `git worktree` checkout manager as a fallback when `wt` CLI is not installed, with configurable worktree path templates.

**Architecture:** New `GitCheckoutManager` implementing the existing `CheckoutManager` trait. Uses `git worktree` commands directly. Path templates use `minijinja` for worktrunk-compatible Jinja syntax. Config loaded from `~/.config/flotilla/config.toml` (global) and per-repo `repos/<slug>.toml` overrides.

**Tech Stack:** Rust, minijinja (new dep), git CLI, toml/serde (existing)

---

### Task 1: Add minijinja dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add minijinja to Cargo.toml**

Add after the `toml` line:

```toml
minijinja = "2"
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles with no new errors

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add minijinja dependency for worktree path templates"
```

---

### Task 2: Add config loading for `[vcs.git.checkouts]`

**Files:**
- Modify: `src/config.rs`

**Step 1: Add config structs and loading**

Add to `src/config.rs` after the existing `RepoConfig` struct:

```rust
/// Global flotilla config from ~/.config/flotilla/config.toml
#[derive(Debug, Default, Deserialize)]
pub struct FlotillaConfig {
    #[serde(default)]
    pub vcs: VcsConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct VcsConfig {
    #[serde(default)]
    pub git: GitConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct GitConfig {
    #[serde(default)]
    pub checkouts: CheckoutsConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckoutsConfig {
    /// Worktree path template (minijinja/worktrunk-compatible).
    /// Variables: repo_path, repo, branch. Filters: sanitize.
    #[serde(default = "CheckoutsConfig::default_path")]
    pub path: String,
    /// Provider selection: "auto" (default), "git", or "wt".
    #[serde(default = "CheckoutsConfig::default_provider")]
    pub provider: String,
}

impl Default for CheckoutsConfig {
    fn default() -> Self {
        Self {
            path: Self::default_path(),
            provider: Self::default_provider(),
        }
    }
}

impl CheckoutsConfig {
    fn default_path() -> String {
        "{{ repo_path }}/../{{ repo }}.{{ branch | sanitize }}".to_string()
    }
    fn default_provider() -> String {
        "auto".to_string()
    }
}
```

Add a `RepoFileConfig` struct that includes the optional override section. The existing `RepoConfig` only has `path`, so add a new struct:

```rust
/// Full repo config file including optional overrides.
#[derive(Debug, Default, Deserialize)]
pub struct RepoFileConfig {
    pub path: String,
    #[serde(default)]
    pub vcs: VcsConfig,
}
```

Add a function to load the global config:

```rust
/// Load global flotilla config from ~/.config/flotilla/config.toml.
pub fn load_config() -> FlotillaConfig {
    let path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config/flotilla/config.toml");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}
```

Add a function to resolve checkouts config for a specific repo (merging global + per-repo):

```rust
/// Resolve checkouts config for a repo: per-repo override > global > defaults.
pub fn resolve_checkouts_config(repo_root: &std::path::Path) -> CheckoutsConfig {
    let global = load_config();
    let slug = path_to_slug(repo_root);
    let repo_file = config_dir().join(format!("{slug}.toml"));
    if let Ok(content) = std::fs::read_to_string(&repo_file) {
        if let Ok(repo_cfg) = toml::from_str::<RepoFileConfig>(&content) {
            // Merge: repo overrides global for non-default fields
            let repo_co = &repo_cfg.vcs.git.checkouts;
            return CheckoutsConfig {
                path: if repo_co.path != CheckoutsConfig::default_path() {
                    repo_co.path.clone()
                } else {
                    global.vcs.git.checkouts.path
                },
                provider: if repo_co.provider != CheckoutsConfig::default_provider() {
                    repo_co.provider.clone()
                } else {
                    global.vcs.git.checkouts.provider
                },
            };
        }
    }
    global.vcs.git.checkouts
}
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles (new structs/functions are unused for now, that's fine)

**Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: add config loading for [vcs.git.checkouts] section"
```

---

### Task 3: Implement GitCheckoutManager — list_checkouts

**Files:**
- Create: `src/providers/vcs/git_worktree.rs`
- Modify: `src/providers/vcs/mod.rs`

**Step 1: Create the file with struct, path template rendering, and list_checkouts**

Create `src/providers/vcs/git_worktree.rs`:

```rust
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::info;

use crate::config::CheckoutsConfig;
use crate::providers::run_cmd;
use crate::providers::types::*;

pub struct GitCheckoutManager {
    config: CheckoutsConfig,
}

impl GitCheckoutManager {
    pub fn new(config: CheckoutsConfig) -> Self {
        Self { config }
    }

    /// Render the worktree path template for a given repo and branch.
    fn render_worktree_path(&self, repo_root: &Path, branch: &str) -> Result<PathBuf, String> {
        let repo_name = repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string());

        let mut env = minijinja::Environment::new();
        env.add_filter("sanitize", |value: String| -> String {
            value.replace(['/', '\\'], "-")
        });
        env.add_template("path", &self.config.path)
            .map_err(|e| format!("invalid worktree path template: {e}"))?;
        let tmpl = env.get_template("path").map_err(|e| e.to_string())?;
        let rendered = tmpl
            .render(minijinja::context! {
                repo_path => repo_root.to_string_lossy(),
                repo => repo_name,
                branch => branch,
            })
            .map_err(|e| format!("failed to render worktree path: {e}"))?;

        // Resolve the rendered path (handles ../ etc)
        let path = PathBuf::from(rendered.trim());
        Ok(if path.is_absolute() {
            path
        } else {
            repo_root.join(&path)
        })
    }

    /// Parse `git worktree list --porcelain` output into (path, branch, is_bare) tuples.
    fn parse_porcelain(output: &str) -> Vec<(PathBuf, String, bool)> {
        let mut results = Vec::new();
        let mut path: Option<PathBuf> = None;
        let mut branch: Option<String> = None;
        let mut bare = false;

        for line in output.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                // Start of a new entry — flush previous
                if let (Some(p), Some(b)) = (path.take(), branch.take()) {
                    results.push((p, b, bare));
                }
                path = Some(PathBuf::from(p));
                branch = None;
                bare = false;
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            } else if line == "bare" {
                bare = true;
            }
        }
        // Flush last entry
        if let (Some(p), Some(b)) = (path, branch) {
            results.push((p, b, bare));
        }
        results
    }

    /// Detect the default branch (main or master) for trunk detection.
    async fn default_branch(repo_root: &Path) -> String {
        // Try symbolic ref of origin/HEAD
        if let Ok(out) = run_cmd(
            "git",
            &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
            repo_root,
        )
        .await
        {
            let trimmed = out.trim();
            if let Some(branch) = trimmed.strip_prefix("origin/") {
                return branch.to_string();
            }
        }
        "main".to_string()
    }

    /// Gather detailed info for a single worktree checkout.
    async fn enrich_checkout(
        repo_root: &Path,
        path: &Path,
        branch: &str,
        is_trunk: bool,
        default_branch: &str,
    ) -> Checkout {
        let correlation_keys = vec![
            CorrelationKey::Branch(branch.to_string()),
            CorrelationKey::CheckoutPath(path.to_path_buf()),
        ];

        // Trunk ahead/behind
        let trunk_ahead_behind = if !is_trunk {
            run_cmd(
                "git",
                &[
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...{default_branch}"),
                ],
                path,
            )
            .await
            .ok()
            .and_then(|out| parse_ahead_behind(&out))
        } else {
            None
        };

        // Remote ahead/behind
        let remote_ahead_behind = run_cmd(
            "git",
            &[
                "rev-list",
                "--left-right",
                "--count",
                &format!("HEAD...origin/{branch}"),
            ],
            path,
        )
        .await
        .ok()
        .and_then(|out| parse_ahead_behind(&out));

        // Working tree status
        let working_tree = run_cmd("git", &["status", "--porcelain"], path)
            .await
            .ok()
            .map(|out| parse_working_tree(&out));

        // Last commit
        let last_commit = run_cmd(
            "git",
            &["log", "-1", "--format=%h\t%s"],
            path,
        )
        .await
        .ok()
        .and_then(|out| {
            let trimmed = out.trim();
            let (sha, msg) = trimmed.split_once('\t')?;
            Some(CommitInfo {
                short_sha: sha.to_string(),
                message: msg.to_string(),
            })
        });

        Checkout {
            branch: branch.to_string(),
            path: path.to_path_buf(),
            is_trunk,
            trunk_ahead_behind,
            remote_ahead_behind,
            working_tree,
            last_commit,
            correlation_keys,
        }
    }
}

fn parse_ahead_behind(output: &str) -> Option<AheadBehind> {
    let parts: Vec<&str> = output.trim().split_whitespace().collect();
    if parts.len() == 2 {
        Some(AheadBehind {
            ahead: parts[0].parse().ok()?,
            behind: parts[1].parse().ok()?,
        })
    } else {
        None
    }
}

fn parse_working_tree(output: &str) -> WorkingTreeStatus {
    let mut staged = 0usize;
    let mut modified = 0usize;
    let mut untracked = 0usize;
    for line in output.lines() {
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0];
        let y = bytes[1];
        if x == b'?' {
            untracked += 1;
        } else {
            if x != b' ' && x != b'?' {
                staged += 1;
            }
            if y != b' ' && y != b'?' {
                modified += 1;
            }
        }
    }
    WorkingTreeStatus {
        staged,
        modified,
        untracked,
    }
}

#[async_trait]
impl super::CheckoutManager for GitCheckoutManager {
    fn display_name(&self) -> &str {
        "git"
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

    async fn list_checkouts(&self, repo_root: &Path) -> Result<Vec<Checkout>, String> {
        let output = run_cmd("git", &["worktree", "list", "--porcelain"], repo_root).await?;
        let entries = Self::parse_porcelain(&output);
        let default_branch = Self::default_branch(repo_root).await;

        let mut checkouts = Vec::new();
        for (path, branch, _bare) in &entries {
            let is_trunk = *branch == default_branch;
            let checkout = Self::enrich_checkout(
                repo_root,
                path,
                branch,
                is_trunk,
                &default_branch,
            )
            .await;
            checkouts.push(checkout);
        }
        Ok(checkouts)
    }

    async fn create_checkout(
        &self,
        repo_root: &Path,
        branch: &str,
    ) -> Result<Checkout, String> {
        let wt_path = self.render_worktree_path(repo_root, branch)?;
        info!("git: creating worktree for {branch} at {}", wt_path.display());

        // Check if local branch exists
        let branch_exists = run_cmd(
            "git",
            &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
            repo_root,
        )
        .await
        .is_ok();

        if branch_exists {
            // Existing branch: git worktree add <path> <branch>
            run_cmd(
                "git",
                &["worktree", "add", wt_path.to_str().unwrap_or(""), branch],
                repo_root,
            )
            .await?;
        } else {
            // New branch: git worktree add -b <branch> <path>
            run_cmd(
                "git",
                &["worktree", "add", "-b", branch, wt_path.to_str().unwrap_or("")],
                repo_root,
            )
            .await?;
        }

        let default_branch = Self::default_branch(repo_root).await;
        let is_trunk = branch == default_branch;
        Ok(Self::enrich_checkout(repo_root, &wt_path, branch, is_trunk, &default_branch).await)
    }

    async fn remove_checkout(
        &self,
        repo_root: &Path,
        branch: &str,
    ) -> Result<(), String> {
        info!("git: removing worktree for {branch}");

        // Find the worktree path for this branch
        let output = run_cmd("git", &["worktree", "list", "--porcelain"], repo_root).await?;
        let entries = Self::parse_porcelain(&output);
        let wt_path = entries
            .iter()
            .find(|(_, b, _)| b == branch)
            .map(|(p, _, _)| p.clone())
            .ok_or_else(|| format!("no worktree found for branch {branch}"))?;

        // Remove the worktree
        run_cmd(
            "git",
            &["worktree", "remove", wt_path.to_str().unwrap_or("")],
            repo_root,
        )
        .await?;

        // Delete the branch
        let _ = run_cmd("git", &["branch", "-D", branch], repo_root).await;

        Ok(())
    }
}
```

**Step 2: Add module declaration**

In `src/providers/vcs/mod.rs`, add after `pub mod wt;`:

```rust
pub mod git_worktree;
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles (with warnings about unused code, which is fine)

**Step 4: Commit**

```bash
git add src/providers/vcs/git_worktree.rs src/providers/vcs/mod.rs
git commit -m "feat: add GitCheckoutManager using plain git worktree commands"
```

---

### Task 4: Wire up provider discovery with config-aware selection

**Files:**
- Modify: `src/providers/discovery.rs`

**Step 1: Update discovery to use config**

Replace the checkout manager section (lines ~113-120) in `detect_providers`:

```rust
use crate::config;
use crate::providers::vcs::git_worktree::GitCheckoutManager;
```

Replace the checkout manager detection block:

```rust
    // 2. Checkout manager: config-driven provider selection
    let co_config = config::resolve_checkouts_config(repo_root);
    match co_config.provider.as_str() {
        "wt" => {
            // Forced wt
            registry
                .checkout_managers
                .insert("git".to_string(), Box::new(WtCheckoutManager::new()));
            info!("{repo_name}: Checkout mgr → wt (forced)");
        }
        "git" => {
            // Forced plain git
            registry
                .checkout_managers
                .insert("git".to_string(), Box::new(GitCheckoutManager::new(co_config)));
            info!("{repo_name}: Checkout mgr → git (forced)");
        }
        _ => {
            // Auto: try wt first, fall back to git
            if command_exists("wt", &["--version"]) {
                registry
                    .checkout_managers
                    .insert("git".to_string(), Box::new(WtCheckoutManager::new()));
                info!("{repo_name}: Checkout mgr → wt");
            } else {
                registry
                    .checkout_managers
                    .insert("git".to_string(), Box::new(GitCheckoutManager::new(co_config)));
                info!("{repo_name}: Checkout mgr → git (fallback)");
            }
        }
    }
```

Remove the old TODO comment about fallback.

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles clean

**Step 3: Commit**

```bash
git add src/providers/discovery.rs
git commit -m "feat: wire GitCheckoutManager into provider discovery with config selection"
```

---

### Task 5: Manual integration test

**Step 1: Create a test config to force git provider**

Create `~/.config/flotilla/config.toml`:

```toml
[vcs.git.checkouts]
provider = "git"
```

**Step 2: Run flotilla and verify worktrees are listed**

Run: `cargo run`

Expected: Worktrees are listed in the table with the "Worktrees" section label, showing branch names, ahead/behind counts, and working tree status. The event log should show "Checkout mgr → git (forced)".

**Step 3: Test worktree creation**

Use the `n` key to create a new branch worktree. Verify it creates at the expected path (`../flotilla.plain-worktrees.<branch-slug>`).

**Step 4: Test worktree removal**

Select the new worktree and use `d` to delete. Verify it removes the worktree directory and branch.

**Step 5: Restore config**

Remove or update the `provider` line back to `"auto"` in the config.

**Step 6: Commit any fixes found during testing**

---

### Task 6: Clippy and final cleanup

**Step 1: Run clippy**

Run: `cargo clippy`
Expected: No new warnings from our code

**Step 2: Run tests**

Run: `cargo test`
Expected: All existing tests pass

**Step 3: Final commit if any cleanup needed**
