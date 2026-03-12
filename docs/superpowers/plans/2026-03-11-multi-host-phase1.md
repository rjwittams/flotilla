# Multi-Host Phase 1: Read-Only Visibility — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** See work items from remote hosts in the local flotilla TUI — checkouts, branches, and workspaces merged into unified repo tabs via daemon-to-daemon replication.

**Architecture:** The local daemon (leader) SSH-forwards to remote daemon sockets (followers), exchanges raw `ProviderData` via a new `Message::PeerData` protocol variant, merges data by matching repos on `RepoIdentity`, and presents a unified snapshot to the TUI. Followers report only local state; the leader relays between followers.

**Tech Stack:** Rust, tokio, serde, SSH (child process), Unix domain sockets

**Spec:** `docs/superpowers/specs/2026-03-11-multi-host-phase1-design.md`

---

## Chunk 1: Foundation Types

New types in `flotilla-protocol` that the rest of the feature depends on.

### Task 1: Add `HostName` type

**Files:**
- Create: `crates/flotilla-protocol/src/host.rs`
- Modify: `crates/flotilla-protocol/src/lib.rs` (add module + re-export)

- [ ] **Step 1: Write tests for HostName**

In `crates/flotilla-protocol/src/host.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_name_display() {
        let h = HostName::new("desktop");
        assert_eq!(h.as_str(), "desktop");
        assert_eq!(format!("{h}"), "desktop");
    }

    #[test]
    fn host_name_equality() {
        let a = HostName::new("desktop");
        let b = HostName::new("desktop");
        let c = HostName::new("laptop");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn host_name_serde_roundtrip() {
        let h = HostName::new("cloud-vm");
        let json = serde_json::to_string(&h).unwrap();
        assert_eq!(json, "\"cloud-vm\"");
        let back: HostName = serde_json::from_str(&json).unwrap();
        assert_eq!(h, back);
    }
}
```

- [ ] **Step 2: Implement HostName**

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostName(String);

impl HostName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create a HostName from the local machine's hostname.
    /// Uses `gethostname` crate (already a dependency in flotilla-core).
    pub fn local() -> Self {
        let name = gethostname::gethostname()
            .into_string()
            .unwrap_or_else(|_| "localhost".to_string());
        Self(name)
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
```

- [ ] **Step 3: Add `gethostname` dependency to flotilla-protocol**

In `crates/flotilla-protocol/Cargo.toml`, add:
```toml
gethostname = "0.5"
```

Note: `gethostname` is already used in `flotilla-core`. Reuse the same crate rather than adding a second hostname crate.

- [ ] **Step 4: Wire up module in lib.rs**

In `crates/flotilla-protocol/src/lib.rs`, add:
```rust
mod host;
pub use host::HostName;
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p flotilla-protocol`
Expected: All tests pass including new HostName tests.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: add HostName type to flotilla-protocol"
```

---

### Task 2: Add `HostPath` type

**Files:**
- Modify: `crates/flotilla-protocol/src/host.rs`

- [ ] **Step 1: Write tests for HostPath**

Append to `crates/flotilla-protocol/src/host.rs` tests:

```rust
#[test]
fn host_path_display_format() {
    let hp = HostPath {
        host: HostName::new("desktop"),
        path: PathBuf::from("/Users/dev/project"),
    };
    assert_eq!(format!("{hp}"), "desktop:/Users/dev/project");
}

#[test]
fn host_path_equality_different_hosts() {
    let a = HostPath {
        host: HostName::new("laptop"),
        path: PathBuf::from("/home/dev/repo"),
    };
    let b = HostPath {
        host: HostName::new("desktop"),
        path: PathBuf::from("/home/dev/repo"),
    };
    assert_ne!(a, b); // same path, different host = different identity
}

#[test]
fn host_path_serde_roundtrip() {
    let hp = HostPath {
        host: HostName::new("cloud"),
        path: PathBuf::from("/opt/repos/app"),
    };
    let json = serde_json::to_string(&hp).unwrap();
    let back: HostPath = serde_json::from_str(&json).unwrap();
    assert_eq!(hp, back);
}
```

- [ ] **Step 2: Implement HostPath**

In `crates/flotilla-protocol/src/host.rs`:

```rust
use std::path::PathBuf;

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct HostPath {
    pub host: HostName,
    pub path: PathBuf,
}

impl HostPath {
    pub fn new(host: HostName, path: impl Into<PathBuf>) -> Self {
        Self {
            host,
            path: path.into(),
        }
    }
}

impl fmt::Display for HostPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.path.display())
    }
}
```

- [ ] **Step 3: Export from lib.rs**

Add `HostPath` to the `pub use host::` line.

- [ ] **Step 4: Run tests, commit**

```bash
cargo test -p flotilla-protocol
git add -A && git commit -m "feat: add HostPath type to flotilla-protocol"
```

---

### Task 3: Add `RepoIdentity` type

**Files:**
- Modify: `crates/flotilla-protocol/src/host.rs`

- [ ] **Step 1: Write tests for RepoIdentity**

```rust
#[test]
fn repo_identity_from_github_ssh() {
    let id = RepoIdentity::from_remote_url("git@github.com:rjwittams/flotilla.git");
    assert_eq!(
        id,
        Some(RepoIdentity {
            authority: "github.com".into(),
            path: "rjwittams/flotilla".into(),
        })
    );
}

#[test]
fn repo_identity_from_github_https() {
    let id = RepoIdentity::from_remote_url("https://github.com/rjwittams/flotilla.git");
    assert_eq!(
        id,
        Some(RepoIdentity {
            authority: "github.com".into(),
            path: "rjwittams/flotilla".into(),
        })
    );
}

#[test]
fn repo_identity_ssh_and_https_match() {
    let ssh = RepoIdentity::from_remote_url("git@github.com:owner/repo.git").unwrap();
    let https = RepoIdentity::from_remote_url("https://github.com/owner/repo.git").unwrap();
    assert_eq!(ssh, https);
}

#[test]
fn repo_identity_different_authorities() {
    let gh = RepoIdentity::from_remote_url("git@github.com:team/project.git").unwrap();
    let gl = RepoIdentity::from_remote_url("git@gitlab.company.com:team/project.git").unwrap();
    assert_ne!(gh, gl); // same path, different authority
}

#[test]
fn repo_identity_unknown_format() {
    let id = RepoIdentity::from_remote_url("file:///local/repo");
    assert_eq!(
        id,
        Some(RepoIdentity {
            authority: "unknown".into(),
            path: "file:///local/repo".into(),
        })
    );
}

#[test]
fn repo_identity_display() {
    let id = RepoIdentity {
        authority: "github.com".into(),
        path: "rjwittams/flotilla".into(),
    };
    assert_eq!(format!("{id}"), "github.com:rjwittams/flotilla");
}
```

- [ ] **Step 2: Implement RepoIdentity**

```rust
#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepoIdentity {
    pub authority: String,
    pub path: String,
}

impl RepoIdentity {
    /// Extract a RepoIdentity from a git remote URL.
    ///
    /// Handles SSH (`git@github.com:owner/repo.git`) and HTTPS
    /// (`https://github.com/owner/repo.git`). Unknown formats get
    /// authority "unknown" with the full URL as path.
    pub fn from_remote_url(url: &str) -> Option<Self> {
        // SSH format: git@host:owner/repo.git
        if let Some(rest) = url.strip_prefix("git@") {
            if let Some((host, path)) = rest.split_once(':') {
                let path = path.trim_end_matches(".git");
                return Some(Self {
                    authority: host.to_string(),
                    path: path.to_string(),
                });
            }
        }

        // HTTPS format: https://host/owner/repo.git
        if url.starts_with("https://") || url.starts_with("http://") {
            if let Ok(parsed) = url::Url::parse(url) {
                if let Some(host) = parsed.host_str() {
                    let path = parsed.path().trim_start_matches('/').trim_end_matches(".git");
                    if !path.is_empty() {
                        return Some(Self {
                            authority: host.to_string(),
                            path: path.to_string(),
                        });
                    }
                }
            }
        }

        // SSH shorthand: ssh://git@host/owner/repo.git
        if url.starts_with("ssh://") {
            if let Ok(parsed) = url::Url::parse(url) {
                if let Some(host) = parsed.host_str() {
                    let path = parsed.path().trim_start_matches('/').trim_end_matches(".git");
                    if !path.is_empty() {
                        return Some(Self {
                            authority: host.to_string(),
                            path: path.to_string(),
                        });
                    }
                }
            }
        }

        // Unknown format — fallback
        Some(Self {
            authority: "unknown".to_string(),
            path: url.to_string(),
        })
    }
}

impl fmt::Display for RepoIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.authority, self.path)
    }
}
```

- [ ] **Step 3: Add `url` dependency**

In `crates/flotilla-protocol/Cargo.toml`:
```toml
url = "2"
```

- [ ] **Step 4: Export, run tests, commit**

```bash
cargo test -p flotilla-protocol
git add -A && git commit -m "feat: add RepoIdentity type with URL parsing"
```

---

## Chunk 2: ProviderData and Correlation Migration to HostPath

Migrate `ProviderData.checkouts` keys, `CorrelationKey::CheckoutPath`, and `WorkItemIdentity::Checkout` from bare `PathBuf` to `HostPath`. This is a cross-crate change that touches many files.

### Task 4: Migrate ProviderData.checkouts to HostPath keys

**Files:**
- Modify: `crates/flotilla-protocol/src/provider_data.rs` (`ProviderData.checkouts` key type)
- Modify: `crates/flotilla-protocol/src/provider_data.rs` (`CorrelationKey::CheckoutPath`)
- Modify: `crates/flotilla-protocol/src/snapshot.rs` (`WorkItemIdentity::Checkout`, `CheckoutRef.key`)
- Modify: `crates/flotilla-protocol/src/delta.rs` (`Change::Checkout` key, `Change::WorkItem`)
- Modify: All construction sites across `flotilla-core`, `flotilla-tui`, `flotilla-daemon`

This is a large mechanical change. The approach:

- [ ] **Step 1: Change the types in flotilla-protocol**

In `provider_data.rs`:
```rust
// Change CorrelationKey::CheckoutPath from PathBuf to HostPath
pub enum CorrelationKey {
    Branch(String),
    CheckoutPath(HostPath),  // was PathBuf
    ChangeRequestRef(String, String),
    SessionRef(String, String),
}
```

In `provider_data.rs` ProviderData:
```rust
pub struct ProviderData {
    pub checkouts: IndexMap<HostPath, Checkout>,  // was IndexMap<PathBuf, Checkout>
    // ... rest unchanged
}
```

In `snapshot.rs`:
```rust
pub enum WorkItemIdentity {
    Checkout(HostPath),  // was PathBuf
    ChangeRequest(String),
    Session(String),
    Issue(String),
    RemoteBranch(String),
}

pub struct CheckoutRef {
    pub key: HostPath,  // was PathBuf
    pub is_main_checkout: bool,
}
```

In `delta.rs`:
```rust
pub enum Change {
    Checkout {
        key: HostPath,  // was PathBuf
        op: EntryOp<Checkout>,
    },
    // ... rest unchanged
}
```

- [ ] **Step 2: Thread `HostName` through provider infrastructure**

Providers need access to `HostName` to construct `HostPath` keys. Add `host_name: HostName` to `RepoModel` and thread it through provider discovery. All providers receive it at construction time.

Key threading path:
- `InProcessDaemon` stores `host_name: HostName` (from config or `HostName::local()`)
- `RepoModel::new()` receives `host_name` parameter
- Each provider's `poll_*` methods use `self.host_name` (or receive it from `RepoModel`)
- Workspace/terminal providers (`cmux.rs`, `tmux.rs`, `zellij.rs`) also need it — they construct `CorrelationKey::CheckoutPath` at lines ~70, ~143, ~190 respectively

- [ ] **Step 3: Fix `WorkItem::checkout_key()` return type (critical cascade)**

`WorkItem::checkout_key()` in `snapshot.rs:100-102` currently returns `Option<&Path>`. After `CheckoutRef.key` becomes `HostPath`, this must return `Option<&HostPath>`:

```rust
pub fn checkout_key(&self) -> Option<&HostPath> {
    self.checkout.as_ref().map(|co| &co.key)
}
```

This cascades to **~15+ call sites** in the TUI that call `.checkout_key()` expecting `&Path`:
- `intent.rs` lines 42, 44, 46, 53, 87, 97, 132, 171 — uses `.to_path_buf()`, `.display()`
- `ui.rs` lines 504, 516, 551, 625 — renders paths, looks up `providers.checkouts.get(key)`
- `key_handlers.rs` — passes to executor
- `executor.rs` lines 33, 135 — filesystem operations on checkout path

At each site, decide whether the code needs `&HostPath` (for map lookups) or `&Path` (for filesystem ops). For filesystem ops, use `checkout_key.path` to get the inner `PathBuf`. For provider data lookups, pass the full `&HostPath`.

- [ ] **Step 4: Fix all remaining compilation errors**

Run `cargo build --workspace 2>&1` and fix every error. Full scope of affected files:

**Protocol crate:**
- `provider_data.rs` — `CorrelationKey::CheckoutPath`, `ProviderData.checkouts`
- `snapshot.rs` — `WorkItemIdentity::Checkout`, `CheckoutRef.key`, `checkout_key()`
- `delta.rs` — `Change::Checkout` key

**Core crate — providers (need HostName access):**
- `providers/vcs/git.rs` — checkout construction
- `providers/vcs/wt.rs` — worktree checkout construction, line 71 `CorrelationKey::CheckoutPath`
- `providers/vcs/git_worktree.rs` — line 145 `CorrelationKey::CheckoutPath`
- `providers/workspace/cmux.rs` — lines 70, 253 `CorrelationKey::CheckoutPath`
- `providers/workspace/tmux.rs` — lines 143, 277 `CorrelationKey::CheckoutPath`
- `providers/workspace/zellij.rs` — lines 190, 303 `CorrelationKey::CheckoutPath`
- `providers/correlation.rs` — `ProviderItemKey::Checkout`, test helpers

**Core crate — data processing:**
- `data.rs` — lines 424, 865 `CorrelationKey::CheckoutPath`, WorkItem building, `group_work_items`
- `convert.rs` — core-to-protocol conversion
- `in_process.rs` — snapshot building, delta diffing
- `delta.rs` — `apply_changes`

**TUI crate (see Step 3 for checkout_key cascade):**
- `app/mod.rs` — snapshot/delta handling
- `app/intent.rs` — checkout_key() extraction (~8 call sites)
- `app/executor.rs` — command execution
- `app/key_handlers.rs` — passes checkout key
- `ui.rs` — rendering checkout paths (~4 call sites)

**All test files** that construct `CorrelationKey::CheckoutPath`, `WorkItemIdentity::Checkout`, or `ProviderData.checkouts`.

- [ ] **Step 3: Run all tests**

```bash
cargo test --workspace
```

- [ ] **Step 4: Run clippy**

```bash
cargo clippy --all-targets --locked -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "refactor: migrate checkout keys from PathBuf to HostPath"
```

---

## Chunk 3: WorkItem Host Provenance and Config

### Task 5: Add `host` field to WorkItem and Snapshot hostname

**Files:**
- Modify: `crates/flotilla-protocol/src/snapshot.rs` (`WorkItem.host`, `Snapshot.host_name`)
- Modify: `crates/flotilla-core/src/data.rs` (set `host` during WorkItem construction)
- Modify: `crates/flotilla-core/src/convert.rs` (propagate `host`)

- [ ] **Step 1: Add fields**

In `snapshot.rs`:
```rust
pub struct Snapshot {
    pub seq: u64,
    pub repo: PathBuf,
    pub work_items: Vec<WorkItem>,
    pub providers: ProviderData,
    pub provider_health: HashMap<String, HashMap<String, bool>>,
    pub errors: Vec<ProviderError>,
    #[serde(default)]
    pub issue_total: Option<u32>,
    #[serde(default)]
    pub issue_has_more: bool,
    #[serde(default)]
    pub issue_search_results: Option<Vec<(String, Issue)>>,
    pub host_name: HostName,  // NEW — daemon's identity
}

pub struct WorkItem {
    // ... existing fields ...
    pub host: HostName,  // NEW — always populated, which host this item originates from
}
```

Per the spec and the project's no-backwards-compatibility phase, these are non-optional. Every WorkItem carries its origin host; every Snapshot identifies its daemon.

- [ ] **Step 2: Set host in WorkItem construction**

In `crates/flotilla-core/src/data.rs`, where `CorrelatedWorkItem` is built, derive `host` from the anchor item's `HostPath`:
```rust
// The checkout's HostPath already carries the host
host: checkout_ref.key.host.clone(),
```

For non-checkout-anchored items (standalone PRs, sessions, issues), use the local hostname (passed to `group_work_items` or `correlate`).

- [ ] **Step 3: Set host_name in Snapshot construction**

In `crates/flotilla-core/src/in_process.rs`, when building `Snapshot`:
```rust
Snapshot {
    // ... existing ...
    host_name: Some(self.host_name.clone()),
}
```

`InProcessDaemon` needs a `host_name: HostName` field, set from config or `HostName::local()`.

- [ ] **Step 4: Fix compilation, run tests, commit**

```bash
cargo test --workspace
cargo clippy --all-targets --locked -- -D warnings
git add -A && git commit -m "feat: add host provenance to WorkItem and Snapshot"
```

---

### Task 6: Host configuration and RepoIdentity extraction

**Files:**
- Modify: `crates/flotilla-core/src/config.rs` (add `load_hosts`, `load_daemon_config`)
- Modify: `crates/flotilla-core/src/providers/discovery.rs` (extend `extract_repo_slug` → `extract_repo_identity`)

- [ ] **Step 1: Write tests for host config parsing**

In `crates/flotilla-core/src/config.rs` tests:

```rust
#[test]
fn parse_hosts_config() {
    let toml = r#"
[hosts.desktop]
hostname = "desktop.local"
user = "robert"
daemon_socket = "/run/user/1000/flotilla/daemon.sock"

[hosts.cloud]
hostname = "10.0.1.50"
daemon_socket = "/home/robert/.config/flotilla/daemon.sock"
"#;
    let config: HostsConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.hosts.len(), 2);
    assert_eq!(config.hosts["desktop"].hostname, "desktop.local");
    assert_eq!(config.hosts["desktop"].user, Some("robert".into()));
    assert_eq!(config.hosts["cloud"].user, None);
}

#[test]
fn parse_daemon_config_follower() {
    let toml = r#"
follower = true
host_name = "my-desktop"
"#;
    let config: DaemonConfig = toml::from_str(toml).unwrap();
    assert!(config.follower);
    assert_eq!(config.host_name, Some("my-desktop".into()));
}
```

- [ ] **Step 2: Implement config types**

```rust
#[derive(Debug, Default, Deserialize)]
pub struct HostsConfig {
    #[serde(default)]
    pub hosts: HashMap<String, RemoteHostConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RemoteHostConfig {
    pub hostname: String,
    pub user: Option<String>,
    pub daemon_socket: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub follower: bool,
    pub host_name: Option<String>,
}
```

Add methods to `ConfigStore`:
```rust
pub fn load_hosts(&self) -> HostsConfig { ... }
pub fn load_daemon_config(&self) -> DaemonConfig { ... }
```

- [ ] **Step 3: Write tests for RepoIdentity extraction**

In `crates/flotilla-core/src/providers/discovery.rs` tests:

```rust
#[test]
fn extract_repo_identity_github_ssh() {
    let id = extract_repo_identity("git@github.com:rjwittams/flotilla.git");
    assert_eq!(id, Some(RepoIdentity {
        authority: "github.com".into(),
        path: "rjwittams/flotilla".into(),
    }));
}
```

- [ ] **Step 4: Implement `extract_repo_identity`**

Delegate to `RepoIdentity::from_remote_url` (already implemented in Task 3). The existing `extract_repo_slug` can call through or be deprecated.

- [ ] **Step 5: Run tests, commit**

```bash
cargo test --workspace
git add -A && git commit -m "feat: add host config parsing and RepoIdentity extraction"
```

---

## Chunk 4: Protocol Extensions

### Task 7: Add `Message::PeerData` and `PeerDataMessage`

**Files:**
- Modify: `crates/flotilla-protocol/src/lib.rs` (`Message` enum, `PeerDataMessage`, `PeerDataKind`)
- Create: `crates/flotilla-protocol/src/peer.rs` (peer-specific types)

- [ ] **Step 1: Write tests for PeerDataMessage serde**

In `crates/flotilla-protocol/src/peer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_data_message_snapshot_roundtrip() {
        let msg = PeerDataMessage {
            origin_host: HostName::new("desktop"),
            repo_identity: RepoIdentity {
                authority: "github.com".into(),
                path: "owner/repo".into(),
            },
            repo_path: PathBuf::from("/home/dev/repo"),
            kind: PeerDataKind::Snapshot {
                data: ProviderData::default(),
                seq: 1,
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: PeerDataMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.origin_host, msg.origin_host);
        assert_eq!(back.repo_identity, msg.repo_identity);
    }

    #[test]
    fn peer_data_message_request_resync() {
        let msg = PeerDataMessage {
            origin_host: HostName::new("cloud"),
            repo_identity: RepoIdentity {
                authority: "github.com".into(),
                path: "owner/repo".into(),
            },
            repo_path: PathBuf::from("/opt/repo"),
            kind: PeerDataKind::RequestResync { since_seq: 5 },
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: PeerDataMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.kind, PeerDataKind::RequestResync { since_seq: 5 }));
    }
}
```

- [ ] **Step 2: Implement PeerDataMessage types**

In `crates/flotilla-protocol/src/peer.rs`:

```rust
use crate::{HostName, RepoIdentity, provider_data::ProviderData};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDataMessage {
    pub origin_host: HostName,
    pub repo_identity: RepoIdentity,
    pub repo_path: PathBuf,
    pub kind: PeerDataKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PeerDataKind {
    #[serde(rename = "snapshot")]
    Snapshot {
        data: ProviderData,
        seq: u64,
    },
    #[serde(rename = "delta")]
    Delta {
        changes: Vec<crate::delta::Change>,  // Reuses Change enum but only provider-data variants
        seq: u64,                            // (Checkout, Branch, Workspace, Session, Issue).
        prev_seq: u64,                       // WorkItem/ProviderHealth/ErrorsChanged are snapshot-level
    },                                       // and must be filtered out before sending peer deltas.
    #[serde(rename = "request_resync")]
    RequestResync {
        since_seq: u64,
    },
}
```

- [ ] **Step 3: Add `PeerData` variant to `Message`**

In `crates/flotilla-protocol/src/lib.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "request")]
    Request { id: u64, method: String, #[serde(default)] params: serde_json::Value },
    #[serde(rename = "response")]
    Response { id: u64, ok: bool, data: Option<serde_json::Value>, error: Option<String> },
    #[serde(rename = "event")]
    Event { event: Box<DaemonEvent> },
    #[serde(rename = "peer_data")]
    PeerData(Box<PeerDataMessage>),  // NEW
}
```

- [ ] **Step 4: Wire up module, run tests, commit**

```bash
cargo test -p flotilla-protocol
git add -A && git commit -m "feat: add Message::PeerData and PeerDataMessage types"
```

---

## Chunk 5: PeerTransport Trait and SSH Implementation

### Task 8: Define `PeerTransport` trait

**Files:**
- Create: `crates/flotilla-daemon/src/peer/mod.rs`
- Create: `crates/flotilla-daemon/src/peer/transport.rs`
- Modify: `crates/flotilla-daemon/src/lib.rs` (add module)

- [ ] **Step 1: Define trait**

In `crates/flotilla-daemon/src/peer/transport.rs`:

```rust
use async_trait::async_trait;
use flotilla_protocol::PeerDataMessage;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
}

#[async_trait]
pub trait PeerTransport: Send + Sync {
    async fn connect(&mut self) -> Result<(), String>;
    async fn disconnect(&mut self) -> Result<(), String>;
    fn status(&self) -> PeerConnectionStatus;

    /// Subscribe to inbound peer data messages.
    async fn subscribe(&mut self) -> Result<mpsc::Receiver<PeerDataMessage>, String>;
    /// Send a peer data message to the remote daemon.
    /// Uses `&self` (not `&mut self`) — implementations use interior mutability
    /// (e.g. `Mutex<mpsc::Sender>`) so the PeerManager can iterate peers and send.
    async fn send(&self, msg: PeerDataMessage) -> Result<(), String>;
}
```

- [ ] **Step 2: Create mod.rs**

```rust
pub mod transport;
pub use transport::{PeerConnectionStatus, PeerTransport};
```

- [ ] **Step 3: Wire up, compile, commit**

```bash
cargo build -p flotilla-daemon
git add -A && git commit -m "feat: define PeerTransport trait"
```

---

### Task 9: SSH transport implementation

**Files:**
- Create: `crates/flotilla-daemon/src/peer/ssh_transport.rs`
- Modify: `crates/flotilla-daemon/src/peer/mod.rs`

- [ ] **Step 1: Implement SshTransport**

The SSH transport:
1. Spawns `ssh -N -L <local-sock>:<remote-sock> user@host` as a child process
2. Connects to the forwarded local socket
3. Reads/writes `Message` JSON lines on the socket
4. Filters for `PeerData` messages inbound, sends `PeerData` outbound
5. Reconnects with capped exponential backoff on failure

```rust
pub struct SshTransport {
    config: RemoteHostConfig,
    host_name: HostName,
    local_socket_path: PathBuf,
    ssh_process: Option<tokio::process::Child>,
    status: PeerConnectionStatus,
    inbound_tx: Option<mpsc::Sender<PeerDataMessage>>,
    outbound_tx: Option<mpsc::Sender<PeerDataMessage>>,
}
```

Key implementation details:
- `connect()`: clean up stale socket, spawn SSH process, wait for socket to appear (poll with short timeout), connect to local socket, spawn reader/writer tasks
- Reader task: reads JSON lines, parses as `Message`, filters `PeerData` variant, sends to `inbound_tx`
- Writer task: receives from `outbound_tx`, serializes as `Message::PeerData`, writes to socket
- `disconnect()`: kill SSH process, clean up socket
- On reader/writer task failure: set status to `Reconnecting`, schedule reconnect with backoff (1s, 2s, 4s, ... capped at 60s)
- `kill_on_drop`: SSH child process must be killed when transport is dropped

- [ ] **Step 2: Write integration test (skipped in CI)**

```rust
#[tokio::test]
#[ignore] // requires SSH setup
async fn ssh_transport_connects() {
    // This test requires a running daemon on localhost
    // Useful for manual testing
}
```

- [ ] **Step 3: Commit**

```bash
cargo build -p flotilla-daemon
git add -A && git commit -m "feat: SSH transport implementation for peer connections"
```

---

## Chunk 6: DaemonServer PeerData Handling and Follower Mode

### Task 10: Accept PeerData messages in DaemonServer

**Files:**
- Modify: `crates/flotilla-daemon/src/server.rs` (`handle_client`, `dispatch_request`)

- [ ] **Step 1: Handle PeerData in the client message loop**

In `handle_client`, when parsing incoming messages, add handling for `Message::PeerData`:

```rust
Message::PeerData(peer_msg) => {
    // Forward to the daemon's peer data channel
    if let Err(e) = peer_data_tx.send(*peer_msg).await {
        warn!("failed to forward peer data: {e}");
    }
}
```

The `DaemonServer` needs a channel for inbound peer data that the `PeerManager` consumes.

- [ ] **Step 2: Add ability to send PeerData to specific clients**

The server needs to track which connected clients are peers (those that have sent a `PeerData` message) and be able to push `PeerData` messages to them for relay. Add a `peer_clients: Arc<Mutex<HashMap<HostName, mpsc::Sender<Message>>>>` to track peer client writers.

- [ ] **Step 3: Commit**

```bash
cargo build -p flotilla-daemon
git add -A && git commit -m "feat: accept PeerData messages in DaemonServer"
```

---

### Task 11: Implement follower mode

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs` (skip external providers when follower)
- Modify: `crates/flotilla-core/src/providers/discovery.rs` (filter providers)

- [ ] **Step 1: Thread follower flag through InProcessDaemon**

Add a `follower: bool` parameter to `InProcessDaemon::new()`. When `true`, provider discovery skips external providers:

```rust
// In provider discovery, when follower mode:
// - Keep: GitVcs, GitWorktreeManager (local VCS)
// - Keep: ShpoolTerminalPool (local terminals)
// - Skip: GithubCodeReview, GithubIssueTracker (external APIs)
// - Skip: ClaudeAgentService, CodexAgentService (external APIs)
```

- [ ] **Step 2: Write test**

```rust
#[tokio::test]
async fn follower_mode_skips_external_providers() {
    let daemon = InProcessDaemon::new_with_options(
        vec![repo_path],
        config,
        InProcessOptions { follower: true, ..Default::default() },
    ).await;
    let state = daemon.get_state(&repo_path).await.unwrap();
    // Should have checkouts (VCS) but no change_requests (GitHub)
    assert!(!state.providers.checkouts.is_empty());
    assert!(state.providers.change_requests.is_empty());
}
```

- [ ] **Step 3: Commit**

```bash
cargo test --workspace
git add -A && git commit -m "feat: implement follower mode for daemon"
```

---

## Chunk 7: PeerManager — Merge, Relay, State Management

### Task 12: PeerManager core structure

**Files:**
- Create: `crates/flotilla-daemon/src/peer/manager.rs`
- Modify: `crates/flotilla-daemon/src/peer/mod.rs`

- [ ] **Step 1: Define PeerManager**

```rust
pub struct PeerManager {
    local_host: HostName,
    peers: HashMap<HostName, Box<dyn PeerTransport>>,
    peer_data: HashMap<HostName, HashMap<RepoIdentity, PerRepoPeerState>>,
    daemon: Arc<InProcessDaemon>,
    config: Arc<ConfigStore>,
}

pub struct PerRepoPeerState {
    pub provider_data: ProviderData,
    pub repo_path: PathBuf,
    pub seq: u64,
}
```

- [ ] **Step 2: Implement peer data ingestion**

When PeerManager receives a `PeerDataMessage`:
1. Store in `peer_data[origin_host][repo_identity]`
2. If leader, relay to other peers (not back to origin)
3. Trigger re-merge and re-correlation on the daemon

```rust
impl PeerManager {
    pub async fn handle_peer_data(&mut self, msg: PeerDataMessage) {
        let origin = msg.origin_host.clone();

        // Store peer state
        let repo_state = self.peer_data
            .entry(origin.clone())
            .or_default()
            .entry(msg.repo_identity.clone())
            .or_insert_with(|| PerRepoPeerState {
                provider_data: ProviderData::default(),
                repo_path: msg.repo_path.clone(),
                seq: 0,
            });

        match msg.kind {
            PeerDataKind::Snapshot { data, seq } => {
                repo_state.provider_data = data;
                repo_state.seq = seq;
            }
            PeerDataKind::Delta { changes, seq, prev_seq } => {
                if prev_seq != repo_state.seq {
                    // Gap detected — request resync
                    self.request_resync(&origin, &msg.repo_identity, repo_state.seq).await;
                    return;
                }
                // apply_changes is in crates/flotilla-core/src/delta.rs
                // Takes changes by value: apply_changes(&mut provider_data, changes)
                flotilla_core::delta::apply_changes(&mut repo_state.provider_data, changes);
                repo_state.seq = seq;
            }
            PeerDataKind::RequestResync { since_seq } => {
                self.send_snapshot_to(&origin, &msg.repo_identity).await;
                return;
            }
        }

        // Relay to other peers (leader only)
        self.relay(&origin, &msg).await;

        // Trigger re-merge on daemon
        self.notify_daemon_merge(&msg.repo_identity).await;
    }
}
```

- [ ] **Step 3: Implement relay logic**

```rust
async fn relay(&self, origin: &HostName, msg: &PeerDataMessage) {
    for (peer_name, transport) in &self.peers {
        if peer_name != origin {
            if let Err(e) = transport.send(msg.clone()).await {
                warn!(peer = %peer_name, "relay failed: {e}");
            }
        }
    }
}
```

- [ ] **Step 4: Commit**

```bash
cargo build -p flotilla-daemon
git add -A && git commit -m "feat: PeerManager with data ingestion and relay"
```

---

### Task 13: Snapshot merging in the daemon

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs` or create `crates/flotilla-daemon/src/peer/merge.rs`

- [ ] **Step 1: Implement merge_provider_data**

Given local `ProviderData` for a repo and a set of `PerRepoPeerState` entries from remote hosts for the same logical repo, produce a merged `ProviderData`:

```rust
pub fn merge_provider_data(
    local: &ProviderData,
    local_host: &HostName,
    peers: &[(HostName, &ProviderData)],
) -> ProviderData {
    let mut merged = local.clone();

    for (peer_host, peer_data) in peers {
        // Merge host-scoped data with HostPath namespacing
        for (host_path, checkout) in &peer_data.checkouts {
            // Remote checkouts already have correct HostPath with peer host
            merged.checkouts.insert(host_path.clone(), checkout.clone());
        }

        for (name, terminal) in &peer_data.managed_terminals {
            let namespaced = format!("{}:{}", peer_host, name);
            merged.managed_terminals.insert(namespaced, terminal.clone());
        }

        // Service-level data (PRs, issues, sessions) comes only from leader
        // Followers don't have this data, so no merge conflict
    }

    merged
}
```

- [ ] **Step 2: Write tests for merge**

```rust
#[test]
fn merge_combines_checkouts_from_multiple_hosts() {
    let local = ProviderData {
        checkouts: indexmap! {
            HostPath::new(HostName::new("laptop"), "/home/dev/repo") => checkout("main"),
        },
        ..Default::default()
    };
    let remote = ProviderData {
        checkouts: indexmap! {
            HostPath::new(HostName::new("desktop"), "/home/dev/repo") => checkout("feature"),
        },
        ..Default::default()
    };
    let merged = merge_provider_data(
        &local,
        &HostName::new("laptop"),
        &[(HostName::new("desktop"), &remote)],
    );
    assert_eq!(merged.checkouts.len(), 2);
}
```

- [ ] **Step 3: Run tests, commit**

```bash
cargo test --workspace
git add -A && git commit -m "feat: ProviderData merge across hosts"
```

---

### Task 14: Wire PeerManager into DaemonServer

**Files:**
- Modify: `crates/flotilla-daemon/src/server.rs`
- Modify: `crates/flotilla-daemon/src/peer/manager.rs`

- [ ] **Step 1: Initialize PeerManager in DaemonServer::new**

Load host config, create SSH transports, start PeerManager. The PeerManager runs as a background task that:
1. Connects to all configured peers
2. Listens for inbound PeerData messages from all connections
3. Processes them through `handle_peer_data`
4. Sends local data changes to all peers

- [ ] **Step 2: Connect PeerManager to daemon event stream**

Subscribe PeerManager to the daemon's `broadcast::Sender<DaemonEvent>`. When the local daemon produces a new snapshot/delta, the PeerManager converts it to `PeerDataMessage` and sends to all peers.

- [ ] **Step 3: Connect inbound peer data to daemon**

When merged data changes, the PeerManager tells the daemon to rebuild its snapshots with the merged provider data. This may require a new method on `InProcessDaemon`:

```rust
pub async fn set_peer_data(
    &self,
    repo_identity: &RepoIdentity,
    merged_providers: ProviderData,
)
```

- [ ] **Step 4: Integration test**

Write a test that creates two `InProcessDaemon` instances and a `PeerManager` that connects them, verifying data flows from one to the other.

- [ ] **Step 5: Commit**

```bash
cargo test --workspace
git add -A && git commit -m "feat: wire PeerManager into DaemonServer lifecycle"
```

---

### Task 15: Remote-only repos get tabs

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs` (virtual repo support)
- Modify: `crates/flotilla-daemon/src/peer/manager.rs` (detect remote-only repos)

- [ ] **Step 1: Handle remote-only repos in PeerManager**

When the PeerManager merges peer data and finds a `RepoIdentity` that has no local repo, it must:
1. Create a virtual `RepoState` in `InProcessDaemon` with a synthetic `PathBuf` key: `PathBuf::from(format!("<remote>/{}/{}", peer_host, peer_repo_path.display()))`
2. The synthetic path must be stable across restarts (deterministic from host + path)
3. Emit `DaemonEvent::RepoAdded` so the TUI creates a tab

- [ ] **Step 2: Ensure InProcessDaemon handles virtual repos**

Virtual repos (no local filesystem path) must not trigger VCS or workspace provider polling. Add a `virtual_repo: bool` flag to `RepoState` that skips local provider initialization.

- [ ] **Step 3: Commit**

```bash
cargo test --workspace
git add -A && git commit -m "feat: remote-only repos appear as tabs with synthetic paths"
```

---

## Chunk 8: TUI Changes

### Task 16: Host in Source column

**Files:**
- Modify: `crates/flotilla-tui/src/ui.rs` (Source column rendering)

- [ ] **Step 1: Update Source column to show host for checkouts**

In the unified table rendering, when displaying a checkout's source, prepend the host name if the checkout's `HostPath.host` differs from the local hostname:

```rust
// When rendering source for a checkout:
let source = if work_item.host == my_host {
    work_item.source.clone().unwrap_or_default()
} else {
    format!("{}:{}", work_item.host, work_item.source.as_deref().unwrap_or(""))
};
```

The TUI receives `host_name` from the `Snapshot` and stores it for comparison.

- [ ] **Step 2: Store daemon hostname in TUI model**

In `crates/flotilla-tui/src/app/mod.rs`, add `my_host: Option<HostName>` to `TuiModel`. Set it when receiving the first `Snapshot`:

```rust
self.model.my_host = Some(snapshot.host_name.clone());
```

- [ ] **Step 3: Commit**

```bash
cargo build -p flotilla-tui
git add -A && git commit -m "feat: show host in Source column for remote checkouts"
```

---

### Task 17: Hosts section in config view

**Files:**
- Modify: `crates/flotilla-tui/src/ui.rs` (config/Flotilla tab rendering)

- [ ] **Step 1: Add hosts status data to TUI model**

In `crates/flotilla-tui/src/app/mod.rs`, add:

```rust
pub struct PeerHostStatus {
    pub name: HostName,
    pub status: PeerConnectionStatus,
    pub last_sync: Option<Instant>,
}

// In TuiModel:
pub peer_hosts: Vec<PeerHostStatus>,
```

The daemon sends peer connection status via events. For now, the TUI receives this from Snapshot metadata or a new event type.

- [ ] **Step 2: Render hosts section**

In `crates/flotilla-tui/src/ui.rs`, in the config view rendering (after provider health), add a "Connected Hosts" section:

```rust
fn render_hosts_status(f: &mut Frame, area: Rect, hosts: &[PeerHostStatus]) {
    // Render each host with status icon:
    // ● connected (green), ○ disconnected (red), ◐ reconnecting (yellow)
}
```

- [ ] **Step 3: Commit**

```bash
cargo build -p flotilla-tui
git add -A && git commit -m "feat: show connected hosts in config view"
```

---

### Task 18: Action filtering by host provenance

**Files:**
- Modify: `crates/flotilla-tui/src/app/intent.rs` (filter actions)
- Modify: `crates/flotilla-tui/src/app/executor.rs` (guard execution)

- [ ] **Step 1: Filter action menu for remote items**

In the action menu builder, check `work_item.host` against `my_host`. If remote, exclude:
- Open terminal / workspace actions
- Delete worktree
- Create checkout

Keep:
- Open PR in browser (if local repo exists)
- Copy branch name

- [ ] **Step 2: Guard executor**

As a safety net, the executor checks `host` before executing filesystem operations. If the item is remote, return an error rather than operating on a nonexistent local path.

- [ ] **Step 3: Commit**

```bash
cargo test --workspace
cargo clippy --all-targets --locked -- -D warnings
cargo fmt
git add -A && git commit -m "feat: filter actions for remote work items"
```

---

## Chunk 9: End-to-End Integration

### Task 19: End-to-end integration test

**Files:**
- Create: `crates/flotilla-daemon/tests/multi_host.rs`

- [ ] **Step 1: Write integration test**

Create two `InProcessDaemon` instances (simulating leader and follower). The follower has a checkout; the leader has a PR on the same branch. Wire them through `PeerManager` and verify:

1. Leader receives follower's checkout data
2. Correlation links the follower's checkout with the leader's PR
3. The merged snapshot contains both items
4. The follower's checkout has `host: "follower-host"`

- [ ] **Step 2: Write relay integration test**

Add a third daemon (second follower). Verify:
1. Leader relays follower-1's data to follower-2
2. Leader does NOT relay follower-1's data back to follower-1
3. All three daemons see the same merged state

- [ ] **Step 3: Run full test suite, commit**

```bash
cargo test --workspace
cargo clippy --all-targets --locked -- -D warnings
cargo fmt
git add -A && git commit -m "test: multi-host integration tests"
```

---

### Task 20: File Phase 2 follow-up issue

- [ ] **Step 1: Create GitHub issue**

```bash
gh issue create -R rjwittams/flotilla \
  --title "Multi-host Phase 2: remote actions and terminal forwarding" \
  --label "enhancement,vision" \
  --body "Follow-up from #33 Phase 1 (read-only visibility).

## Phase 2 scope
- Open terminals on remote hosts (terminal pool abstraction)
- Create checkouts on remote hosts (delegate to remote CheckoutManager)
- Session handoff between hosts

## Depends on
- Phase 1: #33

## Future phases
- Per-provider leader election
- Auto-discovery (mDNS)
- Direct GitHub API (replace gh CLI dependency)
- Local ↔ remote branch correlation
- Alternate transports (direct TCP, WireGuard)"
```

- [ ] **Step 2: Commit any final cleanup**

```bash
cargo fmt
git add -A && git commit -m "chore: final cleanup for multi-host phase 1"
```
