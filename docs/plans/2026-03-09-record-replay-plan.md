# Record/Replay Test Harness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a record/replay test harness for provider tests, then use it to add coverage for git.rs, wt.rs, tmux.rs, zellij.rs, and GitHub providers (#120, #127, #89, #90, #91).

**Architecture:** A `ReplaySession` holds a YAML-backed interaction log shared by multiple trait adapters (CommandRunner, GhApi). In replay mode, adapters serve canned responses. In record mode, they passthrough to real implementations and capture the interaction. Tests are normal Rust — the harness just handles I/O canning.

**Tech Stack:** Rust, serde, serde_yml (already a dependency), tokio, async-trait

---

### Task 1: Interaction types and YAML serialization

**Files:**
- Create: `crates/flotilla-core/src/providers/replay.rs`
- Modify: `crates/flotilla-core/src/providers/mod.rs` (add `pub mod replay;`)

**Step 1: Create the interaction types and basic session struct**

Create `replay.rs` with:

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A single recorded interaction with an external system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "channel")]
pub enum Interaction {
    #[serde(rename = "command")]
    Command {
        cmd: String,
        args: Vec<String>,
        cwd: String,
        #[serde(default)]
        stdout: Option<String>,
        #[serde(default)]
        stderr: Option<String>,
        #[serde(default)]
        exit_code: i32,
    },
    #[serde(rename = "gh_api")]
    GhApi {
        method: String,
        endpoint: String,
        status: u16,
        body: String,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        headers: HashMap<String, String>,
    },
}

/// Top-level YAML document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionLog {
    pub interactions: Vec<Interaction>,
}

/// Placeholder substitutions for non-deterministic values.
#[derive(Debug, Clone, Default)]
pub struct Masks {
    substitutions: Vec<(String, String)>,
}

impl Masks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a substitution: concrete value → placeholder.
    pub fn add(&mut self, concrete: impl Into<String>, placeholder: impl Into<String>) {
        self.substitutions.push((concrete.into(), placeholder.into()));
    }

    /// Apply masks: replace concrete values with placeholders (for recording).
    pub fn mask(&self, s: &str) -> String {
        let mut result = s.to_string();
        for (concrete, placeholder) in &self.substitutions {
            result = result.replace(concrete, placeholder);
        }
        result
    }

    /// Apply masks in reverse: replace placeholders with concrete values (for replay).
    pub fn unmask(&self, s: &str) -> String {
        let mut result = s.to_string();
        for (concrete, placeholder) in &self.substitutions {
            result = result.replace(placeholder, concrete);
        }
        result
    }
}

/// Shared session state, holding the interaction log and current read position.
struct SessionInner {
    log: InteractionLog,
    cursor: usize,
    masks: Masks,
    /// In record mode, newly captured interactions accumulate here.
    recorded: Vec<Interaction>,
    recording: bool,
    file_path: Option<PathBuf>,
}

/// A replay session backed by a YAML file. Multiple adapters share one session
/// via `Arc`. Each adapter reads entries matching its channel.
#[derive(Clone)]
pub struct ReplaySession {
    inner: Arc<Mutex<SessionInner>>,
}

impl ReplaySession {
    /// Load a session from a YAML fixture file.
    pub fn from_file(path: impl AsRef<Path>, masks: Masks) -> Self {
        let content = std::fs::read_to_string(path.as_ref())
            .unwrap_or_else(|e| panic!("Failed to read fixture {}: {e}", path.as_ref().display()));
        let log: InteractionLog = serde_yml::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse fixture {}: {e}", path.as_ref().display()));
        Self {
            inner: Arc::new(Mutex::new(SessionInner {
                log,
                cursor: 0,
                masks,
                recorded: Vec::new(),
                recording: false,
                file_path: Some(path.as_ref().to_path_buf()),
            })),
        }
    }

    /// Create an empty session for recording.
    pub fn recording(path: impl AsRef<Path>, masks: Masks) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SessionInner {
                log: InteractionLog { interactions: Vec::new() },
                cursor: 0,
                masks,
                recorded: Vec::new(),
                recording: true,
                file_path: Some(path.as_ref().to_path_buf()),
            })),
        }
    }

    /// Consume the next interaction, asserting it matches the expected channel.
    /// Returns the interaction with masks unmasked (placeholders → concrete values).
    pub(crate) fn next(&self, expected_channel: &str) -> Interaction {
        let mut inner = self.inner.lock().unwrap();
        assert!(
            !inner.recording,
            "next() called in recording mode — use record() instead"
        );
        let idx = inner.cursor;
        let interaction = inner.log.interactions.get(idx)
            .unwrap_or_else(|| panic!(
                "ReplaySession: no more interactions (cursor={idx}, total={})",
                inner.log.interactions.len()
            ))
            .clone();

        // Verify channel matches
        let actual_channel = match &interaction {
            Interaction::Command { .. } => "command",
            Interaction::GhApi { .. } => "gh_api",
        };
        assert_eq!(
            actual_channel, expected_channel,
            "ReplaySession: expected channel '{expected_channel}' at position {idx}, got '{actual_channel}'"
        );

        inner.cursor += 1;
        unmask_interaction(&interaction, &inner.masks)
    }

    /// Record a new interaction (in recording mode).
    pub(crate) fn record(&self, interaction: Interaction) {
        let mut inner = self.inner.lock().unwrap();
        assert!(inner.recording, "record() called in replay mode");
        let masked = mask_interaction(&interaction, &inner.masks);
        inner.recorded.push(masked);
    }

    /// Write recorded interactions to the YAML file.
    pub fn save(&self) {
        let inner = self.inner.lock().unwrap();
        if let Some(ref path) = inner.file_path {
            let log = InteractionLog {
                interactions: inner.recorded.clone(),
            };
            let yaml = serde_yml::to_string(&log).expect("Failed to serialize interactions");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(path, yaml).unwrap_or_else(|e| {
                panic!("Failed to write fixture {}: {e}", path.display())
            });
        }
    }

    /// Check that all interactions were consumed.
    pub fn assert_complete(&self) {
        let inner = self.inner.lock().unwrap();
        if !inner.recording {
            let remaining = inner.log.interactions.len() - inner.cursor;
            assert_eq!(
                remaining, 0,
                "ReplaySession: {remaining} unconsumed interactions remaining"
            );
        }
    }
}

fn unmask_interaction(interaction: &Interaction, masks: &Masks) -> Interaction {
    match interaction {
        Interaction::Command { cmd, args, cwd, stdout, stderr, exit_code } => {
            Interaction::Command {
                cmd: masks.unmask(cmd),
                args: args.iter().map(|a| masks.unmask(a)).collect(),
                cwd: masks.unmask(cwd),
                stdout: stdout.as_ref().map(|s| masks.unmask(s)),
                stderr: stderr.as_ref().map(|s| masks.unmask(s)),
                exit_code: *exit_code,
            }
        }
        Interaction::GhApi { method, endpoint, status, body, headers } => {
            Interaction::GhApi {
                method: method.clone(),
                endpoint: masks.unmask(endpoint),
                status: *status,
                body: masks.unmask(body),
                headers: headers.iter().map(|(k, v)| (k.clone(), masks.unmask(v))).collect(),
            }
        }
    }
}

fn mask_interaction(interaction: &Interaction, masks: &Masks) -> Interaction {
    match interaction {
        Interaction::Command { cmd, args, cwd, stdout, stderr, exit_code } => {
            Interaction::Command {
                cmd: masks.mask(cmd),
                args: args.iter().map(|a| masks.mask(a)).collect(),
                cwd: masks.mask(cwd),
                stdout: stdout.as_ref().map(|s| masks.mask(s)),
                stderr: stderr.as_ref().map(|s| masks.mask(s)),
                exit_code: *exit_code,
            }
        }
        Interaction::GhApi { method, endpoint, status, body, headers } => {
            Interaction::GhApi {
                method: method.clone(),
                endpoint: masks.mask(endpoint),
                status: *status,
                body: masks.mask(body),
                headers: headers.iter().map(|(k, v)| (k.clone(), masks.mask(v))).collect(),
            }
        }
    }
}
```

**Step 2: Add module to mod.rs**

Add `pub mod replay;` to `crates/flotilla-core/src/providers/mod.rs`.

**Step 3: Write unit tests for Masks and YAML round-trip**

In `replay.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_substitute_and_reverse() {
        let mut masks = Masks::new();
        masks.add("/Users/bob/dev/repo", "{repo}");
        masks.add("/Users/bob", "{home}");

        assert_eq!(masks.mask("/Users/bob/dev/repo/src"), "{repo}/src");
        assert_eq!(masks.unmask("{repo}/src"), "/Users/bob/dev/repo/src");
        // Ordering matters: longer match first
        assert_eq!(masks.mask("/Users/bob/.config"), "{home}/.config");
    }

    #[test]
    fn yaml_round_trip() {
        let log = InteractionLog {
            interactions: vec![
                Interaction::Command {
                    cmd: "git".into(),
                    args: vec!["status".into()],
                    cwd: "{repo}".into(),
                    stdout: Some("clean\n".into()),
                    stderr: None,
                    exit_code: 0,
                },
                Interaction::GhApi {
                    method: "GET".into(),
                    endpoint: "/repos/owner/repo/pulls".into(),
                    status: 200,
                    body: "[]".into(),
                    headers: HashMap::new(),
                },
            ],
        };

        let yaml = serde_yml::to_string(&log).unwrap();
        let parsed: InteractionLog = serde_yml::from_str(&yaml).unwrap();
        assert_eq!(parsed.interactions.len(), 2);
    }

    #[test]
    fn replay_session_serves_in_order() {
        let log = InteractionLog {
            interactions: vec![
                Interaction::Command {
                    cmd: "git".into(),
                    args: vec!["status".into()],
                    cwd: "{repo}".into(),
                    stdout: Some("ok\n".into()),
                    stderr: None,
                    exit_code: 0,
                },
            ],
        };

        let yaml = serde_yml::to_string(&log).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.yaml");
        std::fs::write(&path, &yaml).unwrap();

        let mut masks = Masks::new();
        masks.add("/real/repo", "{repo}");
        let session = ReplaySession::from_file(&path, masks);

        let interaction = session.next("command");
        match interaction {
            Interaction::Command { cmd, cwd, .. } => {
                assert_eq!(cmd, "git");
                assert_eq!(cwd, "/real/repo");
            }
            _ => panic!("expected command"),
        }
        session.assert_complete();
    }
}
```

**Step 4: Verify**

Run: `cargo test -p flotilla-core replay 2>&1 | tail -10`
Expected: all tests pass

**Step 5: Commit**

```
feat: add record/replay interaction types and session (#120)
```

---

### Task 2: ReplayRunner (CommandRunner adapter)

**Files:**
- Modify: `crates/flotilla-core/src/providers/replay.rs`

**Step 1: Add ReplayRunner struct**

Add to `replay.rs`:

```rust
use async_trait::async_trait;
use super::{CommandRunner, CommandOutput};

/// A CommandRunner that replays canned responses from a ReplaySession.
pub struct ReplayRunner {
    session: ReplaySession,
}

impl ReplayRunner {
    pub fn new(session: ReplaySession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl CommandRunner for ReplayRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path) -> Result<String, String> {
        let interaction = self.session.next("command");
        let Interaction::Command {
            cmd: expected_cmd,
            args: expected_args,
            cwd: expected_cwd,
            stdout,
            stderr,
            exit_code,
        } = interaction else {
            panic!("ReplayRunner: expected command interaction");
        };

        assert_eq!(cmd, expected_cmd, "ReplayRunner: command mismatch");
        let actual_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        assert_eq!(actual_args, expected_args, "ReplayRunner: args mismatch for '{cmd}'");

        if exit_code == 0 {
            Ok(stdout.unwrap_or_default())
        } else {
            Err(stderr.unwrap_or_default())
        }
    }

    async fn run_output(
        &self,
        cmd: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<CommandOutput, String> {
        let interaction = self.session.next("command");
        let Interaction::Command {
            cmd: expected_cmd,
            args: expected_args,
            stdout,
            stderr,
            exit_code,
            ..
        } = interaction else {
            panic!("ReplayRunner: expected command interaction");
        };

        assert_eq!(cmd, expected_cmd, "ReplayRunner: command mismatch");
        let actual_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        assert_eq!(actual_args, expected_args, "ReplayRunner: args mismatch for '{cmd}'");

        Ok(CommandOutput {
            stdout: stdout.unwrap_or_default(),
            stderr: stderr.unwrap_or_default(),
            success: exit_code == 0,
        })
    }

    async fn exists(&self, _cmd: &str, _args: &[&str]) -> bool {
        true
    }
}
```

Also add a convenience method on `ReplaySession`:

```rust
impl ReplaySession {
    pub fn command_runner(&self) -> ReplayRunner {
        ReplayRunner::new(self.clone())
    }
}
```

**Step 2: Write a test using ReplayRunner with a real provider**

```rust
#[tokio::test]
async fn replay_runner_with_git_vcs() {
    // Create a fixture YAML inline for the test
    let yaml = r#"
interactions:
  - channel: command
    cmd: git
    args: ["branch", "--list", "--format=%(refname:short)|||%(upstream:short)|||%(upstream:track)"]
    cwd: "{repo}"
    stdout: "main|||origin/main|||\nfeature|||origin/feature|||[ahead 2, behind 1]\n"
    exit_code: 0
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.yaml");
    std::fs::write(&path, yaml).unwrap();

    let mut masks = Masks::new();
    masks.add("/test/repo", "{repo}");
    let session = ReplaySession::from_file(&path, masks);
    let runner = Arc::new(session.command_runner());

    use crate::providers::vcs::git::GitVcs;
    let git = GitVcs::new(runner, Path::new("/test/repo"));
    let branches = git.branches().await.unwrap();

    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0].name, "main");
}
```

**Step 3: Verify**

Run: `cargo test -p flotilla-core replay 2>&1 | tail -10`

**Step 4: Commit**

```
feat: add ReplayRunner CommandRunner adapter (#120)
```

---

### Task 3: ReplayGhApi adapter

**Files:**
- Modify: `crates/flotilla-core/src/providers/replay.rs`

**Step 1: Add ReplayGhApi struct**

```rust
use super::github_api::{GhApi, GhApiResponse};

/// A GhApi implementation that replays canned responses from a ReplaySession.
pub struct ReplayGhApi {
    session: ReplaySession,
}

impl ReplayGhApi {
    pub fn new(session: ReplaySession) -> Self {
        Self { session }
    }
}

#[async_trait]
impl GhApi for ReplayGhApi {
    async fn get(&self, endpoint: &str, _repo_root: &Path) -> Result<String, String> {
        let interaction = self.session.next("gh_api");
        let Interaction::GhApi {
            endpoint: expected_endpoint,
            status,
            body,
            ..
        } = interaction else {
            panic!("ReplayGhApi: expected gh_api interaction");
        };

        assert_eq!(endpoint, expected_endpoint, "ReplayGhApi: endpoint mismatch");

        if status >= 200 && status < 300 {
            Ok(body)
        } else {
            Err(format!("HTTP {status}: {body}"))
        }
    }

    async fn get_with_headers(
        &self,
        endpoint: &str,
        _repo_root: &Path,
    ) -> Result<GhApiResponse, String> {
        let interaction = self.session.next("gh_api");
        let Interaction::GhApi {
            endpoint: expected_endpoint,
            status,
            body,
            headers,
            ..
        } = interaction else {
            panic!("ReplayGhApi: expected gh_api interaction");
        };

        assert_eq!(endpoint, expected_endpoint, "ReplayGhApi: endpoint mismatch");

        if status >= 200 && status < 300 {
            Ok(GhApiResponse {
                body,
                has_next_page: headers.get("has_next_page").map(|v| v == "true").unwrap_or(false),
            })
        } else {
            Err(format!("HTTP {status}: {body}"))
        }
    }
}
```

Add convenience method:
```rust
impl ReplaySession {
    pub fn gh_api(&self) -> ReplayGhApi {
        ReplayGhApi::new(self.clone())
    }
}
```

**Step 2: Write test**

```rust
#[tokio::test]
async fn replay_gh_api() {
    let yaml = r#"
interactions:
  - channel: gh_api
    method: GET
    endpoint: "/repos/owner/repo/pulls?state=all&per_page=100"
    status: 200
    body: '[{"number": 42, "title": "Fix bug"}]'
"#;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.yaml");
    std::fs::write(&path, yaml).unwrap();

    let session = ReplaySession::from_file(&path, Masks::new());
    let api = session.gh_api();

    let result = api.get("/repos/owner/repo/pulls?state=all&per_page=100", Path::new("/repo")).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("Fix bug"));
    session.assert_complete();
}
```

**Step 3: Verify and commit**

```
feat: add ReplayGhApi adapter (#120)
```

---

### Task 4: Recording mode for CommandRunner

**Files:**
- Modify: `crates/flotilla-core/src/providers/replay.rs`

**Step 1: Add RecordingRunner**

```rust
/// A CommandRunner that delegates to a real runner and records all interactions.
pub struct RecordingRunner {
    session: ReplaySession,
    inner: Arc<dyn CommandRunner>,
}

impl RecordingRunner {
    pub fn new(session: ReplaySession, inner: Arc<dyn CommandRunner>) -> Self {
        Self { session, inner }
    }
}

#[async_trait]
impl CommandRunner for RecordingRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path) -> Result<String, String> {
        let result = self.inner.run(cmd, args, cwd).await;

        let (stdout, stderr, exit_code) = match &result {
            Ok(out) => (Some(out.clone()), None, 0),
            Err(err) => (None, Some(err.clone()), 1),
        };

        self.session.record(Interaction::Command {
            cmd: cmd.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: cwd.to_string_lossy().to_string(),
            stdout,
            stderr,
            exit_code,
        });

        result
    }

    async fn run_output(
        &self,
        cmd: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<CommandOutput, String> {
        let result = self.inner.run_output(cmd, args, cwd).await;

        if let Ok(ref output) = result {
            self.session.record(Interaction::Command {
                cmd: cmd.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                cwd: cwd.to_string_lossy().to_string(),
                stdout: Some(output.stdout.clone()),
                stderr: Some(output.stderr.clone()),
                exit_code: if output.success { 0 } else { 1 },
            });
        }

        result
    }

    async fn exists(&self, cmd: &str, args: &[&str]) -> bool {
        self.inner.exists(cmd, args).await
    }
}
```

**Step 2: Write a round-trip test (record then replay)**

```rust
#[tokio::test]
async fn record_then_replay() {
    use super::testing::MockRunner;

    let dir = tempfile::tempdir().unwrap();
    let fixture_path = dir.path().join("recorded.yaml");

    // Record phase: use MockRunner as the "real" backend
    {
        let mock = Arc::new(MockRunner::new(vec![
            Ok("hello\n".into()),
            Err("not found".into()),
        ]));
        let session = ReplaySession::recording(&fixture_path, Masks::new());
        let recorder = RecordingRunner::new(session.clone(), mock);

        let r1 = recorder.run("echo", &["hello"], Path::new("/tmp")).await;
        assert!(r1.is_ok());

        let r2 = recorder.run("missing", &[], Path::new("/tmp")).await;
        assert!(r2.is_err());

        session.save();
    }

    // Replay phase: verify the recorded fixture works
    {
        let session = ReplaySession::from_file(&fixture_path, Masks::new());
        let runner = session.command_runner();

        let r1 = runner.run("echo", &["hello"], Path::new("/tmp")).await;
        assert_eq!(r1.unwrap(), "hello\n");

        let r2 = runner.run("missing", &[], Path::new("/tmp")).await;
        assert!(r2.is_err());

        session.assert_complete();
    }
}
```

**Step 3: Verify and commit**

```
feat: add RecordingRunner for record mode (#120)
```

---

### Task 5: Apply to git.rs provider tests (#89)

**Files:**
- Create: `crates/flotilla-core/src/providers/vcs/fixtures/git_branches.yaml`
- Create: `crates/flotilla-core/src/providers/vcs/fixtures/git_remote_branches.yaml`
- Create: `crates/flotilla-core/src/providers/vcs/fixtures/git_working_tree.yaml`
- Modify: `crates/flotilla-core/src/providers/vcs/git.rs` (add tests)

Write replay fixtures and tests for all GitVcs methods: `branches()`, `remote_branches()`, `log()`, `working_tree_status()`, `unpushed_count()`. Each fixture captures the exact git command sequence and canned output. Tests assert on the parsed domain objects.

Examine `git.rs` to determine the exact command sequences, format strings, and parsing logic. Write fixtures that exercise normal output, empty output, and error cases.

**Commit:**
```
test: git.rs provider tests with record/replay (#89)
```

---

### Task 6: Apply to wt.rs provider tests (#89)

**Files:**
- Create: `crates/flotilla-core/src/providers/vcs/fixtures/wt_list.yaml`
- Create: `crates/flotilla-core/src/providers/vcs/fixtures/wt_switch.yaml`
- Create: `crates/flotilla-core/src/providers/vcs/fixtures/wt_remove.yaml`
- Modify: `crates/flotilla-core/src/providers/vcs/wt.rs` (add tests)

Write fixtures and tests for `checkouts()`, `switch()`, `remove()`. The `checkouts()` method parses `wt list --format=json` output — the fixture should include realistic JSON with branches, paths, ahead/behind, working tree status. Also test `strip_to_json()` which handles wt output format quirks (ANSI escapes, leading text before JSON).

**Commit:**
```
test: wt.rs provider tests with record/replay (#89)
```

---

### Task 7: Apply to GitHub provider tests (#90)

**Files:**
- Create: `crates/flotilla-core/src/providers/code_review/fixtures/github_prs.yaml`
- Create: `crates/flotilla-core/src/providers/issue_tracker/fixtures/github_issues.yaml`
- Modify: `crates/flotilla-core/src/providers/code_review/github.rs` (add tests)
- Modify: `crates/flotilla-core/src/providers/issue_tracker/github.rs` (add tests)

These providers use `GhApi` (not CommandRunner directly), so fixtures use `gh_api` channel entries. Test `list()` with various PR states, pagination (has_next_page), empty results, and error responses. Test issue listing with PR filtering logic.

**Commit:**
```
test: GitHub provider tests with record/replay (#90)
```

---

### Task 8: Apply to tmux.rs and zellij.rs provider tests (#91)

**Files:**
- Create: `crates/flotilla-core/src/providers/workspace/fixtures/tmux_list.yaml`
- Create: `crates/flotilla-core/src/providers/workspace/fixtures/tmux_create.yaml`
- Create: `crates/flotilla-core/src/providers/workspace/fixtures/zellij_list.yaml`
- Create: `crates/flotilla-core/src/providers/workspace/fixtures/zellij_create.yaml`
- Modify: `crates/flotilla-core/src/providers/workspace/tmux.rs` (add tests)
- Modify: `crates/flotilla-core/src/providers/workspace/zellij.rs` (add tests)

Test the async `WorkspaceManager` trait methods: `list()`, `create()`, `focus()`. These shell out to tmux/zellij commands via CommandRunner. Fixtures capture the command sequences. Note: tmux/zellij also read/write TOML state files — tests should use `tempdir` for the state directory.

**Commit:**
```
test: tmux/zellij workspace manager tests with record/replay (#91)
```

---

### Task 9: Update executor tests to use capturing runner (#127)

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs` (update tests)

Replace the hand-rolled mock providers that use `clone()` with consistent `take()` enforcement. For tests that need to verify command arguments (identified in #127: `create_checkout_with_issue_ids_writes_git_config`, `link_issues_success_*`, `teleport_session_*`), use ReplayRunner to verify the exact commands issued.

Also standardize MockCheckoutManager, MockWorkspaceManager, MockCodingAgent, MockAiUtility to all use `Option<T>` with `take()` for one-call enforcement.

**Commit:**
```
fix: standardize mock provider enforcement and add arg verification (#127)
```

---

### Task 10: Final verification

**Step 1:** Run full test suite: `cargo test --locked --workspace`

**Step 2:** Run clippy: `cargo clippy --all-targets --locked -- -D warnings`

**Step 3:** Run fmt: `cargo fmt --check`

**Step 4:** Check coverage improvement — the new tests should significantly increase coverage for git.rs, wt.rs, code_review/github.rs, issue_tracker/github.rs, tmux.rs, zellij.rs.
