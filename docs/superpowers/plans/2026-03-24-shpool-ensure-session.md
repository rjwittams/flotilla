# Shpool ensure_session Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make shpool's `ensure_session` actually create the session (attach + detach), so `attach_args` becomes a simple reattach — fixing the `${SHELL:-/bin/sh}` expansion bug and aligning shpool's model with cleat's.

**Architecture:** Add `env_vars` parameter to the `TerminalPool::ensure_session` trait method. Shpool's `ensure_session` resolves `$SHELL` in Rust, builds a `--cmd` string with env vars and the resolved shell path, spawns `shpool attach` to create the session, then calls `shpool detach` to disconnect. Shpool's `attach_args` simplifies to a plain reattach (no `--cmd`, no env, no shell expansion).

**Tech Stack:** Rust, async-trait, tokio, shpool CLI

**Key design note — PTY requirement:** Shpool `attach` may require a TTY on the attaching process. If `ensure_session` runs from the daemon (piped I/O, no TTY), shpool might reject the attach. The plan includes an early verification step. If it fails, we'll need to allocate a PTY via the `pty-process` crate or use `script -q /dev/null` as a wrapper.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/flotilla-core/src/providers/terminal/mod.rs` | Modify | Add `env_vars` to `ensure_session` trait |
| `crates/flotilla-core/src/providers/terminal/shpool.rs` | Modify | Implement real `ensure_session`, simplify `attach_args` |
| `crates/flotilla-core/src/providers/terminal/shpool/tests.rs` | Modify | Update tests for new behavior |
| `crates/flotilla-core/src/providers/terminal/cleat.rs` | Modify | Accept new `env_vars` param (no behavioral change) |
| `crates/flotilla-core/src/providers/terminal/passthrough.rs` | Modify | Accept new `env_vars` param (no behavioral change) |
| `crates/flotilla-core/src/terminal_manager.rs` | Modify | Thread `daemon_socket_path` into `ensure_running` |
| `crates/flotilla-core/src/terminal_manager/tests.rs` | Modify | Update `ensure_running` call sites |
| `crates/flotilla-core/src/executor/terminals.rs` | Modify | Pass `daemon_socket_path` to `ensure_running` |
| `crates/flotilla-core/src/executor/tests.rs` | Modify | Update test call sites |
| `crates/flotilla-core/src/hop_chain/tests.rs` | Modify | Update mock `ensure_session` signatures |
| `crates/flotilla-core/src/hop_chain/snapshots/*.snap` | Modify | Snapshots may change if local attach args change |
| `crates/flotilla-core/src/providers/discovery/test_support.rs` | Modify | Update mock `ensure_session` signature |
| `crates/flotilla-core/src/refresh/tests.rs` | Check | May have mock TerminalPool that needs updating |

---

### Task 1: Verify shpool attach works without a TTY

Before building the implementation, verify that shpool can create a session when spawned from a non-interactive process.

**Files:**
- None (manual verification)

- [ ] **Step 1: Test shpool attach from a non-TTY context**

Run from a shell to simulate what the daemon would do:

```bash
# Spawn shpool attach with piped I/O (no TTY), then detach
shpool attach --cmd 'sleep 300' test-verify-no-tty </dev/null &
ATTACH_PID=$!
sleep 1
shpool list --json
shpool detach test-verify-no-tty
wait $ATTACH_PID 2>/dev/null
shpool kill test-verify-no-tty
```

If `shpool list` shows `test-verify-no-tty` as a session, the approach works.
If it fails with a TTY error, we'll need PTY allocation — flag this before proceeding.

---

### Task 2: Add `env_vars` to `TerminalPool::ensure_session` trait

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/mod.rs:29`

- [ ] **Step 1: Update the trait signature**

```rust
async fn ensure_session(&self, session_name: &str, command: &str, cwd: &Path, env_vars: &TerminalEnvVars) -> Result<(), String>;
```

- [ ] **Step 2: Verify compilation fails**

Run: `cargo check -p flotilla-core 2>&1 | head -30`
Expected: compilation errors in all `TerminalPool` implementations (shpool, cleat, passthrough) and mocks.

- [ ] **Step 3: Fix cleat — accept new param, no behavioral change**

In `crates/flotilla-core/src/providers/terminal/cleat.rs`, update `ensure_session` signature to accept `env_vars: &TerminalEnvVars`. Don't use the parameter yet — cleat's `create` command doesn't need it for now. Add `_` prefix:

```rust
async fn ensure_session(&self, session_name: &str, command: &str, cwd: &Path, _env_vars: &TerminalEnvVars) -> Result<(), String> {
```

- [ ] **Step 4: Fix passthrough — accept new param**

In `crates/flotilla-core/src/providers/terminal/passthrough.rs`:

```rust
async fn ensure_session(&self, _session_name: &str, _command: &str, _cwd: &std::path::Path, _env_vars: &TerminalEnvVars) -> Result<(), String> {
```

Add the import for `TerminalEnvVars` if not already present (it's imported via `super::` in cleat/passthrough).

- [ ] **Step 5: Fix shpool — accept new param temporarily**

In `crates/flotilla-core/src/providers/terminal/shpool.rs`, update the no-op `ensure_session` to accept the new param (we'll implement it fully in Task 4):

```rust
async fn ensure_session(&self, _session_name: &str, _command: &str, _cwd: &Path, _env_vars: &TerminalEnvVars) -> Result<(), String> {
    // No-op: shpool creates sessions on first `attach`.
    Ok(())
}
```

- [ ] **Step 6: Fix all mock implementations**

Search for all `fn ensure_session` in test files and update signatures. Key locations:
- `crates/flotilla-core/src/terminal_manager/tests.rs` (multiple SharedMock impls)
- `crates/flotilla-core/src/hop_chain/tests.rs` (if any mock)
- `crates/flotilla-core/src/executor/tests.rs` (if any mock)
- `crates/flotilla-core/src/providers/discovery/test_support.rs`
- `crates/flotilla-core/src/refresh/tests.rs`
- `crates/flotilla-core/tests/in_process_daemon.rs`

Each mock should accept the new `_env_vars: &TerminalEnvVars` parameter.

- [ ] **Step 7: Verify it compiles**

Run: `cargo check -p flotilla-core 2>&1 | tail -5`
Expected: success (or only warnings)

- [ ] **Step 8: Run tests**

Run: `cargo test --workspace --locked 2>&1 | tail -10`
Expected: all tests pass (no behavioral change yet)

- [ ] **Step 9: Commit**

```
feat: add env_vars parameter to TerminalPool::ensure_session
```

---

### Task 3: Thread `daemon_socket_path` into `TerminalManager::ensure_running`

**Files:**
- Modify: `crates/flotilla-core/src/terminal_manager.rs:110-121`
- Modify: `crates/flotilla-core/src/executor/terminals.rs:61`
- Modify: `crates/flotilla-core/src/terminal_manager/tests.rs`
- Modify: `crates/flotilla-core/src/executor/tests.rs`

- [ ] **Step 1: Write failing test — ensure_running passes env vars to pool**

In `crates/flotilla-core/src/terminal_manager/tests.rs`, update the `ensure_running_uses_attachable_id_as_session_name` test. The SharedMock's `ensure_session` should now record env_vars. Update `PoolCall::EnsureSession` to include env_vars:

```rust
EnsureSession { session_name: String, command: String, cwd: PathBuf, env_vars: TerminalEnvVars },
```

And update the SharedMock impl to record them. Then assert that env_vars contains `FLOTILLA_ATTACHABLE_ID` when `ensure_running` is called with a socket path.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core ensure_running_uses_attachable_id 2>&1 | tail -10`
Expected: FAIL — `ensure_running` doesn't pass env_vars yet.

- [ ] **Step 3: Update `ensure_running` to accept `daemon_socket_path` and build env vars**

In `crates/flotilla-core/src/terminal_manager.rs`:

```rust
pub async fn ensure_running(&self, attachable_id: &AttachableId, daemon_socket_path: Option<&str>) -> Result<(), String> {
    let (command, cwd) = {
        let store = self.store.lock().map_err(|e| format!("failed to lock store: {e}"))?;
        let attachable =
            store.registry().attachables.get(attachable_id).ok_or_else(|| format!("attachable not found: {attachable_id}"))?;
        match &attachable.content {
            AttachableContent::Terminal(t) => (t.command.clone(), t.working_directory.clone()),
        }
    };
    let mut env_vars: TerminalEnvVars = vec![("FLOTILLA_ATTACHABLE_ID".to_string(), attachable_id.to_string())];
    if let Some(socket) = daemon_socket_path {
        env_vars.push(("FLOTILLA_DAEMON_SOCKET".to_string(), socket.to_string()));
    }
    let session_name = attachable_id.to_string();
    self.pool.ensure_session(&session_name, &command, &cwd, &env_vars).await
}
```

Add import for `TerminalEnvVars`:
```rust
use crate::providers::terminal::TerminalEnvVars;
```

- [ ] **Step 4: Update callers of `ensure_running`**

In `crates/flotilla-core/src/executor/terminals.rs`, both call sites (lines ~61 and ~118) need to pass the socket path:

```rust
if let Err(err) = self.terminal_manager.ensure_running(&attachable_id, socket_str.as_deref()).await {
```

The `socket_str` is already available at both call sites from `self.daemon_socket_path.map(|p| p.display().to_string())`.

- [ ] **Step 5: Fix remaining test compilation**

Update all test call sites for `ensure_running` to pass the new parameter. Key locations:
- `crates/flotilla-core/src/terminal_manager/tests.rs` — `ensure_running(&att_id, Some("/tmp/flotilla.sock"))` or `ensure_running(&att_id, None)`
- `crates/flotilla-core/src/executor/tests.rs` — if any direct calls

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace --locked 2>&1 | tail -10`
Expected: all pass

- [ ] **Step 7: Commit**

```
feat: thread daemon_socket_path into ensure_running for env var injection
```

---

### Task 4: Implement shpool's `ensure_session` — create session via attach + detach

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/shpool.rs:479-482`
- Modify: `crates/flotilla-core/src/providers/terminal/shpool/tests.rs`

- [ ] **Step 1: Write failing test — ensure_session runs attach then detach**

In `crates/flotilla-core/src/providers/terminal/shpool/tests.rs`:

```rust
#[tokio::test]
async fn ensure_session_creates_via_attach_then_detach() {
    let runner = Arc::new(MockRunner::new(vec![
        Ok(String::new()), // attach returns (process exits after detach)
        Ok(String::new()), // detach returns
    ]));
    let (pool, _dir) = test_pool(runner.clone());
    let env = vec![
        ("FLOTILLA_ATTACHABLE_ID".to_string(), "test-uuid".to_string()),
    ];

    pool.ensure_session("test-session", "claude", Path::new("/repo"), &env)
        .await
        .expect("ensure_session");

    let calls = runner.calls();
    assert_eq!(calls.len(), 2, "should call attach then detach: {calls:?}");

    // First call: shpool attach with --cmd
    assert_eq!(calls[0].0, "shpool");
    let attach_args = &calls[0].1;
    assert!(attach_args.contains(&"attach".to_string()), "first call should be attach: {attach_args:?}");
    assert!(attach_args.contains(&"--cmd".to_string()), "attach should have --cmd: {attach_args:?}");
    assert!(attach_args.contains(&"test-session".to_string()), "attach should include session name: {attach_args:?}");

    // The --cmd value should contain the resolved shell (not ${SHELL:-/bin/sh})
    let cmd_idx = attach_args.iter().position(|a| a == "--cmd").expect("--cmd present");
    let cmd_val = &attach_args[cmd_idx + 1];
    assert!(!cmd_val.contains("${SHELL"), "should not contain unresolved shell variable: {cmd_val}");
    assert!(cmd_val.contains("FLOTILLA_ATTACHABLE_ID"), "should contain env var: {cmd_val}");

    // Second call: shpool detach
    assert_eq!(calls[1].0, "shpool");
    let detach_args = &calls[1].1;
    assert!(detach_args.contains(&"detach".to_string()), "second call should be detach: {detach_args:?}");
    assert!(detach_args.contains(&"test-session".to_string()), "detach should include session name: {detach_args:?}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core ensure_session_creates_via_attach 2>&1 | tail -10`
Expected: FAIL — `ensure_session` is still a no-op.

- [ ] **Step 3: Implement `ensure_session`**

In `crates/flotilla-core/src/providers/terminal/shpool.rs`, replace the no-op:

```rust
async fn ensure_session(&self, session_name: &str, command: &str, cwd: &Path, env_vars: &TerminalEnvVars) -> Result<(), String> {
    // Resolve $SHELL from the process environment — avoids shell expansion
    // issues with shpool's shell-words tokenizer.
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

    // Build the --cmd value: env K=V ... /resolved/shell [-lic command]
    let mut cmd_parts: Vec<String> = Vec::new();
    if !env_vars.is_empty() {
        cmd_parts.push("env".to_string());
        for (k, v) in env_vars {
            cmd_parts.push(format!("{k}={v}"));
        }
    }
    cmd_parts.push(shell);
    if !command.is_empty() {
        cmd_parts.push("-lic".to_string());
        cmd_parts.push(command.to_string());
    }
    let cmd_str = cmd_parts.join(" ");

    let socket_str = self.socket_path.display().to_string();
    let config_str = self.config_path.display().to_string();
    let cwd_str = cwd.display().to_string();

    // Create the session by attaching (shpool creates on first attach)
    run!(
        self.runner,
        "shpool",
        &["--socket", &socket_str, "-c", &config_str, "attach", "--cmd", &cmd_str, "--dir", &cwd_str, session_name],
        Path::new("/")
    )?;

    // Detach to release the session — it keeps running in the shpool daemon
    run!(
        self.runner,
        "shpool",
        &["--socket", &socket_str, "-c", &config_str, "detach", session_name],
        Path::new("/")
    )?;

    Ok(())
}
```

**Note:** The `run!` macro calls `runner.run_output()` which captures stdout/stderr. The attach call may fail if shpool requires a TTY — Task 1 should have verified this. If it does require a TTY, we'll need to spawn it differently (e.g., with a PTY or via `script`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p flotilla-core ensure_session_creates_via_attach 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: Write test — ensure_session with empty command**

```rust
#[tokio::test]
async fn ensure_session_empty_command_starts_login_shell() {
    let runner = Arc::new(MockRunner::new(vec![
        Ok(String::new()),
        Ok(String::new()),
    ]));
    let (pool, _dir) = test_pool(runner.clone());

    pool.ensure_session("test-session", "", Path::new("/repo"), &vec![])
        .await
        .expect("ensure_session");

    let calls = runner.calls();
    let cmd_idx = calls[0].1.iter().position(|a| a == "--cmd").expect("--cmd present");
    let cmd_val = &calls[0].1[cmd_idx + 1];
    // Should be just the resolved shell path, no -lic
    assert!(!cmd_val.contains("-lic"), "empty command should not have -lic: {cmd_val}");
}
```

- [ ] **Step 6: Run test**

Run: `cargo test -p flotilla-core ensure_session_empty_command 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 7: Commit**

```
feat: implement shpool ensure_session — create session via attach + detach
```

---

### Task 5: Simplify shpool's `attach_args` to plain reattach

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/shpool.rs:484-513`
- Modify: `crates/flotilla-core/src/providers/terminal/shpool/tests.rs`

- [ ] **Step 1: Write failing test — attach_args produces simple reattach**

In `crates/flotilla-core/src/providers/terminal/shpool/tests.rs`, add a new test:

```rust
#[test]
fn attach_args_simple_reattach() {
    let (pool, _dir) = test_pool(Arc::new(MockRunner::new(vec![])));
    let socket = pool.socket_path.display().to_string();
    let config = pool.config_path.display().to_string();
    let env = vec![("FLOTILLA_ATTACHABLE_ID".to_string(), "uuid".to_string())];
    let args = pool.attach_args("test-session", "claude", Path::new("/repo"), &env).expect("attach_args");

    // Should be a simple reattach — no --cmd, no NestedCommand
    assert_eq!(args, vec![
        Arg::Quoted("shpool".into()),
        Arg::Literal("--socket".into()),
        Arg::Quoted(socket),
        Arg::Literal("-c".into()),
        Arg::Quoted(config),
        Arg::Literal("attach".into()),
        Arg::Literal("--force".into()),
        Arg::Literal("--dir".into()),
        Arg::Quoted("/repo".into()),
        Arg::Quoted("test-session".into()),
    ]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core attach_args_simple_reattach 2>&1 | tail -10`
Expected: FAIL — current `attach_args` still produces `--cmd` with NestedCommand.

- [ ] **Step 3: Simplify `attach_args`**

Replace the current `attach_args` implementation in `shpool.rs`:

```rust
fn attach_args(&self, session_name: &str, _command: &str, cwd: &Path, _env_vars: &TerminalEnvVars) -> Result<Vec<Arg>, String> {
    Ok(vec![
        Arg::Quoted("shpool".into()),
        Arg::Literal("--socket".into()),
        Arg::Quoted(self.socket_path.display().to_string()),
        Arg::Literal("-c".into()),
        Arg::Quoted(self.config_path.display().to_string()),
        Arg::Literal("attach".into()),
        Arg::Literal("--force".into()),
        Arg::Literal("--dir".into()),
        Arg::Quoted(cwd.display().to_string()),
        Arg::Quoted(session_name.into()),
    ])
}
```

The `command` and `env_vars` parameters are now unused — they were baked in during `ensure_session`. `--force` ensures we can reattach even if another process is already attached.

- [ ] **Step 4: Run new test**

Run: `cargo test -p flotilla-core attach_args_simple_reattach 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: Update existing shpool attach_args tests**

The old tests (`attach_args_with_command_no_env`, `attach_args_flatten_with_command_no_env`, `attach_args_empty_command_no_env`, `attach_args_with_env_vars`, `attach_args_with_env_vars_empty_command`) all expect the old `--cmd` + NestedCommand structure. Replace them with tests that verify the new simple reattach behavior:

- `attach_args_with_command_no_env` → verify no `--cmd`, has `--force`
- `attach_args_flatten_with_command_no_env` → verify flattened string is simple
- `attach_args_empty_command_no_env` → verify same structure (no difference now)
- `attach_args_with_env_vars` → verify env_vars are ignored (no NestedCommand)
- `attach_args_with_env_vars_empty_command` → verify same

Also update `attach_builds_command` which checks for `--cmd` and `-lic`.

- [ ] **Step 6: Run all shpool tests**

Run: `cargo test -p flotilla-core shpool 2>&1 | tail -20`
Expected: all pass

- [ ] **Step 7: Commit**

```
feat: simplify shpool attach_args to plain reattach
```

---

### Task 6: Update hop chain snapshots and remaining tests

**Files:**
- Modify: `crates/flotilla-core/src/hop_chain/tests.rs`
- Modify: `crates/flotilla-core/src/hop_chain/snapshots/*.snap`
- Modify: `crates/flotilla-core/src/executor/tests.rs`

- [ ] **Step 1: Run full test suite to find failures**

Run: `cargo test --workspace --locked 2>&1 | grep 'FAILED\|failures'`

Identify all failing tests. The hop chain snapshots that reference shpool attach args may need updating. The executor tests that check for `--cmd` may need updating.

- [ ] **Step 2: Fix each failing test**

For each failure, investigate whether the change is an intended consequence of the new behavior. If yes, update the test/snapshot. If no, investigate the bug.

**Remember:** Never blindly accept snapshot changes. Each changed snapshot should be explainable by the design change.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace --locked 2>&1 | tail -10`
Expected: all pass

- [ ] **Step 4: Run CI checks**

Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no errors

- [ ] **Step 5: Commit**

```
test: update snapshots and tests for shpool ensure_session model
```

---

### Task 7: Remove `${SHELL:-/bin/sh}` from shpool entirely

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/shpool.rs`

- [ ] **Step 1: Verify no remaining references to `${SHELL:-/bin/sh}` in shpool code**

Run: `grep -n 'SHELL:-' crates/flotilla-core/src/providers/terminal/shpool.rs crates/flotilla-core/src/providers/terminal/shpool/tests.rs`
Expected: no matches (all removed by Task 4+5)

If any remain, remove them.

- [ ] **Step 2: Run full test suite**

Run: `cargo test --workspace --locked 2>&1 | tail -10`
Expected: all pass

- [ ] **Step 3: Commit (if changes were made)**

```
chore: remove residual ${SHELL:-/bin/sh} references from shpool
```
