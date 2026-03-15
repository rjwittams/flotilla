# Remote Terminal Execution Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make remote terminal execution work end-to-end by fixing the SSH environment, adding connection multiplexing, and showing which host a workspace belongs to.

**Architecture:** Three changes to `wrap_remote_attach_commands` and its supporting code: (1) wrap remote commands in `$SHELL -l -c "..."` for proper login environment, (2) add SSH `ControlMaster` multiplexing controlled by `hosts.toml` config, (3) prefix remote workspace names with `host:`. Config changes add SSH settings to `HostsConfig` via the existing custom deserialize path.

**Tech Stack:** Rust, TOML (serde), SSH CLI args, existing `shell_quote` infrastructure.

**Design doc:** `docs/plans/2026-03-15-remote-terminal-execution-design.md`

---

### Task 1: Add `escape_for_double_quotes` helper and login shell wrapping

Escape characters that are special inside double quotes, then change `wrap_remote_attach_commands` to wrap commands in `$SHELL -l -c "..."`.

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs:901-937`

- [ ] **Step 1: Write tests for `escape_for_double_quotes`**

Add to `mod tests` in `crates/flotilla-core/src/executor.rs`:

```rust
#[test]
fn escape_for_double_quotes_handles_special_chars() {
    assert_eq!(escape_for_double_quotes("hello"), "hello");
    assert_eq!(escape_for_double_quotes(r#"say "hi""#), r#"say \"hi\""#);
    assert_eq!(escape_for_double_quotes("$HOME"), r"\$HOME");
    assert_eq!(escape_for_double_quotes("a`cmd`b"), r"a\`cmd\`b");
    assert_eq!(escape_for_double_quotes(r"back\slash"), r"back\\slash");
    assert_eq!(escape_for_double_quotes(""), "");
    // Shpool attach commands have no special chars — passthrough
    assert_eq!(
        escape_for_double_quotes("shpool --socket /tmp/s.sock attach flotilla/feat/main/0"),
        "shpool --socket /tmp/s.sock attach flotilla/feat/main/0"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core escape_for_double_quotes`
Expected: FAIL — `escape_for_double_quotes` not defined.

- [ ] **Step 3: Implement `escape_for_double_quotes`**

Add after `shell_quote` (line 937) in `crates/flotilla-core/src/executor.rs`:

```rust
fn escape_for_double_quotes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '"' | '$' | '`' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p flotilla-core escape_for_double_quotes`
Expected: PASS

- [ ] **Step 5: Write test for login shell wrapping**

Add to `mod tests`:

```rust
#[test]
fn wrap_remote_attach_commands_uses_login_shell() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("hosts.toml"),
        "[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/flotilla.sock\"\n",
    )
    .expect("write hosts config");

    let commands = vec![PreparedTerminalCommand { role: "main".into(), command: "claude".into() }];
    let result = wrap_remote_attach_commands(
        &HostName::new("desktop"),
        &PathBuf::from("/home/dev/project"),
        &commands,
        temp.path(),
    )
    .unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].role, "main");
    // Should use $SHELL -l -c with double-quoted inner command
    assert!(result[0].command.contains("$SHELL -l -c"), "expected login shell wrapper, got: {}", result[0].command);
    assert!(result[0].command.contains("ssh -t"), "expected ssh -t, got: {}", result[0].command);
    assert!(result[0].command.contains("desktop.local"), "expected host, got: {}", result[0].command);
    assert!(result[0].command.contains("/home/dev/project"), "expected remote dir, got: {}", result[0].command);
    assert!(result[0].command.contains("claude"), "expected command, got: {}", result[0].command);
}
```

- [ ] **Step 6: Update `wrap_remote_attach_commands` to use login shell wrapping**

Replace the function body at lines 901-919:

```rust
fn wrap_remote_attach_commands(
    target_host: &HostName,
    checkout_path: &Path,
    commands: &[PreparedTerminalCommand],
    config_base: &Path,
) -> Result<Vec<PreparedTerminalCommand>, String> {
    let ssh_target = remote_ssh_target(target_host, config_base)?;
    let remote_dir = checkout_path.display().to_string();
    Ok(commands
        .iter()
        .map(|entry| {
            let inner = format!("cd {} && {}", shell_quote(&remote_dir), entry.command);
            let login_wrapped = format!("$SHELL -l -c \"{}\"", escape_for_double_quotes(&inner));
            PreparedTerminalCommand {
                role: entry.role.clone(),
                command: format!("ssh -t {} {}", shell_quote(&ssh_target), shell_quote(&login_wrapped)),
            }
        })
        .collect())
}
```

Note: the `cd` argument uses `shell_quote` (single quotes) inside the double-quoted `-c` string. This is valid — single quotes work inside double quotes.

- [ ] **Step 7: Run all tests to verify nothing breaks**

Run: `cargo test -p flotilla-core wrap_remote && cargo test -p flotilla-core escape_for_double && cargo test -p flotilla-core create_workspace_from_prepared`
Expected: PASS — existing `.contains()` assertions still match because `"ssh -t"`, `"desktop.local"`, `"/remote/feat"`, and `"bash -l"` are all still present in the output.

- [ ] **Step 8: Add assertion for login shell to existing tests**

In `create_workspace_from_prepared_terminal_wraps_remote_commands_in_ssh` (line 1527), add after the existing assertions:

```rust
assert!(resolved[0].1.contains("$SHELL -l -c"), "expected login shell wrapper, got: {}", resolved[0].1);
```

In `create_workspace_from_prepared_terminal_uses_local_fallback_for_remote_only_repo` (line 1619), add:

```rust
let resolved = created[0].resolved_commands.as_ref().expect("resolved commands");
assert!(resolved[0].1.contains("$SHELL -l -c"), "expected login shell wrapper, got: {}", resolved[0].1);
```

- [ ] **Step 9: Run full test suite and clippy**

Run: `cargo test -p flotilla-core --locked && cargo clippy -p flotilla-core --all-targets --locked -- -D warnings`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "feat: wrap remote SSH commands in login shell for proper environment (#273)"
```

---

### Task 2: SSH config types and deserialization

Add `SshConfig` and `ssh_multiplex` fields to the hosts config, threading through the custom `Deserialize` impl.

**Files:**
- Modify: `crates/flotilla-core/src/config.rs:106-155`

- [ ] **Step 1: Write test for SSH config parsing**

Add to `mod tests` in `crates/flotilla-core/src/config.rs`:

```rust
#[test]
fn load_hosts_with_ssh_config() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    std::fs::write(
        base.join("hosts.toml"),
        "\
[ssh]\nmultiplex = false\n\n\
[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/d.sock\"\n\n\
[hosts.feta]\nhostname = \"feta.local\"\nexpected_host_name = \"feta\"\ndaemon_socket = \"/tmp/f.sock\"\nssh_multiplex = true\n",
    )
    .unwrap();
    let store = ConfigStore::with_base(base);
    let config = store.load_hosts().unwrap();
    // Global default is false
    assert!(!config.ssh.multiplex);
    // desktop inherits global (false)
    assert_eq!(config.hosts["desktop"].ssh_multiplex, None);
    assert!(!config.resolved_ssh_multiplex("desktop"));
    // feta overrides to true
    assert_eq!(config.hosts["feta"].ssh_multiplex, Some(true));
    assert!(config.resolved_ssh_multiplex("feta"));
}

#[test]
fn load_hosts_ssh_defaults_to_multiplex_true() {
    let dir = tempdir().unwrap();
    let base = dir.path();
    std::fs::write(
        base.join("hosts.toml"),
        "[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/d.sock\"\n",
    )
    .unwrap();
    let store = ConfigStore::with_base(base);
    let config = store.load_hosts().unwrap();
    // No [ssh] section — defaults to multiplex=true
    assert!(config.ssh.multiplex);
    assert!(config.resolved_ssh_multiplex("desktop"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core load_hosts_with_ssh`
Expected: FAIL — `SshConfig`, `ssh_multiplex`, `resolved_ssh_multiplex` not defined.

- [ ] **Step 3: Add SSH config types**

In `crates/flotilla-core/src/config.rs`, add before `HostsConfig` (line 106):

```rust
/// Global SSH settings for remote host connections.
#[derive(Debug, Clone, Deserialize)]
pub struct SshConfig {
    #[serde(default = "default_true")]
    pub multiplex: bool,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self { multiplex: true }
    }
}

fn default_true() -> bool {
    true
}
```

Add `ssh: SshConfig` field to `HostsConfig`:

```rust
#[derive(Debug, Default)]
pub struct HostsConfig {
    pub ssh: SshConfig,
    pub hosts: HashMap<String, RemoteHostConfig>,
}
```

Add `ssh_multiplex: Option<bool>` to `RemoteHostConfig`:

```rust
#[derive(Debug, Deserialize)]
pub struct RemoteHostConfig {
    pub hostname: String,
    pub expected_host_name: String,
    pub user: Option<String>,
    pub daemon_socket: String,
    pub ssh_multiplex: Option<bool>,
}
```

Add `ssh_multiplex: Option<bool>` to `RawRemoteHostConfig` and `ssh: Option<SshConfig>` to `RawHostsConfig`:

```rust
#[derive(Debug, Deserialize)]
struct RawHostsConfig {
    #[serde(default)]
    ssh: Option<SshConfig>,
    #[serde(default)]
    hosts: HashMap<String, RawRemoteHostConfig>,
}

#[derive(Debug, Deserialize)]
struct RawRemoteHostConfig {
    hostname: String,
    expected_host_name: Option<String>,
    user: Option<String>,
    daemon_socket: String,
    ssh_multiplex: Option<bool>,
}
```

Update the custom `Deserialize` impl for `HostsConfig` (line 134-155) to thread the new fields:

```rust
impl<'de> Deserialize<'de> for HostsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawHostsConfig::deserialize(deserializer)?;
        let ssh = raw.ssh.unwrap_or_default();
        let hosts = raw
            .hosts
            .into_iter()
            .map(|(label, host)| {
                let expected_host_name = host.expected_host_name.unwrap_or_else(|| label.clone());
                (label, RemoteHostConfig {
                    hostname: host.hostname,
                    expected_host_name,
                    user: host.user,
                    daemon_socket: host.daemon_socket,
                    ssh_multiplex: host.ssh_multiplex,
                })
            })
            .collect();
        Ok(Self { ssh, hosts })
    }
}
```

Add the resolution method on `HostsConfig`:

```rust
impl HostsConfig {
    /// Resolve SSH multiplex setting for a host label.
    /// Per-host `ssh_multiplex` overrides global `ssh.multiplex`.
    pub fn resolved_ssh_multiplex(&self, host_label: &str) -> bool {
        self.hosts
            .get(host_label)
            .and_then(|h| h.ssh_multiplex)
            .unwrap_or(self.ssh.multiplex)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-core load_hosts`
Expected: PASS (all existing `load_hosts_*` tests plus new ones)

- [ ] **Step 5: Run clippy**

Run: `cargo clippy -p flotilla-core --all-targets --locked -- -D warnings`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/flotilla-core/src/config.rs
git commit -m "feat: add SSH multiplex config to hosts.toml (#273)"
```

---

### Task 3: Refactor `remote_ssh_target` and add SSH multiplex args

Change `remote_ssh_target` to return a struct with both the SSH target and the resolved multiplex setting, then add multiplex args to the SSH command.

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs:901-937`

- [ ] **Step 1: Write tests for SSH multiplexing**

Add to `mod tests` in `crates/flotilla-core/src/executor.rs`:

```rust
#[test]
fn wrap_remote_attach_commands_includes_multiplex_args() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("hosts.toml"),
        "[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/flotilla.sock\"\n",
    )
    .expect("write hosts config");

    let commands = vec![PreparedTerminalCommand { role: "main".into(), command: "bash".into() }];
    let result = wrap_remote_attach_commands(
        &HostName::new("desktop"),
        &PathBuf::from("/home/dev/project"),
        &commands,
        temp.path(),
    )
    .unwrap();

    // Default is multiplex=true
    assert!(result[0].command.contains("ControlMaster=auto"), "expected ControlMaster, got: {}", result[0].command);
    assert!(result[0].command.contains("ControlPersist=60"), "expected ControlPersist, got: {}", result[0].command);
}

#[test]
fn wrap_remote_attach_commands_omits_multiplex_when_disabled() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("hosts.toml"),
        "[ssh]\nmultiplex = false\n\n[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/flotilla.sock\"\n",
    )
    .expect("write hosts config");

    let commands = vec![PreparedTerminalCommand { role: "main".into(), command: "bash".into() }];
    let result = wrap_remote_attach_commands(
        &HostName::new("desktop"),
        &PathBuf::from("/home/dev/project"),
        &commands,
        temp.path(),
    )
    .unwrap();

    assert!(!result[0].command.contains("ControlMaster"), "should not have ControlMaster when disabled, got: {}", result[0].command);
}

#[test]
fn wrap_remote_attach_commands_per_host_multiplex_override() {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        temp.path().join("hosts.toml"),
        "[ssh]\nmultiplex = true\n\n[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/flotilla.sock\"\nssh_multiplex = false\n",
    )
    .expect("write hosts config");

    let commands = vec![PreparedTerminalCommand { role: "main".into(), command: "bash".into() }];
    let result = wrap_remote_attach_commands(
        &HostName::new("desktop"),
        &PathBuf::from("/home/dev/project"),
        &commands,
        temp.path(),
    )
    .unwrap();

    assert!(!result[0].command.contains("ControlMaster"), "per-host override should disable multiplex, got: {}", result[0].command);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core wrap_remote_attach_commands`
Expected: FAIL — no multiplex args in output yet.

- [ ] **Step 3: Define `RemoteSshInfo` struct and refactor `remote_ssh_target`**

Replace `remote_ssh_target` (lines 921-933) with:

```rust
struct RemoteSshInfo {
    target: String,
    multiplex: bool,
}

fn remote_ssh_info(target_host: &HostName, config_base: &Path) -> Result<RemoteSshInfo, String> {
    let config = crate::config::ConfigStore::with_base(config_base);
    let hosts = config.load_hosts()?;
    let (label, remote) = hosts
        .hosts
        .iter()
        .find(|(_, host)| host.expected_host_name == target_host.as_str())
        .ok_or_else(|| format!("unknown remote host: {target_host}"))?;
    let target = match &remote.user {
        Some(user) => format!("{user}@{}", remote.hostname),
        None => remote.hostname.clone(),
    };
    let multiplex = hosts.resolved_ssh_multiplex(label);
    Ok(RemoteSshInfo { target, multiplex })
}
```

- [ ] **Step 4: Update `wrap_remote_attach_commands` to use `RemoteSshInfo` and add multiplex args**

```rust
fn wrap_remote_attach_commands(
    target_host: &HostName,
    checkout_path: &Path,
    commands: &[PreparedTerminalCommand],
    config_base: &Path,
) -> Result<Vec<PreparedTerminalCommand>, String> {
    let info = remote_ssh_info(target_host, config_base)?;
    let remote_dir = checkout_path.display().to_string();

    let multiplex_args = if info.multiplex {
        let ctrl_dir = config_base.join("ssh");
        if let Err(e) = std::fs::create_dir_all(&ctrl_dir) {
            tracing::warn!(err = %e, "failed to create SSH control socket directory, disabling multiplexing");
            String::new()
        } else {
            let ctrl_path = ctrl_dir.join("ctrl-%r@%h-%p");
            format!(
                " -o ControlMaster=auto -o ControlPath={} -o ControlPersist=60",
                shell_quote(&ctrl_path.display().to_string()),
            )
        }
    } else {
        String::new()
    };

    Ok(commands
        .iter()
        .map(|entry| {
            let inner = format!("cd {} && {}", shell_quote(&remote_dir), entry.command);
            let login_wrapped = format!("$SHELL -l -c \"{}\"", escape_for_double_quotes(&inner));
            PreparedTerminalCommand {
                role: entry.role.clone(),
                command: format!("ssh -t{} {} {}", multiplex_args, shell_quote(&info.target), shell_quote(&login_wrapped)),
            }
        })
        .collect())
}
```

- [ ] **Step 5: Run all SSH-related tests**

Run: `cargo test -p flotilla-core wrap_remote && cargo test -p flotilla-core create_workspace_from_prepared`
Expected: PASS

- [ ] **Step 6: Run full test suite and clippy**

Run: `cargo test -p flotilla-core --locked && cargo clippy -p flotilla-core --all-targets --locked -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "feat: add SSH connection multiplexing for remote terminals (#273)"
```

---

### Task 4: Host-prefixed workspace naming

Prefix remote workspace names with `target_host:` so they're visually distinguishable in the workspace manager UI.

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs:480-497`

- [ ] **Step 1: Write test for host-prefixed workspace name**

Add to `mod tests` in `crates/flotilla-core/src/executor.rs`:

```rust
#[tokio::test]
async fn create_workspace_from_prepared_terminal_prefixes_name_with_host() {
    let workspace_manager = Arc::new(MockWorkspaceManager::succeeding());
    let mut registry = empty_registry();
    registry.workspace_manager = Some((desc("cmux"), Arc::clone(&workspace_manager) as Arc<dyn WorkspaceManager>));
    let runner = runner_ok();
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_root = temp.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("create repo root");
    std::fs::write(
        temp.path().join("hosts.toml"),
        "[hosts.desktop]\nhostname = \"desktop.local\"\nexpected_host_name = \"desktop\"\ndaemon_socket = \"/tmp/flotilla.sock\"\n",
    )
    .expect("write hosts config");

    let repo = RepoExecutionContext {
        identity: flotilla_protocol::RepoIdentity { authority: "github.com".into(), path: "owner/repo".into() },
        root: repo_root,
    };
    let result = execute(
        CommandAction::CreateWorkspaceFromPreparedTerminal {
            target_host: HostName::new("desktop"),
            branch: "feat".into(),
            checkout_path: PathBuf::from("/remote/feat"),
            commands: vec![PreparedTerminalCommand { role: "main".into(), command: "bash".into() }],
        },
        &repo,
        &registry,
        &empty_data(),
        &runner,
        temp.path(),
        &local_host(),
    )
    .await;

    assert_ok(result);
    let created = workspace_manager.created_configs.lock().await;
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name, "desktop:feat", "workspace name should be host:branch");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core prefixes_name_with_host`
Expected: FAIL — name is `"feat"`, not `"desktop:feat"`.

- [ ] **Step 3: Update `CreateWorkspaceFromPreparedTerminal` to prefix name**

In `crates/flotilla-core/src/executor.rs` line 490, change:

```rust
let mut config = workspace_config(&repo.root, &branch, &working_dir, "claude", config_base);
```

to:

```rust
let remote_name = format!("{}:{}", target_host, branch);
let mut config = workspace_config(&repo.root, &remote_name, &working_dir, "claude", config_base);
```

- [ ] **Step 4: Run all tests**

Run: `cargo test -p flotilla-core --locked && cargo clippy -p flotilla-core --all-targets --locked -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "feat: prefix remote workspace names with host (#273)"
```

---

### Task 5: Full suite, format, and final validation

Run the full build pipeline to confirm everything works together.

**Files:** None — validation only.

- [ ] **Step 1: Format**

Run: `cargo +nightly fmt`

- [ ] **Step 2: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: PASS

- [ ] **Step 3: Full test suite**

Run: `cargo test --locked`
Expected: PASS

- [ ] **Step 4: Commit any format fixups**

```bash
git add -A && git commit -m "chore: fmt cleanup"
```

(Skip if no changes.)
