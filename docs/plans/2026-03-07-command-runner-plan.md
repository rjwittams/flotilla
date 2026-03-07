# CommandRunner Trait Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the bare `run_cmd()`/`command_exists()` free functions with an injectable `CommandRunner` trait so all CLI-dependent providers can be unit-tested without real tools.

**Architecture:** Define a `CommandRunner` trait in `providers/mod.rs` with `run()` and `exists()` methods. A `ProcessCommandRunner` struct wraps existing `tokio::process::Command` logic. All providers and the executor gain an `Arc<dyn CommandRunner>` field. `discovery.rs` creates one shared instance and passes it everywhere.

**Tech Stack:** Rust, async-trait, tokio, Arc<dyn Trait>

---

### Task 1: Define CommandRunner trait and ProcessCommandRunner

**Files:**
- Modify: `crates/flotilla-core/src/providers/mod.rs`

**Step 1: Write the failing test**

Add a test module at the bottom of `providers/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn process_runner_echo() {
        let runner = ProcessCommandRunner;
        let result = runner.run("echo", &["hello"], &PathBuf::from("/")).await;
        assert_eq!(result.unwrap().trim(), "hello");
    }

    #[tokio::test]
    async fn process_runner_exists_true() {
        let runner = ProcessCommandRunner;
        assert!(runner.exists("echo", &["test"]).await);
    }

    #[tokio::test]
    async fn process_runner_exists_false() {
        let runner = ProcessCommandRunner;
        assert!(!runner.exists("nonexistent-binary-xyz", &[]).await);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core providers::tests --no-run 2>&1 | head -20`
Expected: compile error — `ProcessCommandRunner` not defined

**Step 3: Write the trait and default implementation**

Add above the existing `run_cmd` function in `providers/mod.rs`:

```rust
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path) -> Result<String, String>;
    async fn exists(&self, cmd: &str, args: &[&str]) -> bool;
}

pub struct ProcessCommandRunner;

#[async_trait]
impl CommandRunner for ProcessCommandRunner {
    async fn run(&self, cmd: &str, args: &[&str], cwd: &Path) -> Result<String, String> {
        let output = tokio::process::Command::new(cmd)
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    async fn exists(&self, cmd: &str, args: &[&str]) -> bool {
        tokio::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
```

Keep the existing `run_cmd()` and `command_exists()` free functions for now — they'll be removed after all callers are migrated.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-core providers::tests`
Expected: 3 tests pass

**Step 5: Commit**

```
git add crates/flotilla-core/src/providers/mod.rs
git commit -m "feat: define CommandRunner trait and ProcessCommandRunner"
```

---

### Task 2: Inject runner into GitVcs

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/git.rs`

**Step 1: Add runner field and update constructor**

```rust
use std::sync::Arc;
use crate::providers::CommandRunner;

pub struct GitVcs {
    runner: Arc<dyn CommandRunner>,
}

impl GitVcs {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }
}
```

Remove the `Default` impl (it can no longer default-construct without a runner).

**Step 2: Replace all `run_cmd(...)` calls with `self.runner.run(...)`**

Remove `use crate::providers::run_cmd;`. In every async method on the `Vcs` impl, replace `run_cmd("git", ...)` with `self.runner.run("git", ...)`. There are 7 call sites:
- `list_local_branches` (line 70)
- `list_remote_branches` (lines 89, 96)
- `commit_log` (line 116)
- `ahead_behind` (line 136)
- `working_tree_status` (line 154)

Leave `resolve_repo_root` unchanged (synchronous).

**Step 3: Verify it compiles (don't run tests yet — discovery.rs constructor not updated)**

Run: `cargo check -p flotilla-core 2>&1 | head -30`
Expected: errors in `discovery.rs` about `GitVcs::new()` taking args — that's fine, we'll fix it in Task 14.

**Step 4: Commit**

```
git add crates/flotilla-core/src/providers/vcs/git.rs
git commit -m "refactor: inject CommandRunner into GitVcs"
```

---

### Task 3: Inject runner into GitCheckoutManager

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/git_worktree.rs`

**Step 1: Add runner field**

```rust
use std::sync::Arc;
use crate::providers::CommandRunner;

pub struct GitCheckoutManager {
    config: CheckoutsConfig,
    env: minijinja::Environment<'static>,
    runner: Arc<dyn CommandRunner>,
}
```

Update `new()` to accept `runner: Arc<dyn CommandRunner>` and store it.

**Step 2: Convert static methods to instance methods**

`default_branch` and `enrich_checkout` are currently `async fn ...(repo_root: &Path)` (static). They call `run_cmd`. Change them to take `&self` and use `self.runner.run(...)`:

- `default_branch(&self, repo_root: &Path)` — 3 `run_cmd` calls
- `enrich_checkout(&self, path, branch, is_trunk, default_branch)` — 4 `run_cmd` calls

Update call sites in `list_checkouts`, `create_checkout`, `remove_checkout` to use `self.default_branch(...)` and `self.enrich_checkout(...)`.

**Step 3: Replace remaining `run_cmd` calls**

In `list_checkouts`, `create_checkout`, `remove_checkout`:
- `run_cmd("git", &["worktree", "list", ...], ...)` → `self.runner.run("git", ...)`
- `run_cmd("git", &["show-ref", ...], ...)` → `self.runner.run("git", ...)`
- `run_cmd("git", &["worktree", "add", ...], ...)` → `self.runner.run("git", ...)`
- `run_cmd("git", &["worktree", "remove", ...], ...)` → `self.runner.run("git", ...)`
- `run_cmd("git", &["branch", "-D", ...], ...)` → `self.runner.run("git", ...)`

Remove `use crate::providers::run_cmd;`.

**Step 4: Verify existing tests pass**

Run: `cargo test -p flotilla-core vcs::git_worktree::tests`
Expected: all existing tests pass (they test `parse_porcelain`, `render_worktree_path`, etc. — no async calls)

**Step 5: Commit**

```
git add crates/flotilla-core/src/providers/vcs/git_worktree.rs
git commit -m "refactor: inject CommandRunner into GitCheckoutManager"
```

---

### Task 4: Inject runner into WtCheckoutManager

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/wt.rs`

**Step 1: Add runner field and update constructor**

```rust
use std::sync::Arc;
use crate::providers::CommandRunner;

pub struct WtCheckoutManager {
    runner: Arc<dyn CommandRunner>,
}

impl WtCheckoutManager {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }
}
```

Remove `Default` impl.

**Step 2: Replace `run_cmd` calls with `self.runner.run`**

5 call sites in `list_checkouts`, `create_checkout`, `remove_checkout`. All are `run_cmd("wt", ...)`.

Remove `use crate::providers::run_cmd;`.

**Step 3: Commit**

```
git add crates/flotilla-core/src/providers/vcs/wt.rs
git commit -m "refactor: inject CommandRunner into WtCheckoutManager"
```

---

### Task 5: Migrate read_branch_issue_links to accept runner

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/mod.rs`

**Step 1: Change `read_branch_issue_links` signature**

```rust
pub async fn read_branch_issue_links(
    repo_root: &Path,
    branch: &str,
    runner: &dyn CommandRunner,
) -> Vec<AssociationKey> {
    // ...
    let result = runner.run("git", &["config", "--get-regexp", &pattern], repo_root).await;
    // ...
}
```

Add `use crate::providers::CommandRunner;` to the imports.

**Step 2: Update callers**

In `wt.rs` `list_checkouts`: pass `&*self.runner` to `read_branch_issue_links`.
In `git_worktree.rs` `enrich_checkout`: pass runner reference to `read_branch_issue_links`.

**Step 3: Verify tests pass**

Run: `cargo test -p flotilla-core vcs::tests`
Expected: pass (existing tests don't call `read_branch_issue_links`)

**Step 4: Commit**

```
git add crates/flotilla-core/src/providers/vcs/
git commit -m "refactor: pass CommandRunner to read_branch_issue_links"
```

---

### Task 6: Inject runner into GhApiClient

**Files:**
- Modify: `crates/flotilla-core/src/providers/github_api.rs`

**Step 1: Add runner field**

```rust
pub struct GhApiClient {
    cache: Mutex<HashMap<String, CacheEntry>>,
    runner: Arc<dyn CommandRunner>,
}
```

Update `new()` to accept `runner: Arc<dyn CommandRunner>`. Remove `#[derive(Default)]`.

**Step 2: Migrate `get()` method**

The `get()` method at line 90 uses `tokio::process::Command::new("gh")` directly with custom logic (it needs to parse `--include` output including headers). Replace with `self.runner.run(...)`.

Note: the current code checks `!output.status.success()` separately from parsing, and reads both stdout and stderr. The `CommandRunner::run()` method maps these correctly (stdout on success, stderr on failure). However, gh api `--include` writes headers to stdout even on 304, and the current code always reads stdout. We need to handle this:

The runner's `run()` returns `Err(stderr)` on non-zero exit. But `gh api` with `--include` returns non-zero on 304. Currently the code reads stdout regardless of exit status. We have two options:
- (a) Add a `run_raw` method that returns both stdout and exit status
- (b) Keep `GhApiClient::get()` using direct Command::new — it's already behind the `GhApiClient` abstraction

**Choose (b)**: `GhApiClient::get()` already provides a mockable seam (the `GhApiClient` struct itself). Its internal Command usage is an implementation detail. Only migrate the runner field through for consistency in construction, but leave the `get()` implementation using `Command::new` directly.

Wait — actually, reading the code more carefully: `gh api --include` returns status 0 for both 200 and 304 responses. The code checks `!output.status.success()` after parsing the HTTP status from stdout. So `runner.run()` would return Ok(stdout) in both cases. Let's migrate it.

Replace the `tokio::process::Command::new("gh")` block with:

```rust
let raw = self.runner.run("gh", &args_refs, repo_root).await?;
let parsed = parse_gh_api_response(&raw);
```

But wait — on actual HTTP errors (like 404), gh exits non-zero and the error is in stderr. The runner would return `Err(stderr)` which is correct. However, we lose access to stdout which has the parsed HTTP status. That's OK — the caller gets an error string from stderr which is sufficient.

Actually, re-reading more carefully: when gh api returns a 304, it exits with status 0. When it returns a 4xx/5xx, it also exits with status 0 but the HTTP status is in the headers. So `runner.run()` returning `Ok(stdout)` is fine for all cases. The `!output.status.success()` check at line 112 is for gh itself failing to run (network error, auth error, etc).

Hmm, I realize the current code at line 112 checks the process exit status AFTER parsing stdout. `gh` itself exits non-zero only when it fails at the process level. HTTP-level errors (4xx/5xx) are still exit 0 with the error in the HTTP response body.

Actually, looking more carefully: gh CLI actually does exit non-zero for HTTP errors. Let me re-check... yes, `gh api` exits non-zero for 4xx/5xx HTTP responses.

So the flow is:
1. `gh api --include` returns stdout with `HTTP/... 200 OK\n...\n\n{json}` — exit 0
2. `gh api --include` returns stdout with `HTTP/... 304 Not Modified\n...` — exit 0
3. `gh api --include` on 4xx: stdout has `HTTP/... 404 ...`, exit 1, stderr has error

For case 3, `runner.run()` would return `Err(stderr)`, but the current code reads stdout for the headers. This means the CommandRunner abstraction loses information.

**Decision: Leave `GhApiClient::get()` using direct `Command::new`.**  The `GhApiClient` is itself the mockable seam — code review and issue tracker use `api.get()`, not `run_cmd`. Just add the runner field for `command_exists` checks in `discovery.rs`.

Actually wait — `GhApiClient` doesn't call `command_exists` itself. The `command_exists("gh", ...)` calls are in `discovery.rs`. So `GhApiClient` doesn't need the runner at all.

**Revised plan: Skip GhApiClient entirely.** It doesn't call `run_cmd` or `command_exists`. Its `get()` method uses Command::new directly but provides its own mockable seam.

**Step 3: Commit (no-op — skip this task)**

---

### Task 6 (revised): Inject runner into GitHubCodeReview and GitHubIssueTracker

**Files:**
- Modify: `crates/flotilla-core/src/providers/code_review/github.rs`
- Modify: `crates/flotilla-core/src/providers/issue_tracker/github.rs`

Both have one `run_cmd` call each (in `open_in_browser`).

**Step 1: Add runner to GitHubCodeReview**

```rust
use crate::providers::CommandRunner;

pub struct GitHubCodeReview {
    provider_name: String,
    repo_slug: String,
    api: Arc<GhApiClient>,
    runner: Arc<dyn CommandRunner>,
}
```

Update `new()` to accept `runner: Arc<dyn CommandRunner>`.

Replace `run_cmd("gh", ...)` in `open_in_browser` with `self.runner.run("gh", ...)`.
Remove `use crate::providers::run_cmd;`.

**Step 2: Add runner to GitHubIssueTracker**

Same pattern:

```rust
pub struct GitHubIssueTracker {
    provider_name: String,
    repo_slug: String,
    api: Arc<GhApiClient>,
    runner: Arc<dyn CommandRunner>,
}
```

Update `new()`, replace `run_cmd` in `open_in_browser`.
Remove `use crate::providers::run_cmd;`.

**Step 3: Verify existing tests pass**

Run: `cargo test -p flotilla-core code_review::github::tests issue_tracker::github::tests`
Expected: pass

**Step 4: Commit**

```
git add crates/flotilla-core/src/providers/code_review/github.rs crates/flotilla-core/src/providers/issue_tracker/github.rs
git commit -m "refactor: inject CommandRunner into GitHub code review and issue tracker"
```

---

### Task 7: Inject runner into workspace managers (cmux, tmux, zellij)

**Files:**
- Modify: `crates/flotilla-core/src/providers/workspace/cmux.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/tmux.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/zellij.rs`

All three follow the same pattern: they have a private `*_cmd()` static async method that wraps `Command::new`. Convert to instance methods using `self.runner.run()`.

**Step 1: CmuxWorkspaceManager**

Add runner field. Update `new()`. Convert `cmux_cmd` from static to `&self` method using `self.runner.run(CMUX_BIN, args, ...)`.

Note: `cmux_cmd` doesn't take a `cwd` parameter — it runs without current_dir. Use `Path::new("/")` or similar as cwd. Actually, looking at the code, it doesn't set `current_dir` at all. We need a cwd for the runner. Use `std::env::current_dir()` or a sentinel.

**Important design note:** The `CommandRunner::run()` requires a `cwd: &Path`. But `cmux_cmd`, `tmux_cmd`, and `zellij_action` don't set cwd. Options:
- (a) Add a second method `run_no_cwd` to the trait
- (b) Pass a dummy cwd (the process cwd)
- (c) Make `cwd` optional in the trait signature

**Choose (a)**: The existing `run()` sets `current_dir` which is meaningful for git/gh commands. For tools like cmux/tmux/zellij that don't need cwd, we pass a reasonable default. Actually, `tokio::process::Command` without `current_dir` inherits the parent's cwd, which is also what `runner.run()` would do if we pass the current cwd. Let's just pass `&std::env::current_dir().unwrap_or_default()`.

Actually simpler: just pass `Path::new(".")`. The process will resolve it to the current directory.

For cmux, the binary is a fixed path `/Applications/cmux.app/Contents/Resources/bin/cmux`. The runner's `run()` takes `cmd` as the binary name. Pass `CMUX_BIN` as `cmd` and the remaining args.

Similarly `tmux_cmd` calls `Command::new("tmux")` → `self.runner.run("tmux", args, &cwd)`.
And `zellij_action` calls `Command::new("zellij")` → `self.runner.run("zellij", full_args, &cwd)`.

**Step 2: Convert CmuxWorkspaceManager**

```rust
pub struct CmuxWorkspaceManager {
    runner: Arc<dyn CommandRunner>,
}
```

Change `cmux_cmd` from `async fn cmux_cmd(args: &[&str])` to `async fn cmux_cmd(&self, args: &[&str])`. Use `self.runner.run(CMUX_BIN, args, Path::new("."))`.

Note: the current `cmux_cmd` has custom error formatting. The runner's `run()` returns `Err(stderr)` on failure. We need to preserve the error format. Wrap the runner call:

```rust
async fn cmux_cmd(&self, args: &[&str]) -> Result<String, String> {
    self.runner.run(CMUX_BIN, args, Path::new(".")).await
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("cmux {} failed: {}", args.first().unwrap_or(&""), e.trim()))
}
```

Update all `Self::cmux_cmd(...)` calls to `self.cmux_cmd(...)`.

Remove `Default` impl.

**Step 3: Convert TmuxWorkspaceManager**

Same pattern. Change `tmux_cmd` from static to `&self`. Replace `Command::new("tmux")` with `self.runner.run("tmux", ...)`.

Update all `Self::tmux_cmd(...)` calls to `self.tmux_cmd(...)`.

Remove `Default` impl.

**Step 4: Convert ZellijWorkspaceManager**

Same pattern for `zellij_action`. Change to `&self` method.

For `check_version`: currently a static method called from `discovery.rs`. Change it to take `runner: &dyn CommandRunner` parameter so discovery can call it before construction. Replace `Command::new("zellij")` with `runner.run("zellij", &["--version"], Path::new("."))`.

Remove `Default` impl.

**Step 5: Verify existing tests pass**

Run: `cargo test -p flotilla-core workspace`
Expected: all tmux/zellij/cmux unit tests pass (they don't call the async methods)

**Step 6: Commit**

```
git add crates/flotilla-core/src/providers/workspace/
git commit -m "refactor: inject CommandRunner into workspace managers"
```

---

### Task 8: Inject runner into ClaudeCodingAgent and ClaudeAiUtility

**Files:**
- Modify: `crates/flotilla-core/src/providers/coding_agent/claude.rs`
- Modify: `crates/flotilla-core/src/providers/ai_utility/claude.rs`

**Step 1: ClaudeCodingAgent**

Add `runner: Arc<dyn CommandRunner>` field. Update `new()`.

Migrate `read_oauth_token_from_keychain()` — currently a free function using `tokio::process::Command::new("security")`. Change it to take `runner: &dyn CommandRunner`:

```rust
async fn read_oauth_token_from_keychain(runner: &dyn CommandRunner) -> Result<OAuthToken, String> {
    let output = runner
        .run("security", &["find-generic-password", "-s", "Claude Code-credentials", "-w"], Path::new("."))
        .await
        .map_err(|_| "No Claude Code credentials in keychain".to_string())?;
    let json = output.trim();
    let creds: OAuthCredentials = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(creds.claude_ai_oauth)
}
```

Note: the current code checks `!output.status.success()` and returns a custom error. With the runner, a non-zero exit returns `Err`, so we map the error message.

Also: `get_oauth_token()` calls `read_oauth_token_from_keychain()`. It's a free function — it needs to receive `runner` too. Thread it through the call chain:

- `get_oauth_token(runner: &dyn CommandRunner)`
- Called from `fetch_sessions_inner` — but `fetch_sessions_inner` is a static method. Make it take `runner`.
- `fetch_sessions` also needs `runner`.
- `list_sessions` on the trait impl calls `Self::fetch_sessions()` — pass `&*self.runner`.

**Step 2: ClaudeAiUtility**

Add `runner: Arc<dyn CommandRunner>` field alongside `claude_bin`. Update `new()`.

Migrate `generate_branch_name`: replace `Command::new(&self.claude_bin).args(["-p", &prompt])` with `self.runner.run(&self.claude_bin, &["-p", &prompt], Path::new("."))`.

Handle the error/success branches — the runner returns Ok(stdout) or Err(stderr).

**Step 3: Commit**

```
git add crates/flotilla-core/src/providers/coding_agent/claude.rs crates/flotilla-core/src/providers/ai_utility/claude.rs
git commit -m "refactor: inject CommandRunner into Claude providers"
```

---

### Task 9: Migrate resolve_claude_path and first_remote_url

**Files:**
- Modify: `crates/flotilla-core/src/providers/mod.rs`
- Modify: `crates/flotilla-core/src/providers/discovery.rs`

**Step 1: Update `resolve_claude_path` signature**

```rust
pub async fn resolve_claude_path(runner: &dyn CommandRunner) -> Option<String> {
    if runner.exists("claude", &["--version"]).await {
        return Some("claude".to_string());
    }
    let known_paths = [dirs::home_dir().map(|h| h.join(".claude/local/claude"))];
    for path in known_paths.into_iter().flatten() {
        if path.is_file() {
            if runner.exists(path.to_str().unwrap_or(""), &["--version"]).await {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }
    None
}
```

**Step 2: Migrate `first_remote_url` in discovery.rs**

Currently uses `Command::new("git")` directly. Change to accept `runner: &dyn CommandRunner`:

```rust
pub async fn first_remote_url(repo_root: &Path, runner: &dyn CommandRunner) -> Option<String> {
    let remotes_output = runner.run("git", &["remote"], repo_root).await.ok()?;
    for remote in remotes_output.lines() {
        let remote = remote.trim();
        if remote.is_empty() { continue; }
        if let Ok(url) = runner.run("git", &["remote", "get-url", remote], repo_root).await {
            return Some(url.trim().to_string());
        }
    }
    None
}
```

**Step 3: Commit**

```
git add crates/flotilla-core/src/providers/mod.rs crates/flotilla-core/src/providers/discovery.rs
git commit -m "refactor: pass CommandRunner to resolve_claude_path and first_remote_url"
```

---

### Task 10: Inject runner into executor

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`

**Step 1: Add runner parameter to `execute()`**

```rust
pub async fn execute(
    cmd: Command,
    repo_root: &Path,
    registry: &ProviderRegistry,
    providers_data: &ProviderData,
    runner: &dyn CommandRunner,
) -> CommandResult {
```

**Step 2: Migrate the 3 direct `providers::run_cmd` calls**

- `LinkIssuesToPr`: `providers::run_cmd("gh", ...)` → `runner.run("gh", ...)`
- `write_branch_issue_links`: `providers::run_cmd("git", ...)` → `runner.run("git", ...)`
- `TeleportSession`: `providers::resolve_claude_path()` → `providers::resolve_claude_path(runner)` (already migrated in Task 9)

Update `write_branch_issue_links` to accept `runner: &dyn CommandRunner`.

**Step 3: Update callers of `executor::execute`**

In `crates/flotilla-core/src/in_process.rs` at line 209:

```rust
let result = executor::execute(command, &repo_root, &registry, &providers_data, &*runner).await;
```

The `InProcessDaemon` needs access to the runner. Since `detect_providers` creates it, we need to store it alongside the repo. Either:
- Store it in `RepoState`
- Create a shared runner in `InProcessDaemon::new()` and pass it to `detect_providers`

Choose: Create a shared `Arc<ProcessCommandRunner>` in `InProcessDaemon::new()` and pass it everywhere.

**Step 4: Commit**

```
git add crates/flotilla-core/src/executor.rs crates/flotilla-core/src/in_process.rs
git commit -m "refactor: inject CommandRunner into executor"
```

---

### Task 11: Update discovery.rs and InProcessDaemon construction

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery.rs`
- Modify: `crates/flotilla-core/src/in_process.rs`

This is where everything comes together.

**Step 1: Update `detect_providers` to accept and use runner**

```rust
pub async fn detect_providers(
    repo_root: &Path,
    runner: Arc<dyn CommandRunner>,
) -> (ProviderRegistry, Option<String>) {
```

Replace all construction sites:
- `GitVcs::new()` → `GitVcs::new(Arc::clone(&runner))`
- `WtCheckoutManager::new()` → `WtCheckoutManager::new(Arc::clone(&runner))`
- `GitCheckoutManager::new(co_config)` → `GitCheckoutManager::new(co_config, Arc::clone(&runner))`
- `command_exists("wt", ...)` → `runner.exists("wt", ...)`
- `command_exists("gh", ...)` → `runner.exists("gh", ...)`
- `first_remote_url(repo_root)` → `first_remote_url(repo_root, &*runner)`
- `resolve_claude_path()` → `resolve_claude_path(&*runner)`
- `ClaudeCodingAgent::new(name)` → `ClaudeCodingAgent::new(name, Arc::clone(&runner))`
- `ClaudeAiUtility::new(bin)` → `ClaudeAiUtility::new(bin, Arc::clone(&runner))`
- `GitHubCodeReview::new(name, slug, api)` → `GitHubCodeReview::new(name, slug, api, Arc::clone(&runner))`
- `GitHubIssueTracker::new(name, slug, api)` → `GitHubIssueTracker::new(name, slug, api, Arc::clone(&runner))`
- `CmuxWorkspaceManager::new()` → `CmuxWorkspaceManager::new(Arc::clone(&runner))`
- `TmuxWorkspaceManager::new()` → `TmuxWorkspaceManager::new(Arc::clone(&runner))`
- `ZellijWorkspaceManager::new()` → `ZellijWorkspaceManager::new(Arc::clone(&runner))`
- `ZellijWorkspaceManager::check_version()` → `ZellijWorkspaceManager::check_version(&*runner)`

Remove `use super::{command_exists, resolve_claude_path};` and update to `use super::resolve_claude_path;`.
Remove `use tokio::process::Command;` and `use std::process::Stdio;`.

**Step 2: Update InProcessDaemon**

Store a shared runner. Pass it to `detect_providers` and `executor::execute`.

```rust
pub struct InProcessDaemon {
    repos: RwLock<HashMap<PathBuf, RepoState>>,
    repo_order: RwLock<Vec<PathBuf>>,
    event_tx: broadcast::Sender<DaemonEvent>,
    runner: Arc<dyn CommandRunner>,
}
```

In `new()`:
```rust
let runner: Arc<dyn CommandRunner> = Arc::new(ProcessCommandRunner);
```

Pass `Arc::clone(&runner)` to `detect_providers` calls. Store in struct.

In `execute()`: pass `&*self.runner` to `executor::execute`.
In `add_repo()`: pass `Arc::clone(&self.runner)` to `detect_providers`.

**Step 3: Verify everything compiles**

Run: `cargo check`
Expected: clean compilation

**Step 4: Run all tests**

Run: `cargo test`
Expected: all tests pass

**Step 5: Commit**

```
git add crates/flotilla-core/src/providers/discovery.rs crates/flotilla-core/src/in_process.rs
git commit -m "refactor: thread shared CommandRunner through discovery and daemon"
```

---

### Task 12: Clean up deprecated free functions

**Files:**
- Modify: `crates/flotilla-core/src/providers/mod.rs`

**Step 1: Check for remaining callers**

Search for `run_cmd` and `command_exists` across the codebase. If no callers remain, remove:
- `pub async fn run_cmd(...)`
- `pub async fn command_exists(...)`

Keep `resolve_claude_path` (now takes `&dyn CommandRunner`).

**Step 2: Run all tests**

Run: `cargo test`
Expected: pass

**Step 3: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean

**Step 4: Commit**

```
git add crates/flotilla-core/src/providers/mod.rs
git commit -m "chore: remove deprecated run_cmd and command_exists free functions"
```

---

### Task 13: Write example mock test to validate the seam

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/git.rs` (add test)

**Step 1: Write a mock-based test**

Add a test that creates a `MockCommandRunner` and tests `GitVcs` without any real git binary:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::CommandRunner;
    use std::sync::Arc;

    struct MockRunner {
        responses: std::sync::Mutex<Vec<Result<String, String>>>,
    }

    impl MockRunner {
        fn new(responses: Vec<Result<String, String>>) -> Self {
            Self {
                responses: std::sync::Mutex::new(responses),
            }
        }
    }

    #[async_trait::async_trait]
    impl CommandRunner for MockRunner {
        async fn run(&self, _cmd: &str, _args: &[&str], _cwd: &Path) -> Result<String, String> {
            self.responses.lock().unwrap().remove(0)
        }
        async fn exists(&self, _cmd: &str, _args: &[&str]) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn list_local_branches_parses_output() {
        let runner = Arc::new(MockRunner::new(vec![
            Ok("main\nfeature/foo\nfix-bar\n".to_string()),
        ]));
        let vcs = GitVcs::new(runner);
        let branches = vcs.list_local_branches(Path::new("/fake")).await.unwrap();
        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0].name, "main");
        assert!(branches[0].is_trunk);
        assert_eq!(branches[1].name, "feature/foo");
        assert!(!branches[1].is_trunk);
    }

    #[tokio::test]
    async fn working_tree_status_parses_porcelain() {
        let runner = Arc::new(MockRunner::new(vec![
            Ok("M  src/main.rs\n?? new.rs\n".to_string()),
        ]));
        let vcs = GitVcs::new(runner);
        let status = vcs
            .working_tree_status(Path::new("/fake"), Path::new("/fake"))
            .await
            .unwrap();
        assert_eq!(status.staged, 1);
        assert_eq!(status.untracked, 1);
    }
}
```

**Step 2: Run the new tests**

Run: `cargo test -p flotilla-core vcs::git::tests`
Expected: pass

**Step 3: Commit**

```
git add crates/flotilla-core/src/providers/vcs/git.rs
git commit -m "test: add mock-based unit tests for GitVcs"
```

---

### Task 14: Final verification

**Step 1: Format**

Run: `cargo fmt`

**Step 2: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean

**Step 3: Full test suite**

Run: `cargo test --locked`
Expected: all pass

**Step 4: Commit any formatting fixes**

```
git add -A
git commit -m "chore: fmt and clippy fixes"
```
