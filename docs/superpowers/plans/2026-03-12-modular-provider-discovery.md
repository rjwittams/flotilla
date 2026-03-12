# Modular Provider Discovery Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the monolithic `detect_providers` function with a two-phase modular pipeline: environment detectors → typed provider factories.

**Architecture:** Host and repo detectors contribute `EnvironmentAssertion` values to an `EnvironmentBag`. Typed factory traits (one per provider category) inspect the bag plus config to construct providers or report unmet requirements. `ProviderDescriptor` replaces scattered label methods on provider traits.

**Tech Stack:** Rust, async-trait, tokio, indexmap

---

## Chunk 1: Core Types and Traits

### Task 1: EnvironmentAssertion, EnvironmentBag, and supporting types

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/mod.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/mod.rs` (empty, for module structure)
- Create: `crates/flotilla-core/src/providers/discovery/factories/mod.rs` (empty, for module structure)
- Modify: `crates/flotilla-core/src/providers/mod.rs:5` — change `pub mod discovery;` to reference the new module directory

**Note:** Rust allows either `discovery.rs` or `discovery/mod.rs` for a module. The existing `discovery.rs` will be replaced by `discovery/mod.rs` (the directory form) to hold sub-modules. The old `discovery.rs` must be removed and its contents migrated. We do this incrementally — first create the new module with core types, then migrate the orchestrator in a later task.

- [ ] **Step 1: Write tests for EnvironmentAssertion and EnvironmentBag**

In `crates/flotilla-core/src/providers/discovery/mod.rs`:

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum VcsKind {
    Git,
    Jj,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostPlatform {
    GitHub,
    GitLab,
}

#[derive(Debug, Clone)]
pub enum EnvironmentAssertion {
    BinaryAvailable {
        name: String,
        path: PathBuf,
        version: Option<String>,
    },
    EnvVarSet {
        key: String,
        value: String,
    },
    VcsCheckoutDetected {
        root: PathBuf,
        kind: VcsKind,
        is_main_checkout: bool,
    },
    RemoteHost {
        platform: HostPlatform,
        owner: String,
        repo: String,
        remote_name: String,
    },
    AuthFileExists {
        provider: String,
        path: PathBuf,
    },
    SocketAvailable {
        name: String,
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Default)]
pub struct EnvironmentBag {
    assertions: Vec<EnvironmentAssertion>,
}

impl EnvironmentBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, assertion: EnvironmentAssertion) {
        self.assertions.push(assertion);
    }

    pub fn extend(&mut self, assertions: Vec<EnvironmentAssertion>) {
        self.assertions.extend(assertions);
    }

    pub fn find_binary(&self, name: &str) -> Option<&PathBuf> {
        self.assertions.iter().find_map(|a| match a {
            EnvironmentAssertion::BinaryAvailable {
                name: n, path, ..
            } if n == name => Some(path),
            _ => None,
        })
    }

    pub fn find_env_var(&self, key: &str) -> Option<&str> {
        self.assertions.iter().find_map(|a| match a {
            EnvironmentAssertion::EnvVarSet { key: k, value } if k == key => {
                Some(value.as_str())
            }
            _ => None,
        })
    }

    pub fn find_remote_host(
        &self,
        platform: HostPlatform,
    ) -> Option<(&str, &str, &str)> {
        // Priority: "origin" remote first, then first match
        let mut first_match = None;
        for a in &self.assertions {
            if let EnvironmentAssertion::RemoteHost {
                platform: p,
                owner,
                repo,
                remote_name,
            } = a
            {
                if *p == platform {
                    if remote_name == "origin" {
                        return Some((owner.as_str(), repo.as_str(), remote_name.as_str()));
                    }
                    if first_match.is_none() {
                        first_match =
                            Some((owner.as_str(), repo.as_str(), remote_name.as_str()));
                    }
                }
            }
        }
        first_match
    }

    pub fn remote_hosts(&self) -> Vec<&EnvironmentAssertion> {
        self.assertions
            .iter()
            .filter(|a| matches!(a, EnvironmentAssertion::RemoteHost { .. }))
            .collect()
    }

    pub fn has_auth(&self, provider: &str) -> bool {
        self.assertions.iter().any(|a| matches!(a,
            EnvironmentAssertion::AuthFileExists { provider: p, .. } if p == provider
        ))
    }

    pub fn find_socket(&self, name: &str) -> Option<&PathBuf> {
        self.assertions.iter().find_map(|a| match a {
            EnvironmentAssertion::SocketAvailable {
                name: n, path, ..
            } if n == name => Some(path),
            _ => None,
        })
    }

    pub fn find_vcs_checkout(&self, kind: VcsKind) -> Option<(&Path, bool)> {
        self.assertions.iter().find_map(|a| match a {
            EnvironmentAssertion::VcsCheckoutDetected {
                root,
                kind: k,
                is_main_checkout,
            } if *k == kind => Some((root.as_path(), *is_main_checkout)),
            _ => None,
        })
    }

    pub fn repo_slug(&self) -> Option<String> {
        self.find_remote_host(HostPlatform::GitHub)
            .or_else(|| self.find_remote_host(HostPlatform::GitLab))
            .map(|(owner, repo, _)| format!("{owner}/{repo}"))
    }

    pub fn merge(&self, other: &EnvironmentBag) -> EnvironmentBag {
        let mut merged = self.clone();
        merged.assertions.extend(other.assertions.clone());
        merged
    }
}

pub mod detectors;
pub mod factories;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_binary_returns_matching_path() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::BinaryAvailable {
            name: "git".into(),
            path: PathBuf::from("/usr/bin/git"),
            version: Some("2.43.0".into()),
        });
        assert_eq!(
            bag.find_binary("git"),
            Some(&PathBuf::from("/usr/bin/git"))
        );
        assert_eq!(bag.find_binary("gh"), None);
    }

    #[test]
    fn find_env_var_returns_value() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::EnvVarSet {
            key: "TMUX".into(),
            value: "/tmp/tmux-501/default,123,0".into(),
        });
        assert_eq!(bag.find_env_var("TMUX"), Some("/tmp/tmux-501/default,123,0"));
        assert_eq!(bag.find_env_var("ZELLIJ"), None);
    }

    #[test]
    fn find_remote_host_prefers_origin() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "fork-owner".into(),
            repo: "repo".into(),
            remote_name: "upstream".into(),
        });
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "origin-owner".into(),
            repo: "repo".into(),
            remote_name: "origin".into(),
        });
        let (owner, _, remote) = bag.find_remote_host(HostPlatform::GitHub).unwrap();
        assert_eq!(owner, "origin-owner");
        assert_eq!(remote, "origin");
    }

    #[test]
    fn find_remote_host_falls_back_to_first() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "some-owner".into(),
            repo: "repo".into(),
            remote_name: "upstream".into(),
        });
        let (owner, _, _) = bag.find_remote_host(HostPlatform::GitHub).unwrap();
        assert_eq!(owner, "some-owner");
    }

    #[test]
    fn find_remote_host_filters_by_platform() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitLab,
            owner: "gl-owner".into(),
            repo: "repo".into(),
            remote_name: "origin".into(),
        });
        assert!(bag.find_remote_host(HostPlatform::GitHub).is_none());
        assert!(bag.find_remote_host(HostPlatform::GitLab).is_some());
    }

    #[test]
    fn has_auth_checks_provider() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::AuthFileExists {
            provider: "codex".into(),
            path: PathBuf::from("/home/user/.codex/auth.json"),
        });
        assert!(bag.has_auth("codex"));
        assert!(!bag.has_auth("claude"));
    }

    #[test]
    fn find_vcs_checkout_filters_by_kind() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::VcsCheckoutDetected {
            root: PathBuf::from("/repo"),
            kind: VcsKind::Git,
            is_main_checkout: true,
        });
        let (root, is_main) = bag.find_vcs_checkout(VcsKind::Git).unwrap();
        assert_eq!(root, Path::new("/repo"));
        assert!(is_main);
        assert!(bag.find_vcs_checkout(VcsKind::Jj).is_none());
    }

    #[test]
    fn repo_slug_from_preferred_remote() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "rjwittams".into(),
            repo: "flotilla".into(),
            remote_name: "origin".into(),
        });
        assert_eq!(bag.repo_slug(), Some("rjwittams/flotilla".into()));
    }

    #[test]
    fn repo_slug_none_when_no_remotes() {
        let bag = EnvironmentBag::new();
        assert_eq!(bag.repo_slug(), None);
    }

    #[test]
    fn merge_combines_assertions() {
        let mut host_bag = EnvironmentBag::new();
        host_bag.push(EnvironmentAssertion::BinaryAvailable {
            name: "git".into(),
            path: PathBuf::from("/usr/bin/git"),
            version: None,
        });
        let mut repo_bag = EnvironmentBag::new();
        repo_bag.push(EnvironmentAssertion::VcsCheckoutDetected {
            root: PathBuf::from("/repo"),
            kind: VcsKind::Git,
            is_main_checkout: true,
        });
        let merged = host_bag.merge(&repo_bag);
        assert!(merged.find_binary("git").is_some());
        assert!(merged.find_vcs_checkout(VcsKind::Git).is_some());
    }

    #[test]
    fn find_socket_returns_path() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::SocketAvailable {
            name: "cmux".into(),
            path: PathBuf::from("/tmp/cmux.sock"),
        });
        assert_eq!(bag.find_socket("cmux"), Some(&PathBuf::from("/tmp/cmux.sock")));
        assert_eq!(bag.find_socket("shpool"), None);
    }

    #[test]
    fn remote_hosts_returns_all() {
        let mut bag = EnvironmentBag::new();
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitHub,
            owner: "a".into(),
            repo: "r".into(),
            remote_name: "origin".into(),
        });
        bag.push(EnvironmentAssertion::RemoteHost {
            platform: HostPlatform::GitLab,
            owner: "b".into(),
            repo: "r".into(),
            remote_name: "upstream".into(),
        });
        bag.push(EnvironmentAssertion::BinaryAvailable {
            name: "git".into(),
            path: PathBuf::from("/usr/bin/git"),
            version: None,
        });
        assert_eq!(bag.remote_hosts().len(), 2);
    }
}
```

- [ ] **Step 2: Create empty sub-module files**

Create `crates/flotilla-core/src/providers/discovery/detectors/mod.rs`:
```rust
// Detector implementations will be added in subsequent tasks.
```

Create `crates/flotilla-core/src/providers/discovery/factories/mod.rs`:
```rust
// Factory implementations will be added in subsequent tasks.
```

- [ ] **Step 3: Handle the module transition**

The existing `discovery.rs` must coexist during the migration. Rename `crates/flotilla-core/src/providers/discovery.rs` to `crates/flotilla-core/src/providers/discovery_legacy.rs`. Update `crates/flotilla-core/src/providers/mod.rs` line 5:

Change:
```rust
pub mod discovery;
```
To:
```rust
pub mod discovery;
pub mod discovery_legacy;
```

The new `discovery/mod.rs` was created in Step 1. Move the entire existing file into `discovery_legacy.rs` — this includes:
- `detect_providers` function and all its helpers (`first_remote_url`, `extract_repo_slug`, `detect_host_from_url`, `extract_repo_identity`, `tracking_remote_url`)
- The `#[cfg(test)] mod tests` block (~750 lines) including `DiscoveryMockRunner`, `DiscoveryMockRunnerBuilder`, and all integration tests
- All imports

Update all references in `in_process.rs`:
- `crate::providers::discovery::detect_providers` → `crate::providers::discovery_legacy::detect_providers` (lines 329, 1221)
- `crate::providers::discovery::first_remote_url` → `crate::providers::discovery_legacy::first_remote_url` (lines 338, 1266)
- `crate::providers::discovery::extract_repo_identity` → `crate::providers::discovery_legacy::extract_repo_identity` (lines 340, 1268)

- [ ] **Step 4: Run tests to verify the module transition compiles**

Run: `cargo test --workspace --locked`
Expected: All existing tests pass. The new `EnvironmentBag` tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/providers/discovery/ \
       crates/flotilla-core/src/providers/discovery_legacy.rs \
       crates/flotilla-core/src/providers/mod.rs \
       crates/flotilla-core/src/in_process.rs
git commit -m "feat: add EnvironmentAssertion and EnvironmentBag core types (#171)"
```

---

### Task 2: UnmetRequirement, ProviderDescriptor, detector traits, factory traits

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery/mod.rs`

- [ ] **Step 1: Add UnmetRequirement, ProviderDescriptor, and all trait definitions**

Append to `crates/flotilla-core/src/providers/discovery/mod.rs` (before the `#[cfg(test)]` block):

```rust
use std::sync::Arc;
use async_trait::async_trait;
use crate::config::ConfigStore;
use crate::providers::CommandRunner;
use crate::providers::vcs::{Vcs, CheckoutManager};
use crate::providers::code_review::CodeReview;
use crate::providers::issue_tracker::IssueTracker;
use crate::providers::coding_agent::CloudAgentService;
use crate::providers::ai_utility::AiUtility;
use crate::providers::workspace::WorkspaceManager;
use crate::providers::terminal::TerminalPool;

// --- Unmet requirements ---

#[derive(Debug, Clone, PartialEq)]
pub enum UnmetRequirement {
    MissingBinary(String),
    MissingEnvVar(String),
    MissingAuth(String),
    MissingRemoteHost(HostPlatform),
    NoVcsCheckout,
}

// --- Provider descriptor ---

#[derive(Debug, Clone)]
pub struct ProviderDescriptor {
    pub name: String,
    pub display_name: String,
    pub abbreviation: String,
    pub section_label: String,
    pub item_noun: String,
}

// --- Detector traits ---

#[async_trait]
pub trait HostDetector: Send + Sync {
    fn name(&self) -> &str;
    async fn detect(&self, runner: &dyn CommandRunner) -> Vec<EnvironmentAssertion>;
}

#[async_trait]
pub trait RepoDetector: Send + Sync {
    fn name(&self) -> &str;
    async fn detect(
        &self,
        repo_root: &Path,
        runner: &dyn CommandRunner,
    ) -> Vec<EnvironmentAssertion>;
}

// --- Factory traits ---

#[async_trait]
pub trait VcsFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn Vcs>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait CheckoutManagerFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn CheckoutManager>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait CodeReviewFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn CodeReview>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait IssueTrackerFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn IssueTracker>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait CloudAgentFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn CloudAgentService>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait AiUtilityFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn AiUtility>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait WorkspaceManagerFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn WorkspaceManager>, Vec<UnmetRequirement>>;
}

#[async_trait]
pub trait TerminalPoolFactory: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn probe(
        &self,
        env: &EnvironmentBag,
        config: &ConfigStore,
        repo_root: &Path,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Arc<dyn TerminalPool>, Vec<UnmetRequirement>>;
}

// --- Factory registry ---

pub struct FactoryRegistry {
    pub vcs: Vec<Box<dyn VcsFactory>>,
    pub checkout_managers: Vec<Box<dyn CheckoutManagerFactory>>,
    pub code_review: Vec<Box<dyn CodeReviewFactory>>,
    pub issue_trackers: Vec<Box<dyn IssueTrackerFactory>>,
    pub cloud_agents: Vec<Box<dyn CloudAgentFactory>>,
    pub ai_utilities: Vec<Box<dyn AiUtilityFactory>>,
    pub workspace_managers: Vec<Box<dyn WorkspaceManagerFactory>>,
    pub terminal_pools: Vec<Box<dyn TerminalPoolFactory>>,
}
```

- [ ] **Step 2: Run tests to verify compilation**

Run: `cargo test --workspace --locked`
Expected: All tests pass. The new types compile.

- [ ] **Step 3: Commit**

```bash
git add crates/flotilla-core/src/providers/discovery/mod.rs
git commit -m "feat: add detector traits, factory traits, and FactoryRegistry (#171)"
```

---

### Task 3: Update ProviderRegistry with ProviderDescriptor

**Files:**
- Modify: `crates/flotilla-core/src/providers/registry.rs:11-59`
- Modify: `crates/flotilla-core/src/model.rs:13-115` — update `labels_from_registry` and `provider_names_from_registry`
- Modify: All files that access `ProviderRegistry` fields directly (many call sites)

This is a mechanical but wide-reaching change. Every place that accesses `registry.vcs`, `registry.code_review`, etc. needs to destructure the tuple.

- [ ] **Step 1: Update ProviderRegistry struct**

In `crates/flotilla-core/src/providers/registry.rs`, change the struct (lines 11-20) to:

```rust
use crate::providers::discovery::ProviderDescriptor;

pub struct ProviderRegistry {
    pub vcs: IndexMap<String, (ProviderDescriptor, Arc<dyn Vcs>)>,
    pub checkout_managers: IndexMap<String, (ProviderDescriptor, Arc<dyn CheckoutManager>)>,
    pub code_review: IndexMap<String, (ProviderDescriptor, Arc<dyn CodeReview>)>,
    pub issue_trackers: IndexMap<String, (ProviderDescriptor, Arc<dyn IssueTracker>)>,
    pub cloud_agents: IndexMap<String, (ProviderDescriptor, Arc<dyn CloudAgentService>)>,
    pub ai_utilities: IndexMap<String, (ProviderDescriptor, Arc<dyn AiUtility>)>,
    pub workspace_manager: Option<(ProviderDescriptor, Arc<dyn WorkspaceManager>)>,
    pub terminal_pool: Option<(ProviderDescriptor, Arc<dyn TerminalPool>)>,
}
```

Update `new()` to initialize with empty `IndexMap`s and `None`s (same as current but types change). Update `strip_external_providers()` similarly.

- [ ] **Step 2: Update labels_from_registry in model.rs**

Replace the function (lines 13-56) to read from `ProviderDescriptor` instead of calling trait methods. The types are `CategoryLabels` (from `flotilla_protocol::snapshot`) and `RepoLabels` — fields are `checkouts`, `code_review`, `issues`, `sessions`, each with `section`, `noun`, `abbr`:

```rust
pub fn labels_from_registry(registry: &ProviderRegistry) -> RepoLabels {
    RepoLabels {
        checkouts: registry
            .checkout_managers
            .values()
            .next()
            .map(|(desc, _)| CategoryLabels {
                section: desc.section_label.clone(),
                noun: desc.item_noun.clone(),
                abbr: desc.abbreviation.clone(),
            })
            .unwrap_or_default(),
        code_review: registry
            .code_review
            .values()
            .next()
            .map(|(desc, _)| CategoryLabels {
                section: desc.section_label.clone(),
                noun: desc.item_noun.clone(),
                abbr: desc.abbreviation.clone(),
            })
            .unwrap_or_default(),
        issues: registry
            .issue_trackers
            .values()
            .next()
            .map(|(desc, _)| CategoryLabels {
                section: desc.section_label.clone(),
                noun: desc.item_noun.clone(),
                abbr: desc.abbreviation.clone(),
            })
            .unwrap_or_default(),
        sessions: registry
            .cloud_agents
            .values()
            .next()
            .map(|(desc, _)| CategoryLabels {
                section: desc.section_label.clone(),
                noun: desc.item_noun.clone(),
                abbr: desc.abbreviation.clone(),
            })
            .unwrap_or_default(),
    }
}
```

- [ ] **Step 3: Update provider_names_from_registry in model.rs**

Replace the function (lines 58-115) to read `ProviderDescriptor::display_name`. Preserve the existing behavior of only inserting non-empty entries (an empty registry must return an empty map):

```rust
pub fn provider_names_from_registry(
    registry: &ProviderRegistry,
) -> HashMap<String, Vec<String>> {
    let mut names: HashMap<String, Vec<String>> = HashMap::new();
    let vcs: Vec<String> = registry.vcs.values().map(|(d, _)| d.display_name.clone()).collect();
    if !vcs.is_empty() { names.insert("vcs".into(), vcs); }
    let cms: Vec<String> = registry.checkout_managers.values().map(|(d, _)| d.display_name.clone()).collect();
    if !cms.is_empty() { names.insert("checkout_manager".into(), cms); }
    let crs: Vec<String> = registry.code_review.values().map(|(d, _)| d.display_name.clone()).collect();
    if !crs.is_empty() { names.insert("code_review".into(), crs); }
    let its: Vec<String> = registry.issue_trackers.values().map(|(d, _)| d.display_name.clone()).collect();
    if !its.is_empty() { names.insert("issue_tracker".into(), its); }
    let cas: Vec<String> = registry.cloud_agents.values().map(|(d, _)| d.display_name.clone()).collect();
    if !cas.is_empty() { names.insert("cloud_agent".into(), cas); }
    let ais: Vec<String> = registry.ai_utilities.values().map(|(d, _)| d.display_name.clone()).collect();
    if !ais.is_empty() { names.insert("ai_utility".into(), ais); }
    if let Some((desc, _)) = &registry.workspace_manager {
        names.insert("workspace_manager".into(), vec![desc.display_name.clone()]);
    }
    if let Some((desc, _)) = &registry.terminal_pool {
        names.insert("terminal_pool".into(), vec![desc.display_name.clone()]);
    }
    names
}
```

- [ ] **Step 4: Fix all compilation errors from the type change**

This is a mechanical grep-and-fix pass. Every place that accesses a `ProviderRegistry` field needs updating. Key patterns:

- `registry.vcs.get("key")` now returns `Option<&(ProviderDescriptor, Arc<dyn Vcs>)>` — destructure to `(_, provider)`
- `registry.vcs.insert("key", provider)` needs `("key", (descriptor, provider))`
- `registry.workspace_manager` and `registry.terminal_pool` change from `Option<(String, Arc<...>)>` to `Option<(ProviderDescriptor, Arc<...>)>` — the string name disappears. In `discovery_legacy.rs`, change `registry.workspace_manager = Some(("cmux".into(), ...))` to `Some((descriptor, ...))`
- Iteration: `for (name, provider) in &registry.vcs` becomes `for (name, (_, provider)) in &registry.vcs` where descriptor is not needed

Use `cargo check` iteratively to find and fix each error. The main files affected:
- `crates/flotilla-core/src/providers/discovery_legacy.rs` (was discovery.rs) — all `.insert()` calls need descriptors
- `crates/flotilla-core/src/in_process.rs` — refresh logic accessing registry fields
- `crates/flotilla-core/src/data.rs` — correlation and table building
- `crates/flotilla-core/src/executor.rs` — command execution against providers
- `crates/flotilla-tui/src/app/` — UI code accessing providers

For `discovery_legacy.rs`: create temporary `ProviderDescriptor` instances matching current label values. Example for git VCS:
```rust
use crate::providers::discovery::ProviderDescriptor;

let git_descriptor = ProviderDescriptor {
    name: "git".into(),
    display_name: "git".into(),
    abbreviation: "".into(),
    section_label: "".into(),
    item_noun: "".into(),
};
registry.vcs.insert("git".into(), (git_descriptor, Arc::new(git_vcs)));
```

Replicate for each provider, using the values currently returned by their `display_name()`, `section_label()`, etc. methods.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace --locked`
Expected: All tests pass with the updated types.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "refactor: add ProviderDescriptor to ProviderRegistry (#171)"
```

---

### Task 4: Remove label methods from provider traits

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/mod.rs:17,44-53` — remove `display_name` from Vcs, remove `display_name`/`section_label`/`item_noun`/`abbreviation` from CheckoutManager
- Modify: `crates/flotilla-core/src/providers/code_review/mod.rs:9-18` — remove 4 methods from CodeReview
- Modify: `crates/flotilla-core/src/providers/issue_tracker/mod.rs:9-18` — remove 4 methods from IssueTracker
- Modify: `crates/flotilla-core/src/providers/coding_agent/mod.rs:17-26` — remove 4 methods from CloudAgentService
- Modify: `crates/flotilla-core/src/providers/ai_utility/mod.rs:7` — remove `display_name` from AiUtility
- Modify: `crates/flotilla-core/src/providers/workspace/mod.rs:13` — remove `display_name` from WorkspaceManager
- Modify: `crates/flotilla-core/src/providers/terminal/mod.rs:11` — remove `display_name` from TerminalPool
- Modify: All provider implementations — remove `display_name()` and label method implementations

- [ ] **Step 1: Remove methods from all 8 trait definitions**

Remove the listed methods from each trait. These are the trait definitions only — not the implementations yet.

- [ ] **Step 2: Remove method implementations from all provider structs**

Find and remove `fn display_name`, `fn section_label`, `fn item_noun`, `fn abbreviation` implementations. Only `display_name` is abstract on all traits; `section_label`/`item_noun`/`abbreviation` have defaults on `CheckoutManager`, `CodeReview`, `IssueTracker`, and `CloudAgentService` — so only impls that override the defaults need touching. Use `cargo check` to find all remaining implementations after the trait methods are removed. Key files:

- `crates/flotilla-core/src/providers/vcs/git.rs` — `display_name` on Vcs impl
- `crates/flotilla-core/src/providers/vcs/wt.rs` — `display_name` on CheckoutManager impl (may override labels too)
- `crates/flotilla-core/src/providers/vcs/git_worktree.rs` — `display_name` on CheckoutManager impl
- `crates/flotilla-core/src/providers/code_review/github.rs` — `display_name` + overridden labels on CodeReview impl
- `crates/flotilla-core/src/providers/issue_tracker/github.rs` — `display_name` on IssueTracker impl (uses trait defaults for labels)
- `crates/flotilla-core/src/providers/coding_agent/claude.rs` — `display_name` on CloudAgentService impl (uses trait defaults for labels)
- `crates/flotilla-core/src/providers/coding_agent/cursor.rs` — `display_name` on CloudAgentService impl
- `crates/flotilla-core/src/providers/coding_agent/codex.rs` — `display_name` on CloudAgentService impl
- `crates/flotilla-core/src/providers/ai_utility/claude.rs` — `display_name` on AiUtility impl
- `crates/flotilla-core/src/providers/workspace/cmux.rs` — `display_name` on WorkspaceManager impl
- `crates/flotilla-core/src/providers/workspace/tmux.rs` — `display_name` on WorkspaceManager impl
- `crates/flotilla-core/src/providers/workspace/zellij.rs` — `display_name` on WorkspaceManager impl
- `crates/flotilla-core/src/providers/terminal/shpool.rs` — `display_name` on TerminalPool impl
- `crates/flotilla-core/src/providers/terminal/passthrough.rs` — `display_name` on TerminalPool impl
- Test mocks and replay implementations in `model.rs` tests (~500 lines of test stubs that implement `display_name()` etc.) and `providers/mod.rs` testing module

- [ ] **Step 3: Fix compilation errors**

Run `cargo check` and fix any remaining references. The `labels_from_registry` and `provider_names_from_registry` were already updated in Task 3 to use descriptors, so those should be fine.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace --locked`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "refactor: remove label methods from provider traits, use ProviderDescriptor (#171)"
```

---

## Chunk 2: Detectors

### Task 5: Host detectors — Git, GhCli, Claude, Cursor

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/detectors/git.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/github.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/claude.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/cursor.rs`
- Modify: `crates/flotilla-core/src/providers/discovery/detectors/mod.rs`

Each detector follows the same pattern: implement `HostDetector`, use `CommandRunner::exists()` for binary checks and `CommandRunner::run()` for version/output parsing. The existing `DiscoveryMockRunner` in `discovery_legacy.rs` tests has a builder pattern with `on_run()` and `tool_exists()` — port this builder into a shared test utility (e.g. `crates/flotilla-core/src/providers/discovery/test_support.rs`) so all detector and factory tests can use it.

- [ ] **Step 0: Create shared test mock runner**

Create `crates/flotilla-core/src/providers/discovery/test_support.rs` (gated with `#[cfg(test)]`). Port `DiscoveryMockRunner` and `DiscoveryMockRunnerBuilder` from `discovery_legacy.rs` tests (the `builder()`, `on_run()`, `tool_exists()`, `build()` API). This gives all detector and factory tests a command-aware mock runner.

- [ ] **Step 1: Write tests for GitBinaryDetector**

In `crates/flotilla-core/src/providers/discovery/detectors/git.rs`:

```rust
use std::path::{Path, PathBuf};
use async_trait::async_trait;
use crate::providers::CommandRunner;
use crate::providers::discovery::{EnvironmentAssertion, HostDetector, RepoDetector, VcsKind, HostPlatform};

pub struct GitBinaryDetector;

#[async_trait]
impl HostDetector for GitBinaryDetector {
    fn name(&self) -> &str { "git-binary" }

    async fn detect(&self, runner: &dyn CommandRunner) -> Vec<EnvironmentAssertion> {
        todo!()
    }
}

pub struct VcsRepoDetector;

#[async_trait]
impl RepoDetector for VcsRepoDetector {
    fn name(&self) -> &str { "vcs-repo" }

    async fn detect(&self, repo_root: &Path, runner: &dyn CommandRunner) -> Vec<EnvironmentAssertion> {
        todo!()
    }
}

pub struct RemoteHostDetector;

#[async_trait]
impl RepoDetector for RemoteHostDetector {
    fn name(&self) -> &str { "remote-host" }

    async fn detect(&self, repo_root: &Path, runner: &dyn CommandRunner) -> Vec<EnvironmentAssertion> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::discovery::test_support::DiscoveryMockRunner;

    // --- GitBinaryDetector ---

    #[tokio::test]
    async fn git_binary_detector_found() {
        let runner = DiscoveryMockRunner::builder()
            .tool_exists("git", true)
            .on_run("git", &["--version"], Ok("git version 2.43.0\n".into()))
            .build();
        let assertions = GitBinaryDetector.detect(&runner).await;
        assert_eq!(assertions.len(), 1);
        match &assertions[0] {
            EnvironmentAssertion::BinaryAvailable { name, version, .. } => {
                assert_eq!(name, "git");
                assert_eq!(version.as_deref(), Some("2.43.0"));
            }
            other => panic!("expected BinaryAvailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn git_binary_detector_not_found() {
        let runner = DiscoveryMockRunner::builder()
            .tool_exists("git", false)
            .build();
        let assertions = GitBinaryDetector.detect(&runner).await;
        assert!(assertions.is_empty());
    }

    // --- VcsRepoDetector ---

    #[tokio::test]
    async fn vcs_repo_detector_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let runner = DiscoveryMockRunner::builder().build();
        let assertions = VcsRepoDetector.detect(dir.path(), &runner).await;
        assert_eq!(assertions.len(), 1);
        match &assertions[0] {
            EnvironmentAssertion::VcsCheckoutDetected { kind, is_main_checkout, .. } => {
                assert_eq!(*kind, VcsKind::Git);
                assert!(*is_main_checkout); // .git directory = main checkout
            }
            other => panic!("expected VcsCheckoutDetected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn vcs_repo_detector_git_file_is_worktree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".git"), "gitdir: /somewhere").unwrap();
        let runner = DiscoveryMockRunner::builder().build();
        let assertions = VcsRepoDetector.detect(dir.path(), &runner).await;
        match &assertions[0] {
            EnvironmentAssertion::VcsCheckoutDetected { is_main_checkout, .. } => {
                assert!(!is_main_checkout); // .git file = linked worktree
            }
            other => panic!("expected VcsCheckoutDetected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn vcs_repo_detector_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let runner = DiscoveryMockRunner::builder().build();
        let assertions = VcsRepoDetector.detect(dir.path(), &runner).await;
        assert!(assertions.is_empty());
    }

    // --- RemoteHostDetector ---
    // Port tracking_remote_url, first_remote_url, detect_host_from_url,
    // and extract_repo_slug logic. Key tests to port from discovery_legacy:

    #[tokio::test]
    async fn remote_host_detector_github_ssh() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let runner = DiscoveryMockRunner::builder()
            .on_run("git", &["rev-parse", "--abbrev-ref", "@{upstream}"],
                Err("fatal: no upstream".into()))
            .on_run("git", &["remote"], Ok("origin\n".into()))
            .on_run("git", &["remote", "get-url", "origin"],
                Ok("git@github.com:rjwittams/flotilla.git\n".into()))
            .build();
        let assertions = RemoteHostDetector.detect(dir.path(), &runner).await;
        assert_eq!(assertions.len(), 1);
        match &assertions[0] {
            EnvironmentAssertion::RemoteHost { platform, owner, repo, remote_name } => {
                assert_eq!(*platform, HostPlatform::GitHub);
                assert_eq!(owner, "rjwittams");
                assert_eq!(repo, "flotilla");
                assert_eq!(remote_name, "origin");
            }
            other => panic!("expected RemoteHost, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn remote_host_detector_prefers_tracking_remote() {
        // Port first_remote_prefers_tracking_remote test from discovery_legacy
        // Setup: tracking branch points to "upstream" remote, origin also exists
        // Assert: RemoteHost uses the tracking remote
        todo!("port from discovery_legacy::tests::first_remote_prefers_tracking_remote")
    }

    #[tokio::test]
    async fn remote_host_detector_https_url() {
        // Test HTTPS URL parsing: https://github.com/owner/repo.git
        todo!("port from discovery_legacy::tests::extract_repo_slug_cases")
    }

    #[tokio::test]
    async fn remote_host_detector_no_remotes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let runner = DiscoveryMockRunner::builder()
            .on_run("git", &["rev-parse", "--abbrev-ref", "@{upstream}"],
                Err("fatal: no upstream".into()))
            .on_run("git", &["remote"], Ok("".into()))
            .build();
        let assertions = RemoteHostDetector.detect(dir.path(), &runner).await;
        assert!(assertions.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --workspace --locked -- git_binary_detector`
Expected: FAIL — `todo!()` panics.

- [ ] **Step 3: Implement all three detectors**

`GitBinaryDetector`: run `which git`, parse path, run `git --version`, parse version string.

`VcsRepoDetector`: check if `repo_root/.git` exists (as directory or file). Contribute `VcsCheckoutDetected { kind: Git, is_main_checkout }` where `is_main_checkout` is true if `.git` is a directory (not a file — worktrees use a `.git` file).

`RemoteHostDetector`: Port the `tracking_remote_url` + `first_remote_url` + `detect_host_from_url` + `extract_repo_slug` logic from `discovery_legacy.rs` (lines 26-152). Run git commands to find remotes, parse URLs, contribute `RemoteHost` assertions.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --workspace --locked -- detectors::git`
Expected: All pass.

- [ ] **Step 5: Implement GhCliDetector, ClaudeDetector, CursorDetector**

In respective files (`github.rs`, `claude.rs`, `cursor.rs`):

`GhCliDetector` — host detector. `which gh`, parse version from `gh --version`.

`ClaudeDetector` — host detector. Port `resolve_claude_path` logic from `providers/mod.rs:402-417`. Contribute `BinaryAvailable { name: "claude", ... }`.

`CursorDetector` — host detector. Check `CURSOR_API_KEY` env var (contribute `EnvVarSet`). Check `which agent` binary (contribute `BinaryAvailable`).

Each with tests following the same pattern as `GitBinaryDetector`.

- [ ] **Step 6: Update detectors/mod.rs with module declarations**

```rust
pub mod git;
pub mod github;
pub mod claude;
pub mod cursor;
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --workspace --locked`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add crates/flotilla-core/src/providers/discovery/detectors/
git commit -m "feat: add host detectors — git, gh, claude, cursor (#171)"
```

---

### Task 6: Host detectors — Cmux, Tmux, Zellij, Shpool + Repo detector — Codex

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/detectors/cmux.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/tmux.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/env.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/shpool.rs`
- Create: `crates/flotilla-core/src/providers/discovery/detectors/codex.rs`
- Modify: `crates/flotilla-core/src/providers/discovery/detectors/mod.rs`

- [ ] **Step 1: Write tests for CmuxDetector**

`CmuxDetector`: check `CMUX_SOCKET_PATH` env var (contribute `EnvVarSet` + `SocketAvailable`). Check `which cmux` or hardcoded macOS path `/Applications/cmux.app/Contents/Resources/bin/cmux` (contribute `BinaryAvailable`).

- [ ] **Step 2: Implement CmuxDetector, run tests**

- [ ] **Step 3: Write tests and implement TmuxDetector**

`TmuxDetector`: check `TMUX` env var (contribute `EnvVarSet`).

- [ ] **Step 4: Write tests and implement ZellijDetector**

In `env.rs`. `ZellijDetector`: check `ZELLIJ` env var (contribute `EnvVarSet`). Run `zellij --version` for version string.

- [ ] **Step 5: Write tests and implement ShpoolDetector**

`ShpoolDetector`: `which shpool` (contribute `BinaryAvailable`).

- [ ] **Step 6: Write tests and implement CodexAuthDetector**

`CodexAuthDetector` — repo detector. Port `codex_auth_file_exists` logic from `coding_agent/codex.rs:78`. Contribute `AuthFileExists` if the auth file exists.

- [ ] **Step 7: Update detectors/mod.rs**

Add module declarations for all new detector files.

- [ ] **Step 8: Run all tests**

Run: `cargo test --workspace --locked`
Expected: All pass.

- [ ] **Step 9: Commit**

```bash
git add crates/flotilla-core/src/providers/discovery/detectors/
git commit -m "feat: add remaining detectors — cmux, tmux, zellij, shpool, codex (#171)"
```

---

### Task 7: Registration functions for detectors

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery/detectors/mod.rs`

- [ ] **Step 1: Add default_host_detectors and default_repo_detectors functions**

```rust
use super::{HostDetector, RepoDetector};

pub mod git;
pub mod github;
pub mod claude;
pub mod cursor;
pub mod cmux;
pub mod tmux;
pub mod env;
pub mod shpool;
pub mod codex;

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
```

- [ ] **Step 2: Write a test that the registration functions return non-empty vecs**

```rust
#[test]
fn default_host_detectors_non_empty() {
    assert!(!default_host_detectors().is_empty());
}

#[test]
fn default_repo_detectors_non_empty() {
    assert!(!default_repo_detectors().is_empty());
}
```

- [ ] **Step 3: Run tests, commit**

Run: `cargo test --workspace --locked`

```bash
git add crates/flotilla-core/src/providers/discovery/detectors/
git commit -m "feat: add detector registration functions (#171)"
```

---

## Chunk 3: Factories

### Task 8: VCS and checkout manager factories

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/factories/git.rs`
- Modify: `crates/flotilla-core/src/providers/discovery/factories/mod.rs`

- [ ] **Step 1: Write tests for GitVcsFactory**

```rust
#[tokio::test]
async fn git_vcs_factory_succeeds_with_git_checkout() {
    let bag = bag_with_git_checkout();
    let config = test_config();
    let runner = MockRunner::builder().build();
    let result = GitVcsFactory.probe(&bag, &config, Path::new("/repo"), Arc::new(runner)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn git_vcs_factory_fails_without_checkout() {
    let bag = EnvironmentBag::new();
    let config = test_config();
    let runner = MockRunner::builder().build();
    let result = GitVcsFactory.probe(&bag, &config, Path::new("/repo"), Arc::new(runner)).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains(&UnmetRequirement::NoVcsCheckout));
}
```

- [ ] **Step 2: Implement GitVcsFactory**

Check `env.find_vcs_checkout(VcsKind::Git)`. If found, construct `GitVcs::new(repo_root, runner)`. Return descriptor with `name: "git"`, `display_name: "git"`.

- [ ] **Step 3: Write tests for WtCheckoutManagerFactory and GitCheckoutManagerFactory**

Test config-driven selection: when config says `"wt"`, `GitCheckoutManagerFactory` returns `Err(vec![])` (empty — this is config exclusion, not a missing capability). When config says `"git"`, `WtCheckoutManagerFactory` returns `Err(vec![])`. When config says `"auto"`, both probe the environment normally.

- [ ] **Step 4: Implement both checkout manager factories**

Each checks `config.resolve_checkouts_config(repo_root).provider` — if it names a different provider, return `Err(vec![])` (empty unmet list, since this is a config choice not a missing requirement — it should not pollute the unmet requirements shown to the user). Otherwise, check for the required binary in the bag and construct the provider.

`WtCheckoutManagerFactory`: needs `env.find_binary("wt")`. Descriptor: `name: "wt"`, `section_label: "Checkouts"`, `item_noun: "checkout"`, `abbreviation: "CO"`.

`GitCheckoutManagerFactory`: needs `env.find_binary("git")`. Descriptor: `name: "git"`, same labels.

- [ ] **Step 5: Run tests, commit**

Run: `cargo test --workspace --locked`

```bash
git add crates/flotilla-core/src/providers/discovery/factories/
git commit -m "feat: add VCS and checkout manager factories (#171)"
```

---

### Task 9: GitHub factories (CodeReview + IssueTracker)

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/factories/github.rs`
- Modify: `crates/flotilla-core/src/providers/discovery/factories/mod.rs`

- [ ] **Step 1: Write tests**

Test that factories succeed when bag has `RemoteHost { GitHub, ... }` + `BinaryAvailable { "gh" }`. Test that they fail with `MissingBinary("gh")` or `MissingRemoteHost(GitHub)`.

- [ ] **Step 2: Implement GitHubCodeReviewFactory and GitHubIssueTrackerFactory**

Both check for `env.find_remote_host(HostPlatform::GitHub)` and `env.find_binary("gh")`. Construct shared `GhApiClient` (via `Arc`). A `GitHubFactory` struct implements both traits, sharing the descriptor (`name: "github"`).

`CodeReviewFactory` descriptor: `display_name: "GitHub"`, `section_label: "Pull Requests"`, `item_noun: "pull request"`, `abbreviation: "PR"`.

`IssueTrackerFactory` descriptor: `display_name: "GitHub"`, `section_label: "Issues"`, `item_noun: "issue"`, `abbreviation: "#"`.

- [ ] **Step 3: Run tests, commit**

```bash
git add crates/flotilla-core/src/providers/discovery/factories/github.rs
git commit -m "feat: add GitHub code review and issue tracker factories (#171)"
```

---

### Task 10: Cloud agent factories (Claude, Cursor, Codex) + AI utility factory

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/factories/claude.rs`
- Create: `crates/flotilla-core/src/providers/discovery/factories/cursor.rs`
- Create: `crates/flotilla-core/src/providers/discovery/factories/codex.rs`
- Modify: `crates/flotilla-core/src/providers/discovery/factories/mod.rs`

- [ ] **Step 1: Write tests for ClaudeCodingAgentFactory**

Succeeds when `env.find_binary("claude")` exists. Fails with `MissingBinary("claude")`.

- [ ] **Step 2: Implement ClaudeCodingAgentFactory and ClaudeAiUtilityFactory**

`ClaudeCodingAgentFactory`: check `env.find_binary("claude")`. Construct `ClaudeCodingAgent`. Descriptor: `name: "claude"`, `display_name: "Claude"`, `section_label: "Cloud Agents"`, `item_noun: "agent"`, `abbreviation: "Agt"`.

`ClaudeAiUtilityFactory`: same binary check. Construct `ClaudeAiUtility`. Descriptor: `name: "claude"`, `display_name: "Claude"`.

- [ ] **Step 3: Write tests and implement CursorCodingAgentFactory**

Needs `env.find_binary("agent")` + `env.find_env_var("CURSOR_API_KEY")`. Descriptor: `name: "cursor"`, `display_name: "Cursor"`.

- [ ] **Step 4: Write tests and implement CodexCodingAgentFactory**

Needs `env.has_auth("codex")`. Descriptor: `name: "codex"`, `display_name: "Codex"`.

- [ ] **Step 5: Run tests, commit**

```bash
git add crates/flotilla-core/src/providers/discovery/factories/claude.rs \
       crates/flotilla-core/src/providers/discovery/factories/cursor.rs \
       crates/flotilla-core/src/providers/discovery/factories/codex.rs
git commit -m "feat: add cloud agent and AI utility factories (#171)"
```

---

### Task 11: Workspace manager and terminal pool factories

**Files:**
- Create: `crates/flotilla-core/src/providers/discovery/factories/cmux.rs`
- Create: `crates/flotilla-core/src/providers/discovery/factories/tmux.rs`
- Create: `crates/flotilla-core/src/providers/discovery/factories/zellij.rs`
- Create: `crates/flotilla-core/src/providers/discovery/factories/shpool.rs`
- Create: `crates/flotilla-core/src/providers/discovery/factories/passthrough.rs`
- Modify: `crates/flotilla-core/src/providers/discovery/factories/mod.rs`

- [ ] **Step 1: Write tests for CmuxWorkspaceManagerFactory**

Succeeds when `env.find_env_var("CMUX_SOCKET_PATH")` or `env.find_binary("cmux")` exists. Test env var path takes priority.

- [ ] **Step 2: Implement CmuxWorkspaceManagerFactory**

Check for `CMUX_SOCKET_PATH` env var first (socket path), then `cmux` binary. Descriptor: `name: "cmux"`, `display_name: "cmux"`.

- [ ] **Step 3: Write tests and implement TmuxWorkspaceManagerFactory**

Needs `env.find_env_var("TMUX")`. Descriptor: `name: "tmux"`, `display_name: "tmux"`.

- [ ] **Step 4: Write tests and implement ZellijWorkspaceManagerFactory**

Needs `env.find_env_var("ZELLIJ")`. Port version checking from current code. Descriptor: `name: "zellij"`, `display_name: "Zellij"`.

- [ ] **Step 5: Write tests and implement ShpoolTerminalPoolFactory**

Needs `env.find_binary("shpool")`. Handles async `ShpoolTerminalPool::create` during `probe`. Descriptor: `name: "shpool"`, `display_name: "shpool"`.

- [ ] **Step 6: Implement PassthroughTerminalPoolFactory**

Always succeeds (unconditional fallback). Descriptor: `name: "passthrough"`, `display_name: "passthrough"`.

- [ ] **Step 7: Run tests, commit**

```bash
git add crates/flotilla-core/src/providers/discovery/factories/
git commit -m "feat: add workspace manager and terminal pool factories (#171)"
```

---

### Task 12: Registration functions for factories

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery/factories/mod.rs`

- [ ] **Step 1: Add all default_*_factories functions and FactoryRegistry::default**

```rust
pub mod git;
pub mod github;
pub mod claude;
pub mod cursor;
pub mod codex;
pub mod cmux;
pub mod tmux;
pub mod zellij;
pub mod shpool;
pub mod passthrough;

use super::*;

impl FactoryRegistry {
    pub fn default_all() -> Self {
        Self {
            vcs: vec![Box::new(git::GitVcsFactory)],
            checkout_managers: vec![
                Box::new(git::WtCheckoutManagerFactory),
                Box::new(git::GitCheckoutManagerFactory),
            ],
            code_review: vec![Box::new(github::GitHubCodeReviewFactory)],
            issue_trackers: vec![Box::new(github::GitHubIssueTrackerFactory)],
            cloud_agents: vec![
                Box::new(claude::ClaudeCodingAgentFactory),
                Box::new(cursor::CursorCodingAgentFactory),
                Box::new(codex::CodexCodingAgentFactory),
            ],
            ai_utilities: vec![Box::new(claude::ClaudeAiUtilityFactory)],
            workspace_managers: vec![
                Box::new(cmux::CmuxWorkspaceManagerFactory),
                Box::new(zellij::ZellijWorkspaceManagerFactory),
                Box::new(tmux::TmuxWorkspaceManagerFactory),
            ],
            terminal_pools: vec![
                Box::new(shpool::ShpoolTerminalPoolFactory),
                Box::new(passthrough::PassthroughTerminalPoolFactory),
            ],
        }
    }

    pub fn for_follower() -> Self {
        Self {
            vcs: vec![Box::new(git::GitVcsFactory)],
            checkout_managers: vec![
                Box::new(git::WtCheckoutManagerFactory),
                Box::new(git::GitCheckoutManagerFactory),
            ],
            code_review: vec![],
            issue_trackers: vec![],
            cloud_agents: vec![],
            ai_utilities: vec![],
            workspace_managers: vec![
                Box::new(cmux::CmuxWorkspaceManagerFactory),
                Box::new(zellij::ZellijWorkspaceManagerFactory),
                Box::new(tmux::TmuxWorkspaceManagerFactory),
            ],
            terminal_pools: vec![
                Box::new(shpool::ShpoolTerminalPoolFactory),
                Box::new(passthrough::PassthroughTerminalPoolFactory),
            ],
        }
    }
}
```

- [ ] **Step 2: Run tests, commit**

```bash
git add crates/flotilla-core/src/providers/discovery/factories/mod.rs
git commit -m "feat: add factory registration and FactoryRegistry::default_all (#171)"
```

---

## Chunk 4: Orchestrator and Integration

### Task 13: Orchestrator — discover_providers

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery/mod.rs` — add `DiscoveryResult` and `discover_providers` function

- [ ] **Step 1: Add `repo_identity()` method to EnvironmentBag**

In `crates/flotilla-core/src/providers/discovery/mod.rs`, add to `EnvironmentBag` impl:

```rust
pub fn repo_identity(&self) -> Option<flotilla_protocol::RepoIdentity> {
    // Find the preferred RemoteHost assertion and construct RepoIdentity
    // HostPlatform::GitHub → authority "github.com", HostPlatform::GitLab → "gitlab.com"
    self.assertions.iter().find_map(|a| match a {
        EnvironmentAssertion::RemoteHost { platform, owner, repo, .. } => {
            let authority = match platform {
                HostPlatform::GitHub => "github.com",
                HostPlatform::GitLab => "gitlab.com",
            };
            Some(flotilla_protocol::RepoIdentity {
                authority: authority.into(),
                path: format!("{owner}/{repo}"),
            })
        }
        _ => None,
    })
}
```

Add a test:
```rust
#[test]
fn repo_identity_from_github_remote() {
    let mut bag = EnvironmentBag::new();
    bag.push(EnvironmentAssertion::RemoteHost {
        platform: HostPlatform::GitHub,
        owner: "rjwittams".into(),
        repo: "flotilla".into(),
        remote_name: "origin".into(),
    });
    let identity = bag.repo_identity().unwrap();
    assert_eq!(identity.authority, "github.com");
    assert_eq!(identity.path, "rjwittams/flotilla");
}
```

- [ ] **Step 2: Write orchestrator integration tests**

```rust
#[tokio::test]
async fn discover_providers_with_git_repo() {
    let mut host_bag = EnvironmentBag::new();
    host_bag.push(EnvironmentAssertion::BinaryAvailable {
        name: "git".into(),
        path: PathBuf::from("/usr/bin/git"),
        version: Some("2.43.0".into()),
    });

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();

    let runner = Arc::new(DiscoveryMockRunner::builder().build());
    let config = ConfigStore::empty();
    let repo_detectors = detectors::default_repo_detectors();
    let factories = FactoryRegistry::default_all();

    let result = discover_providers(
        &host_bag, dir.path(), &repo_detectors, &factories, &config, runner,
    ).await;

    assert!(result.registry.vcs.contains_key("git"));
    assert!(result.bag.find_binary("git").is_some()); // merged bag
    assert!(result.bag.find_vcs_checkout(VcsKind::Git).is_some());
}

#[tokio::test]
async fn discover_providers_checkout_manager_first_wins() {
    // Host bag with both "wt" and "git" binaries, config set to "auto"
    // Factories registered: [WtCheckoutManagerFactory, GitCheckoutManagerFactory]
    // Assert only one checkout manager in registry (wt wins, it's first)
    let mut host_bag = EnvironmentBag::new();
    host_bag.push(EnvironmentAssertion::BinaryAvailable {
        name: "wt".into(), path: PathBuf::from("/usr/bin/wt"), version: None,
    });
    host_bag.push(EnvironmentAssertion::BinaryAvailable {
        name: "git".into(), path: PathBuf::from("/usr/bin/git"), version: None,
    });
    // ... setup repo, runner, auto config, run discover_providers
    // assert_eq!(result.registry.checkout_managers.len(), 1);
    // assert!(result.registry.checkout_managers.contains_key("wt"));
}

#[tokio::test]
async fn discover_providers_collects_unmet_requirements() {
    // Empty host bag — no binaries. Factories will fail.
    let host_bag = EnvironmentBag::new();
    // ... setup repo with .git but no remotes, minimal factories
    // assert!(!result.unmet.is_empty());
}

#[tokio::test]
async fn discover_providers_follower_mode() {
    // Use FactoryRegistry::for_follower()
    // Assert code_review and issue_tracker and cloud_agents are empty
    let factories = FactoryRegistry::for_follower();
    assert!(factories.code_review.is_empty());
    assert!(factories.issue_trackers.is_empty());
    assert!(factories.cloud_agents.is_empty());
}
```

- [ ] **Step 2: Implement discover_providers**

```rust
pub struct DiscoveryResult {
    pub registry: ProviderRegistry,
    pub bag: EnvironmentBag,
    pub repo_slug: Option<String>,
    pub unmet: Vec<UnmetRequirement>,
}

pub async fn run_host_detectors(
    detectors: &[Box<dyn HostDetector>],
    runner: &dyn CommandRunner,
) -> EnvironmentBag {
    let mut bag = EnvironmentBag::new();
    for detector in detectors {
        let assertions = detector.detect(runner).await;
        bag.extend(assertions);
    }
    bag
}

pub async fn discover_providers(
    host_bag: &EnvironmentBag,
    repo_root: &Path,
    repo_detectors: &[Box<dyn RepoDetector>],
    factories: &FactoryRegistry,
    config: &ConfigStore,
    runner: Arc<dyn CommandRunner>,
) -> DiscoveryResult {
    // Phase 1: run repo detectors
    let mut repo_bag = EnvironmentBag::new();
    for detector in repo_detectors {
        let assertions = detector.detect(repo_root, &*runner).await;
        repo_bag.extend(assertions);
    }
    let combined = host_bag.merge(&repo_bag);

    // Phase 2: run factories
    let mut registry = ProviderRegistry::new();
    let mut unmet = Vec::new();

    // VCS — all factories
    for factory in &factories.vcs {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.vcs.insert(desc.name.clone(), (desc, provider));
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // Checkout managers — first-wins
    for factory in &factories.checkout_managers {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.checkout_managers.insert(desc.name.clone(), (desc, provider));
                break;
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // Code review — all factories
    for factory in &factories.code_review {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.code_review.insert(desc.name.clone(), (desc, provider));
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // Issue trackers — all factories
    for factory in &factories.issue_trackers {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.issue_trackers.insert(desc.name.clone(), (desc, provider));
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // Cloud agents — all factories
    for factory in &factories.cloud_agents {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.cloud_agents.insert(desc.name.clone(), (desc, provider));
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // AI utilities — all factories
    for factory in &factories.ai_utilities {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.ai_utilities.insert(desc.name.clone(), (desc, provider));
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // Workspace managers — first-wins
    for factory in &factories.workspace_managers {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.workspace_manager = Some((desc, provider));
                break;
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    // Terminal pools — first-wins
    for factory in &factories.terminal_pools {
        match factory.probe(&combined, config, repo_root, runner.clone()).await {
            Ok(provider) => {
                let desc = factory.descriptor();
                registry.terminal_pool = Some((desc, provider));
                break;
            }
            Err(reqs) => unmet.extend(reqs),
        }
    }

    let repo_slug = combined.repo_slug();

    DiscoveryResult {
        registry,
        bag: combined,
        repo_slug,
        unmet,
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace --locked`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/providers/discovery/mod.rs
git commit -m "feat: add discover_providers orchestrator (#171)"
```

---

### Task 14: Wire up callers — replace detect_providers with discover_providers

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs:329-343,1221-1238` — switch to new pipeline
- Modify: `crates/flotilla-core/src/providers/mod.rs` — remove `discovery_legacy` module

- [ ] **Step 1: Add discovery fields to InProcessDaemon**

Add three fields to `InProcessDaemon` struct (around line 271 of `in_process.rs`):

```rust
host_bag: EnvironmentBag,
repo_detectors: Vec<Box<dyn RepoDetector>>,
factories: FactoryRegistry,
```

These are computed once in the constructor and reused in `add_repo`.

- [ ] **Step 2: Update InProcessDaemon::new to use the new pipeline**

In `in_process.rs`, the constructor needs to:
1. Run host detectors once before the repo loop
2. Call `discover_providers` per repo instead of `detect_providers`
3. Replace `first_remote_url`/`extract_repo_identity` with `bag.repo_identity()`

Replace the `detect_providers` call and surrounding code:

```rust
use crate::providers::discovery::{
    self, detectors, DiscoveryResult, EnvironmentBag, FactoryRegistry,
};

// Before the repo loop (around line 325):
let host_detectors = detectors::default_host_detectors();
let repo_detectors = detectors::default_repo_detectors();
let host_bag = discovery::run_host_detectors(&host_detectors, &*runner).await;
let factories = if follower {
    FactoryRegistry::for_follower()
} else {
    FactoryRegistry::default_all()
};

// Inside the repo loop, replace both detect_providers + first_remote_url/extract_repo_identity:
let DiscoveryResult { registry, repo_slug, bag, unmet } =
    discovery::discover_providers(
        &host_bag,
        &path,
        &repo_detectors,
        &factories,
        &config,
        Arc::clone(&runner),
    ).await;
// repo_slug is now Option<String> directly
// RepoIdentity comes from the bag instead of standalone helpers:
let repo_identity = bag.repo_identity();
```

Store `host_bag`, `repo_detectors`, and `factories` on `self` for reuse by `add_repo`.

- [ ] **Step 3: Update add_repo method**

Replace the second call site (lines 1221-1227) to use the stored fields:

```rust
let DiscoveryResult { registry, repo_slug, bag, unmet } =
    discovery::discover_providers(
        &self.host_bag,
        &path,
        &self.repo_detectors,
        &self.factories,
        &self.config,
        Arc::clone(&self.runner),
    ).await;
let repo_identity = bag.repo_identity();
```

- [ ] **Step 4: Remove discovery_legacy module**

Delete `crates/flotilla-core/src/providers/discovery_legacy.rs`. Remove `pub mod discovery_legacy;` from `providers/mod.rs`. The standalone helpers (`first_remote_url`, `extract_repo_slug`, `extract_repo_identity`) have been absorbed into `RemoteHostDetector` and `EnvironmentBag`. The ~750 lines of legacy tests are superseded by the new detector/factory/orchestrator tests. If any code outside discovery still calls the standalone helpers, port those call sites to use the bag or re-export from the new module.

- [ ] **Step 5: Run full test suite**

Run: `cargo test --workspace --locked`
Expected: All tests pass.

- [ ] **Step 6: Run clippy and fmt**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings`
Expected: Clean.

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "feat: wire up modular discovery pipeline, remove legacy detect_providers (#171)"
```

---

### Task 15: Final cleanup and verification

**Files:**
- Various — any remaining references to old code

- [ ] **Step 1: Verify no references to discovery_legacy remain**

Run: `grep -r "discovery_legacy" crates/`
Expected: No matches.

- [ ] **Step 2: Verify no provider traits have display/label methods**

Run: `grep -n "fn display_name\|fn section_label\|fn item_noun\|fn abbreviation" crates/flotilla-core/src/providers/*/mod.rs`
Expected: No matches in trait definitions (only in descriptor).

- [ ] **Step 3: Run the full validation suite**

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
Expected: All pass.

- [ ] **Step 4: Final commit if any cleanup was needed**

```bash
git add -u
git commit -m "chore: final cleanup for modular provider discovery (#171)"
```
