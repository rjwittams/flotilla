# Persistent Terminal Sessions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `TerminalPool` provider that manages persistent terminal processes via shpool, decoupling process lifecycle from workspace manager views.

**Architecture:** New `TerminalPool` provider trait with `ShpoolTerminalPool` (manages a shpool daemon subprocess, shells out to `shpool` CLI) and `PassthroughTerminalPool` (degenerate no-op). Workspace managers use the pool's `attach_command()` to connect panes to persistent sessions. Template format evolves to split `content:` (what to run) from `layout:` (how to display), with role-based slot matching.

**Tech Stack:** Rust, shpool CLI (external binary), serde_json for `shpool list --json` parsing, async-trait for provider trait.

**Design doc:** `docs/plans/2026-03-09-persistent-terminal-sessions-design.md`

---

### Task 1: Protocol types for ManagedTerminal

Add the data types that flow through the protocol layer.

**Files:**
- Modify: `crates/flotilla-protocol/src/provider_data.rs:114-129`

**Step 1: Write test for ManagedTerminal serde roundtrip**

Add to the existing `mod tests` at line 131 in `crates/flotilla-protocol/src/provider_data.rs`:

```rust
#[test]
fn managed_terminal_roundtrip() {
    use crate::test_helpers::assert_roundtrip;

    let id = ManagedTerminalId {
        checkout: "my-feature".into(),
        role: "shell".into(),
        index: 0,
    };
    assert_roundtrip(&id);

    let terminal = ManagedTerminal {
        id: id.clone(),
        role: "shell".into(),
        command: "$SHELL".into(),
        working_directory: PathBuf::from("/Users/dev/project"),
        status: TerminalStatus::Running,
    };
    assert_roundtrip(&terminal);

    // Test all status variants
    assert_roundtrip(&TerminalStatus::Running);
    assert_roundtrip(&TerminalStatus::Disconnected);
    assert_roundtrip(&TerminalStatus::Exited(0));
    assert_roundtrip(&TerminalStatus::Exited(1));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-protocol managed_terminal_roundtrip`
Expected: FAIL — `ManagedTerminalId`, `ManagedTerminal`, `TerminalStatus` not defined.

**Step 3: Add types**

Add before `pub struct Workspace` (line 114) in `crates/flotilla-protocol/src/provider_data.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ManagedTerminalId {
    pub checkout: String,
    pub role: String,
    pub index: u32,
}

impl std::fmt::Display for ManagedTerminalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.checkout, self.role, self.index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalStatus {
    Running,
    Disconnected,
    Exited(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedTerminal {
    pub id: ManagedTerminalId,
    pub role: String,
    pub command: String,
    pub working_directory: PathBuf,
    pub status: TerminalStatus,
}
```

Also add `managed_terminals: IndexMap<String, ManagedTerminal>` to `ProviderData` (after `workspaces` at line 128).

**Step 4: Run test to verify it passes**

Run: `cargo test -p flotilla-protocol managed_terminal_roundtrip`
Expected: PASS

**Step 5: Run full protocol tests and clippy**

Run: `cargo test -p flotilla-protocol && cargo clippy -p flotilla-protocol --all-targets -- -D warnings`
Expected: PASS (delta tests may need updating if `ProviderData` equality checks change — add `managed_terminals: IndexMap::new()` to any test fixtures)

**Step 6: Commit**

```bash
git add -A && git commit -m "feat: add ManagedTerminal protocol types"
```

---

### Task 2: TerminalPool provider trait

Define the trait and module structure.

**Files:**
- Create: `crates/flotilla-core/src/providers/terminal/mod.rs`
- Modify: `crates/flotilla-core/src/providers/mod.rs:1-11` (add `pub mod terminal;`)

**Step 1: Create the trait**

Create `crates/flotilla-core/src/providers/terminal/mod.rs`:

```rust
pub mod passthrough;
pub mod shpool;

use std::path::Path;

use async_trait::async_trait;
use flotilla_protocol::{ManagedTerminal, ManagedTerminalId};

#[async_trait]
pub trait TerminalPool: Send + Sync {
    fn display_name(&self) -> &str;
    async fn list_terminals(&self) -> Result<Vec<ManagedTerminal>, String>;
    async fn ensure_running(
        &self,
        id: &ManagedTerminalId,
        command: &str,
        cwd: &Path,
    ) -> Result<(), String>;
    async fn attach_command(&self, id: &ManagedTerminalId) -> Result<String, String>;
    async fn kill_terminal(&self, id: &ManagedTerminalId) -> Result<(), String>;
}
```

**Step 2: Add module declaration**

In `crates/flotilla-core/src/providers/mod.rs`, add `pub mod terminal;` after `pub mod workspace;` (line 11).

**Step 3: Verify it compiles**

Run: `cargo check -p flotilla-core 2>&1 | head -20`
Expected: Will fail because `passthrough` and `shpool` submodules don't exist yet. Create empty stubs:

Create `crates/flotilla-core/src/providers/terminal/passthrough.rs`:
```rust
// PassthroughTerminalPool — degenerate no-op implementation
```

Create `crates/flotilla-core/src/providers/terminal/shpool.rs`:
```rust
// ShpoolTerminalPool — shpool CLI-backed implementation
```

Run: `cargo check -p flotilla-core`
Expected: PASS (warnings about unused imports are fine for now)

**Step 4: Commit**

```bash
git add -A && git commit -m "feat: add TerminalPool provider trait"
```

---

### Task 3: PassthroughTerminalPool implementation

The degenerate fallback that does nothing.

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/passthrough.rs`

**Step 1: Write test**

Add to `crates/flotilla-core/src/providers/terminal/passthrough.rs`:

```rust
use async_trait::async_trait;
use flotilla_protocol::{ManagedTerminal, ManagedTerminalId};

use super::TerminalPool;

pub struct PassthroughTerminalPool;

#[async_trait]
impl TerminalPool for PassthroughTerminalPool {
    fn display_name(&self) -> &str {
        "passthrough"
    }

    async fn list_terminals(&self) -> Result<Vec<ManagedTerminal>, String> {
        Ok(vec![])
    }

    async fn ensure_running(
        &self,
        _id: &ManagedTerminalId,
        _command: &str,
        _cwd: &std::path::Path,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn attach_command(&self, _id: &ManagedTerminalId) -> Result<String, String> {
        Err("passthrough pool: no attach command available".into())
    }

    async fn kill_terminal(&self, _id: &ManagedTerminalId) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passthrough_list_returns_empty() {
        let pool = PassthroughTerminalPool;
        let terminals = pool.list_terminals().await.unwrap();
        assert!(terminals.is_empty());
    }

    #[tokio::test]
    async fn passthrough_ensure_running_is_noop() {
        let pool = PassthroughTerminalPool;
        let id = ManagedTerminalId {
            checkout: "test".into(),
            role: "shell".into(),
            index: 0,
        };
        assert!(pool.ensure_running(&id, "bash", "/tmp".as_ref()).await.is_ok());
    }

    #[tokio::test]
    async fn passthrough_attach_command_returns_error() {
        let pool = PassthroughTerminalPool;
        let id = ManagedTerminalId {
            checkout: "test".into(),
            role: "shell".into(),
            index: 0,
        };
        assert!(pool.attach_command(&id).is_err());
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p flotilla-core passthrough`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: add PassthroughTerminalPool implementation"
```

---

### Task 4: ShpoolTerminalPool — list and parse

Build the shpool integration, starting with `list_terminals` which parses `shpool list --json`.

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/shpool.rs`

**Step 1: Write test for parsing shpool list JSON**

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use flotilla_protocol::{ManagedTerminal, ManagedTerminalId, TerminalStatus};

use super::TerminalPool;
use crate::providers::CommandRunner;

pub struct ShpoolTerminalPool {
    runner: Arc<dyn CommandRunner>,
    socket_path: PathBuf,
}

impl ShpoolTerminalPool {
    pub fn new(runner: Arc<dyn CommandRunner>, socket_path: PathBuf) -> Self {
        Self {
            runner,
            socket_path,
        }
    }

    fn socket_args(&self) -> Vec<String> {
        vec![
            "--socket".into(),
            self.socket_path.display().to_string(),
        ]
    }

    /// Parse the JSON output of `shpool list --json`.
    fn parse_list_json(json: &str) -> Result<Vec<ManagedTerminal>, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("failed to parse shpool list: {e}"))?;

        let sessions = parsed["sessions"]
            .as_array()
            .ok_or("shpool list: no sessions array")?;

        let mut terminals = Vec::new();
        for session in sessions {
            let name = session["name"]
                .as_str()
                .ok_or("shpool session missing name")?;

            // Only show flotilla-managed sessions (prefixed "flotilla/")
            let Some(rest) = name.strip_prefix("flotilla/") else {
                continue;
            };

            // Parse "checkout/role/index"
            let parts: Vec<&str> = rest.splitn(3, '/').collect();
            if parts.len() != 3 {
                continue;
            }
            let index: u32 = parts[2].parse().unwrap_or(0);

            let status = match session["status"].as_str() {
                Some("attached") => TerminalStatus::Running,
                Some("disconnected") => TerminalStatus::Disconnected,
                _ => TerminalStatus::Disconnected,
            };

            terminals.push(ManagedTerminal {
                id: ManagedTerminalId {
                    checkout: parts[0].into(),
                    role: parts[1].into(),
                    index,
                },
                role: parts[1].into(),
                command: String::new(), // shpool doesn't report the original command
                working_directory: PathBuf::new(), // populated separately if needed
                status,
            });
        }

        Ok(terminals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_json_with_flotilla_named_sessions() {
        let json = r#"{
            "sessions": [
                {
                    "name": "flotilla/my-feature/shell/0",
                    "started_at_unix_ms": 1709900000000,
                    "status": "attached"
                },
                {
                    "name": "flotilla/my-feature/agent/0",
                    "started_at_unix_ms": 1709900001000,
                    "status": "disconnected"
                },
                {
                    "name": "user-manual-session",
                    "started_at_unix_ms": 1709900002000,
                    "status": "attached"
                }
            ]
        }"#;

        let terminals = ShpoolTerminalPool::parse_list_json(json).unwrap();
        assert_eq!(terminals.len(), 2); // user-manual-session filtered out

        assert_eq!(terminals[0].id.checkout, "my-feature");
        assert_eq!(terminals[0].id.role, "shell");
        assert_eq!(terminals[0].id.index, 0);
        assert_eq!(terminals[0].status, TerminalStatus::Running);

        assert_eq!(terminals[1].id.checkout, "my-feature");
        assert_eq!(terminals[1].id.role, "agent");
        assert_eq!(terminals[1].status, TerminalStatus::Disconnected);
    }

    #[test]
    fn parse_list_json_empty_sessions() {
        let json = r#"{"sessions": []}"#;
        let terminals = ShpoolTerminalPool::parse_list_json(json).unwrap();
        assert!(terminals.is_empty());
    }

    #[test]
    fn parse_list_json_invalid_json() {
        assert!(ShpoolTerminalPool::parse_list_json("not json").is_err());
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p flotilla-core shpool`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: add ShpoolTerminalPool with list parsing"
```

---

### Task 5: ShpoolTerminalPool — ensure_running and attach_command

Wire up the CLI commands for session creation and attachment.

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/shpool.rs`

**Step 1: Implement the TerminalPool trait**

Add the trait implementation to `shpool.rs`:

```rust
#[async_trait]
impl TerminalPool for ShpoolTerminalPool {
    fn display_name(&self) -> &str {
        "shpool"
    }

    async fn list_terminals(&self) -> Result<Vec<ManagedTerminal>, String> {
        let mut args: Vec<&str> = self.socket_args().iter().map(|s| s.as_str()).collect();
        // Can't borrow socket_args across await — build full args inline
        let socket_path_str = self.socket_path.display().to_string();
        let result = self
            .runner
            .run(
                "shpool",
                &["--socket", &socket_path_str, "list", "--json"],
                Path::new("/"),
            )
            .await;

        match result {
            Ok(json) => Self::parse_list_json(&json),
            Err(e) => {
                // shpool not running is not an error — just no terminals
                tracing::debug!("shpool list failed (daemon may not be running): {e}");
                Ok(vec![])
            }
        }
    }

    async fn ensure_running(
        &self,
        id: &ManagedTerminalId,
        command: &str,
        cwd: &Path,
    ) -> Result<(), String> {
        let session_name = format!("flotilla/{id}");
        let socket_path_str = self.socket_path.display().to_string();
        let cwd_str = cwd.display().to_string();

        // Try to attach in background mode — creates session if new, reuses if exists
        let result = self
            .runner
            .run(
                "shpool",
                &[
                    "--socket", &socket_path_str,
                    "attach",
                    "--background",
                    "--cmd", command,
                    "--dir", &cwd_str,
                    &session_name,
                ],
                Path::new("/"),
            )
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if e.contains("already attached") || e.contains("busy") => {
                // Session already exists and is attached — that's fine
                Ok(())
            }
            Err(e) => Err(format!("shpool ensure_running failed for {session_name}: {e}")),
        }
    }

    async fn attach_command(&self, id: &ManagedTerminalId) -> Result<String, String> {
        let session_name = format!("flotilla/{id}");
        let socket_path_str = self.socket_path.display().to_string();
        Ok(format!(
            "shpool --socket {} attach {}",
            shell_escape::escape(socket_path_str.into()),
            shell_escape::escape(session_name.into()),
        ))
    }

    async fn kill_terminal(&self, id: &ManagedTerminalId) -> Result<(), String> {
        let session_name = format!("flotilla/{id}");
        let socket_path_str = self.socket_path.display().to_string();
        self.runner
            .run(
                "shpool",
                &["--socket", &socket_path_str, "kill", &session_name],
                Path::new("/"),
            )
            .await
            .map(|_| ())
    }
}
```

**Step 2: Write tests using MockRunner**

Add to the `mod tests` in `shpool.rs`:

```rust
use crate::providers::testing::MockRunner;

#[tokio::test]
async fn ensure_running_calls_shpool_attach_background() {
    let runner = Arc::new(MockRunner::new(vec![
        Ok("".into()), // shpool attach --background succeeds
    ]));
    let pool = ShpoolTerminalPool::new(runner, PathBuf::from("/tmp/test.sock"));
    let id = ManagedTerminalId {
        checkout: "feat".into(),
        role: "shell".into(),
        index: 0,
    };
    assert!(pool
        .ensure_running(&id, "bash", Path::new("/home/dev"))
        .await
        .is_ok());
}

#[tokio::test]
async fn attach_command_returns_shpool_attach() {
    let runner = Arc::new(MockRunner::new(vec![]));
    let pool = ShpoolTerminalPool::new(runner, PathBuf::from("/tmp/test.sock"));
    let id = ManagedTerminalId {
        checkout: "feat".into(),
        role: "shell".into(),
        index: 0,
    };
    let cmd = pool.attach_command(&id).await.unwrap();
    assert!(cmd.contains("shpool"));
    assert!(cmd.contains("attach"));
    assert!(cmd.contains("flotilla/feat/shell/0"));
}

#[tokio::test]
async fn list_terminals_returns_empty_when_daemon_not_running() {
    let runner = Arc::new(MockRunner::new(vec![
        Err("connection refused".into()),
    ]));
    let pool = ShpoolTerminalPool::new(runner, PathBuf::from("/tmp/test.sock"));
    let terminals = pool.list_terminals().await.unwrap();
    assert!(terminals.is_empty());
}
```

**Step 3: Add `shell-escape` dependency**

Run: `cargo add shell-escape -p flotilla-core`

If `shell-escape` is not appropriate, use a simple quoting function instead (check what the codebase already does — `CmuxWorkspaceManager::shell_quote` at line 16 in `cmux.rs`).

**Step 4: Run tests**

Run: `cargo test -p flotilla-core shpool && cargo clippy -p flotilla-core --all-targets -- -D warnings`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: implement ShpoolTerminalPool ensure_running and attach_command"
```

---

### Task 6: Add TerminalPool to ProviderRegistry

**Files:**
- Modify: `crates/flotilla-core/src/providers/registry.rs:1-38`

**Step 1: Add the field**

In `crates/flotilla-core/src/providers/registry.rs`:

Add import: `use crate::providers::terminal::TerminalPool;`

Add field to `ProviderRegistry` (after `workspace_manager` at line 17):
```rust
pub terminal_pool: Option<(String, Arc<dyn TerminalPool>)>,
```

Add to `new()` (after `workspace_manager: None` at line 29):
```rust
terminal_pool: None,
```

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: add terminal_pool to ProviderRegistry"
```

---

### Task 7: Provider discovery for shpool

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery.rs:197-235`

**Step 1: Add shpool detection**

After the workspace manager detection block (around line 235) and before the `(registry, repo_slug)` return, add:

```rust
// 7. Terminal pool: prefer shpool if available
if runner.exists("shpool", &["version"]).await {
    let shpool_socket = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("flotilla/shpool/shpool.socket");
    registry.terminal_pool = Some((
        "shpool".into(),
        Arc::new(crate::providers::terminal::shpool::ShpoolTerminalPool::new(
            Arc::clone(&runner),
            shpool_socket,
        )),
    ));
    info!("{repo_name}: Terminal pool → shpool");
}
```

Add necessary imports at the top of `discovery.rs`.

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: detect shpool in provider discovery"
```

---

### Task 8: Add managed terminals to refresh cycle

**Files:**
- Modify: `crates/flotilla-core/src/refresh.rs:107-179`

**Step 1: Add terminal pool fetch to refresh_providers**

In `refresh_providers()`, add a future alongside the existing ones (after `ws_fut` around line 161):

```rust
let tp_fut = async {
    if let Some((_, tp)) = &registry.terminal_pool {
        tp.list_terminals().await
    } else {
        Ok(vec![])
    }
};
```

Add `tp_fut` to the `tokio::join!` call (line 163), capturing the result as `managed_terminals`.

After the workspaces processing, add:

```rust
let terminal_list = managed_terminals.unwrap_or_else(|e| {
    errors.push(RefreshError {
        category: "terminals",
        message: e,
    });
    Vec::new()
});
for terminal in terminal_list {
    let key = terminal.id.to_string();
    pd.managed_terminals.insert(key, terminal);
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: PASS

**Step 3: Write a test**

Add a test in the existing `mod tests` section of `refresh.rs` that uses a mock terminal pool to verify terminals appear in `ProviderData`. Follow the pattern of existing tests like `refresh_populates_all_provider_data_and_merged_wins_branch_conflict` (line 669).

**Step 4: Run tests**

Run: `cargo test -p flotilla-core refresh && cargo clippy -p flotilla-core --all-targets -- -D warnings`
Expected: PASS

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: include managed terminals in refresh cycle"
```

---

### Task 9: Add managed terminals to correlation

**Files:**
- Modify: `crates/flotilla-core/src/providers/correlation.rs`
- Modify: `crates/flotilla-core/src/data.rs`

**Step 1: Study existing correlation code**

Read `crates/flotilla-core/src/providers/correlation.rs` and `crates/flotilla-core/src/data.rs` to understand how items are added to the union-find and how work items are built. The pattern is:

1. Each provider item type has a `ProviderItemKey` variant
2. Items are added to the correlation engine with their `CorrelationKey`s
3. The engine merges items sharing keys into groups
4. Groups are converted into `WorkItem`s

**Step 2: Add `ManagedTerminal` to correlation**

Add a `ProviderItemKey::ManagedTerminal(String)` variant (keyed by `ManagedTerminalId.to_string()`).

When building correlated items from `ProviderData`, add managed terminals. Each terminal contributes `CorrelationKey::CheckoutPath(terminal.working_directory)` — linking it to the same work item as the checkout it runs inside.

**Step 3: Add terminal refs to WorkItem output**

When building the protocol `WorkItem` from a correlated group, collect terminal keys into a new field or extend the existing `workspace_refs` field. Check if a new field like `terminal_keys: Vec<String>` on `flotilla_protocol::WorkItem` is appropriate, or if reusing `workspace_refs` is cleaner for now.

**Step 4: Write a test**

Test that a `ManagedTerminal` with `working_directory = /foo` correlates with a `Checkout` at path `/foo`.

**Step 5: Run tests**

Run: `cargo test -p flotilla-core correlation && cargo test -p flotilla-core data`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "feat: correlate managed terminals with checkouts via cwd"
```

---

### Task 10: Shpool daemon lifecycle management

The daemon needs to start/manage a shpool subprocess.

**Files:**
- Create: `crates/flotilla-core/src/shpool_daemon.rs`
- Modify: `crates/flotilla-core/src/lib.rs` (add `pub mod shpool_daemon;`)

**Step 1: Write the daemon manager**

```rust
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{error, info};

/// Manages a shpool daemon subprocess for persistent terminal sessions.
pub struct ShpoolDaemonHandle {
    child: Option<Child>,
    socket_path: PathBuf,
}

impl ShpoolDaemonHandle {
    /// Start a shpool daemon with the given socket path.
    /// If a daemon is already listening on this socket, connects to it instead.
    pub async fn start(socket_path: &Path) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create shpool socket dir: {e}"))?;
        }

        // Check if already running
        let check = Command::new("shpool")
            .args(["--socket", &socket_path.display().to_string(), "list"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        if let Ok(status) = check {
            if status.success() {
                info!("shpool daemon already running at {}", socket_path.display());
                return Ok(Self {
                    child: None,
                    socket_path: socket_path.to_path_buf(),
                });
            }
        }

        info!("starting shpool daemon at {}", socket_path.display());
        let child = Command::new("shpool")
            .args([
                "--socket",
                &socket_path.display().to_string(),
                "daemon",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start shpool daemon: {e}"))?;

        // Give it a moment to bind the socket
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(Self {
            child: Some(child),
            socket_path: socket_path.to_path_buf(),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for ShpoolDaemonHandle {
    fn drop(&mut self) {
        // Don't kill shpool on drop — sessions should persist
        if let Some(ref mut child) = self.child {
            // Detach: let the process continue running
            info!("flotilla shutting down, leaving shpool daemon running");
            std::mem::forget(child.id());
        }
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: add ShpoolDaemonHandle for subprocess lifecycle"
```

---

### Task 11: Evolve template format — content and layout split

**Files:**
- Modify: `crates/flotilla-core/src/template.rs`

**Step 1: Write tests for new format parsing**

Add tests that parse the new `content:` + `layout:` YAML format:

```rust
#[test]
fn new_format_content_and_layout() {
    let yaml = r#"
content:
  - role: shell
    command: "$SHELL"
  - role: agent
    command: "claude-code"
    count: 2
  - role: build
    command: "cargo watch -x check"

layout:
  - slot: shell
  - slot: agent
    split: right
    overflow: tab
  - slot: build
    split: down
    parent: shell
    gap: placeholder
"#;
    let template: WorkspaceTemplateV2 = serde_yml::from_str(yaml).unwrap();
    assert_eq!(template.content.len(), 3);
    assert_eq!(template.content[0].role, "shell");
    assert_eq!(template.content[1].count, Some(2));
    assert_eq!(template.layout.len(), 3);
    assert_eq!(template.layout[0].slot, "shell");
    assert_eq!(template.layout[1].overflow.as_deref(), Some("tab"));
    assert_eq!(template.layout[2].gap.as_deref(), Some("placeholder"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core new_format_content_and_layout`
Expected: FAIL — `WorkspaceTemplateV2` not defined.

**Step 3: Add new types**

Add to `crates/flotilla-core/src/template.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceTemplateV2 {
    pub content: Vec<ContentEntry>,
    pub layout: Vec<LayoutSlot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContentEntry {
    pub role: String,
    #[serde(default = "default_content_type")]
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub count: Option<u32>,
}

fn default_content_type() -> String {
    "terminal".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutSlot {
    pub slot: String,
    #[serde(default)]
    pub split: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub overflow: Option<String>,
    #[serde(default)]
    pub gap: Option<String>,
    #[serde(default)]
    pub focus: bool,
}
```

Add a `render` method to `WorkspaceTemplateV2` that substitutes vars in content commands (same pattern as existing `WorkspaceTemplate::render`).

Add a method to detect which format a YAML string uses:

```rust
/// Try to parse as V2 (content/layout) first, fall back to V1 (panes).
pub enum ParsedTemplate {
    V1(WorkspaceTemplate),
    V2(WorkspaceTemplateV2),
}

pub fn parse_template(yaml: &str) -> Result<ParsedTemplate, String> {
    if let Ok(v2) = serde_yml::from_str::<WorkspaceTemplateV2>(yaml) {
        if !v2.content.is_empty() {
            return Ok(ParsedTemplate::V2(v2));
        }
    }
    serde_yml::from_str::<WorkspaceTemplate>(yaml)
        .map(ParsedTemplate::V1)
        .map_err(|e| e.to_string())
}
```

**Step 4: Run tests**

Run: `cargo test -p flotilla-core template`
Expected: PASS (all existing tests still pass, new test passes)

**Step 5: Commit**

```bash
git add -A && git commit -m "feat: add V2 template format with content/layout split"
```

---

### Task 12: Wire workspace creation through terminal pool

This is the key integration: when creating a workspace and a terminal pool is available, use it.

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs:29-50`
- Modify: `crates/flotilla-core/src/providers/workspace/cmux.rs`

**Step 1: Update executor to pass terminal pool to workspace creation**

The executor currently calls `ws_mgr.create_workspace(&config)`. When a terminal pool is available, the flow becomes:

1. Parse template (V1 or V2)
2. For V2: for each content entry with type=terminal, call `pool.ensure_running()`
3. Build commands for each pane: `pool.attach_command()` instead of the raw command
4. Pass modified config to workspace manager

This can be done by extending `WorkspaceConfig` to carry pre-resolved commands, or by giving the workspace manager access to the terminal pool. The cleaner approach is to resolve commands in the executor and pass them through.

Add to `WorkspaceConfig` in `crates/flotilla-core/src/providers/types.rs`:

```rust
/// When set, these override the template commands — each entry is (role, attach_command).
/// Used when a TerminalPool has pre-started sessions.
#[serde(skip)]
pub resolved_commands: Option<Vec<(String, String)>>,
```

**Step 2: Update executor**

In the `CreateWorkspaceForCheckout` arm of `execute()` (line 30-50), after building the config:

```rust
// If terminal pool is available, ensure sessions are running and resolve attach commands
if let Some((_, tp)) = &registry.terminal_pool {
    let template_yaml = config.template_yaml.as_deref();
    if let Some(yaml) = template_yaml {
        if let Ok(ParsedTemplate::V2(v2)) = parse_template(yaml) {
            let rendered = v2.render(&config.template_vars);
            let mut resolved = Vec::new();
            for entry in &rendered.content {
                if entry.content_type != "terminal" {
                    continue;
                }
                let count = entry.count.unwrap_or(1);
                for i in 0..count {
                    let id = ManagedTerminalId {
                        checkout: config.name.clone(),
                        role: entry.role.clone(),
                        index: i,
                    };
                    if let Err(e) = tp.ensure_running(&id, &entry.command, &config.working_directory).await {
                        tracing::warn!("failed to ensure terminal {id}: {e}");
                        continue;
                    }
                    match tp.attach_command(&id).await {
                        Ok(cmd) => resolved.push((entry.role.clone(), cmd)),
                        Err(e) => tracing::warn!("failed to get attach command for {id}: {e}"),
                    }
                }
            }
            config.resolved_commands = Some(resolved);
        }
    }
}
```

**Step 3: Update workspace manager to use resolved commands**

In `cmux.rs` (and later tmux/zellij), when building the command for each surface, check if `resolved_commands` provides an override for this role. This requires mapping layout slots to resolved commands by role.

For V1 templates (current format), the workspace manager continues to work as-is. For V2, the executor has resolved commands and passes them through.

**Step 4: Write integration test**

Test the full flow with a mock terminal pool and mock workspace manager, verifying that:
- `ensure_running` is called for each content entry
- `attach_command` results are passed to the workspace manager

**Step 5: Run tests**

Run: `cargo test -p flotilla-core && cargo clippy --all-targets -- -D warnings`
Expected: PASS

**Step 6: Commit**

```bash
git add -A && git commit -m "feat: wire workspace creation through terminal pool"
```

---

### Task 13: Start shpool daemon in InProcessDaemon

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs`

**Step 1: Add shpool startup to daemon initialization**

In `InProcessDaemon::new()` (around line 239), after provider detection, if the registry has a shpool terminal pool, start the shpool daemon:

```rust
// Start shpool daemon if terminal pool uses shpool
let shpool_handle = if registry.terminal_pool.as_ref().map(|(name, _)| name.as_str()) == Some("shpool") {
    let socket_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("flotilla/shpool/shpool.socket");
    match ShpoolDaemonHandle::start(&socket_path).await {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::error!("failed to start shpool daemon: {e}");
            None
        }
    }
} else {
    None
};
```

Store the handle in `InProcessDaemon` so it lives as long as the daemon.

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: PASS

**Step 3: Commit**

```bash
git add -A && git commit -m "feat: start shpool daemon in InProcessDaemon"
```

---

### Task 14: End-to-end manual testing

**No code changes — just verification.**

**Step 1: Install shpool**

Run: `cargo install shpool` (or build from `~/dev/shpool`)

**Step 2: Create a V2 workspace template**

Create `.flotilla/workspace.yaml` in a test repo:

```yaml
content:
  - role: shell
    command: "$SHELL"
  - role: build
    command: "cargo watch -x check"

layout:
  - slot: shell
  - slot: build
    split: right
```

**Step 3: Test the persistence flow**

1. Start flotilla, create a workspace for a checkout
2. Verify shpool sessions are created: `shpool --socket ~/.config/flotilla/shpool/shpool.socket list --json`
3. Kill the workspace manager session (e.g., close cmux workspace)
4. Verify shpool sessions are still alive: `shpool list --json`
5. Recreate the workspace in flotilla
6. Verify it reattaches to existing sessions (scrollback preserved)

**Step 4: Test passthrough fallback**

1. Rename/remove `shpool` binary temporarily
2. Start flotilla — should fall back to PassthroughTerminalPool
3. Workspace creation still works (old behavior)

---

### Task 15: Final cleanup and full test suite

**Step 1: Run full test suite**

Run: `cargo test --locked`
Expected: PASS

**Step 2: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS

**Step 3: Run fmt**

Run: `cargo fmt`

**Step 4: Commit any fixups**

```bash
git add -A && git commit -m "chore: clippy and fmt cleanup"
```
