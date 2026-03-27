# Environment Provisioning Phase E Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect the TUI to the existing environment provisioning infrastructure so users can select a provisioning target and create checkouts inside Docker environments.

**Architecture:** Introduce `ProvisioningTarget` in `flotilla-protocol`, replace `UiState.target_host` and `Command.environment` with it, add `ReadEnvironmentSpec` step, and wire the command palette to build targets from host capabilities.

**Tech Stack:** Rust, ratatui, serde, tokio, YAML (serde_yaml)

---

### Task 1: ProvisioningTarget type in flotilla-protocol

**Files:**
- Create: `crates/flotilla-protocol/src/provisioning_target.rs`
- Modify: `crates/flotilla-protocol/src/lib.rs:19` (add re-export)
- Modify: `crates/flotilla-protocol/src/environment.rs:28-31` (rename field)

- [ ] **Step 1: Write tests for ProvisioningTarget parsing and display**

In `crates/flotilla-protocol/src/provisioning_target.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostName;

    #[test]
    fn display_host() {
        let target = ProvisioningTarget::Host(HostName::new("feta"));
        assert_eq!(target.to_string(), "@feta");
    }

    #[test]
    fn display_new_environment() {
        let target = ProvisioningTarget::NewEnvironment {
            host: HostName::new("feta"),
            provider: "docker".into(),
        };
        assert_eq!(target.to_string(), "+docker@feta");
    }

    #[test]
    fn display_existing_environment() {
        let target = ProvisioningTarget::ExistingEnvironment {
            host: HostName::new("feta"),
            env_id: crate::EnvironmentId::new("env-abc"),
        };
        assert_eq!(target.to_string(), "=env-abc@feta");
    }

    #[test]
    fn parse_host() {
        let target: ProvisioningTarget = "@feta".parse().unwrap();
        assert_eq!(target, ProvisioningTarget::Host(HostName::new("feta")));
    }

    #[test]
    fn parse_new_environment() {
        let target: ProvisioningTarget = "+docker@feta".parse().unwrap();
        assert_eq!(target, ProvisioningTarget::NewEnvironment {
            host: HostName::new("feta"),
            provider: "docker".into(),
        });
    }

    #[test]
    fn parse_existing_environment() {
        let target: ProvisioningTarget = "=env-abc@feta".parse().unwrap();
        assert_eq!(target, ProvisioningTarget::ExistingEnvironment {
            host: HostName::new("feta"),
            env_id: crate::EnvironmentId::new("env-abc"),
        });
    }

    #[test]
    fn parse_invalid_returns_error() {
        assert!("".parse::<ProvisioningTarget>().is_err());
        assert!("noprefix".parse::<ProvisioningTarget>().is_err());
    }

    #[test]
    fn host_accessor() {
        let bare = ProvisioningTarget::Host(HostName::new("feta"));
        assert_eq!(bare.host(), &HostName::new("feta"));

        let new_env = ProvisioningTarget::NewEnvironment {
            host: HostName::new("kiwi"),
            provider: "docker".into(),
        };
        assert_eq!(new_env.host(), &HostName::new("kiwi"));
    }

    #[test]
    fn serde_roundtrip() {
        use crate::test_helpers::assert_json_roundtrip;
        let cases = vec![
            ProvisioningTarget::Host(HostName::new("local")),
            ProvisioningTarget::NewEnvironment {
                host: HostName::new("feta"),
                provider: "docker".into(),
            },
            ProvisioningTarget::ExistingEnvironment {
                host: HostName::new("feta"),
                env_id: crate::EnvironmentId::new("env-abc"),
            },
        ];
        for case in cases {
            assert_json_roundtrip(&case);
        }
    }

    #[test]
    fn display_roundtrips_through_parse() {
        let targets = vec![
            ProvisioningTarget::Host(HostName::new("feta")),
            ProvisioningTarget::NewEnvironment {
                host: HostName::new("feta"),
                provider: "docker".into(),
            },
            ProvisioningTarget::ExistingEnvironment {
                host: HostName::new("feta"),
                env_id: crate::EnvironmentId::new("env-abc"),
            },
        ];
        for target in targets {
            let s = target.to_string();
            let parsed: ProvisioningTarget = s.parse().unwrap();
            assert_eq!(parsed, target);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-protocol --locked`
Expected: compilation failure — `provisioning_target` module doesn't exist yet.

- [ ] **Step 3: Implement ProvisioningTarget**

In `crates/flotilla-protocol/src/provisioning_target.rs`:

```rust
use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{EnvironmentId, HostName};

/// Where a checkout or workspace should be provisioned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProvisioningTarget {
    /// Run directly on the host's ambient environment.
    Host(HostName),
    /// Provision a fresh environment on the host using the named provider.
    NewEnvironment { host: HostName, provider: String },
    /// Reuse an existing running environment.
    ExistingEnvironment { host: HostName, env_id: EnvironmentId },
}

impl ProvisioningTarget {
    /// The host that manages this target.
    pub fn host(&self) -> &HostName {
        match self {
            ProvisioningTarget::Host(h) => h,
            ProvisioningTarget::NewEnvironment { host, .. } => host,
            ProvisioningTarget::ExistingEnvironment { host, .. } => host,
        }
    }
}

impl fmt::Display for ProvisioningTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProvisioningTarget::Host(host) => write!(f, "@{host}"),
            ProvisioningTarget::NewEnvironment { host, provider } => write!(f, "+{provider}@{host}"),
            ProvisioningTarget::ExistingEnvironment { host, env_id } => write!(f, "={env_id}@{host}"),
        }
    }
}

impl FromStr for ProvisioningTarget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = s.strip_prefix('+') {
            let (provider, host) = rest.split_once('@').ok_or_else(|| format!("expected +<provider>@<host>, got: {s}"))?;
            Ok(ProvisioningTarget::NewEnvironment {
                host: HostName::new(host),
                provider: provider.to_string(),
            })
        } else if let Some(rest) = s.strip_prefix('=') {
            let (env_id, host) = rest.split_once('@').ok_or_else(|| format!("expected =<env_id>@<host>, got: {s}"))?;
            Ok(ProvisioningTarget::ExistingEnvironment {
                host: HostName::new(host),
                env_id: EnvironmentId::new(env_id),
            })
        } else if let Some(host) = s.strip_prefix('@') {
            Ok(ProvisioningTarget::Host(HostName::new(host)))
        } else {
            Err(format!("expected @<host>, +<provider>@<host>, or =<env_id>@<host>, got: {s}"))
        }
    }
}
```

Add module declaration and re-export in `crates/flotilla-protocol/src/lib.rs`:

```rust
mod provisioning_target;
pub use provisioning_target::ProvisioningTarget;
```

- [ ] **Step 4: Rename token_requirements to token_env_vars**

In `crates/flotilla-protocol/src/environment.rs`, change line 30:

```rust
pub token_env_vars: Vec<String>,
```

Then fix all compilation errors from the rename (search for `token_requirements` across the workspace).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p flotilla-protocol --locked`
Expected: all tests pass.

- [ ] **Step 6: Run workspace-wide clippy and fmt**

Run: `cargo +nightly-2026-03-12 fmt && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add ProvisioningTarget type with display syntax and parsing"
```

---

### Task 2: Replace Command.environment with Command.provisioning_target

**Files:**
- Modify: `crates/flotilla-protocol/src/commands.rs:67-76` (Command struct)
- Modify: all call sites that construct `Command` with `environment: None` or `environment: Some(...)`

- [ ] **Step 1: Change the Command struct**

In `crates/flotilla-protocol/src/commands.rs`, replace the `environment` field (line 71) with `provisioning_target`:

```rust
pub struct Command {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<crate::HostName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioning_target: Option<crate::ProvisioningTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_repo: Option<RepoSelector>,
    #[serde(flatten)]
    pub action: CommandAction,
}
```

- [ ] **Step 2: Fix all compilation errors**

Every place that constructs a `Command` with `environment: None` now needs `provisioning_target: None`. These are numerous but mechanical. Key locations:

- `crates/flotilla-tui/src/widgets/branch_input.rs:52-54`
- `crates/flotilla-tui/src/app/mod.rs:399-401` (`targeted_command`)
- `crates/flotilla-tui/src/app/mod.rs:403-409` (`targeted_repo_command`)
- `crates/flotilla-tui/src/app/intent.rs` (various places)
- `crates/flotilla-commands/src/commands/checkout.rs` (all `Command` constructors)
- `crates/flotilla-commands/src/commands/` (all other noun files)
- `crates/flotilla-protocol/src/commands.rs` tests (all test cases)
- `crates/flotilla-core/src/executor.rs:111` (`build_plan` destructure)

Use `cargo build --workspace 2>&1 | head -50` iteratively to find and fix each site. This is a rename from `environment` to `provisioning_target` with a type change from `Option<EnvironmentSpec>` to `Option<ProvisioningTarget>`.

- [ ] **Step 3: Update build_plan to use provisioning_target**

In `crates/flotilla-core/src/executor.rs`, the `build_plan` function (line 111) currently destructures `environment` and checks `if let Some(spec) = environment`. Change this to match on `provisioning_target`:

```rust
let Command { host, provisioning_target, action, .. } = cmd;
let target_host = host.unwrap_or_else(|| local_host.clone());
let checkout_host = StepExecutionContext::Host(target_host.clone());

match (&provisioning_target, &action) {
    (Some(ProvisioningTarget::NewEnvironment { .. }), CommandAction::Checkout { .. }) => {
        if let CommandAction::Checkout { target, issue_ids, .. } = action {
            return Ok(build_new_environment_checkout_plan(target, issue_ids, target_host, local_host));
        }
    }
    (Some(ProvisioningTarget::ExistingEnvironment { env_id, .. }), CommandAction::Checkout { .. }) => {
        if let CommandAction::Checkout { target, issue_ids, .. } = action {
            return Ok(build_existing_environment_checkout_plan(env_id.clone(), target, issue_ids, target_host, local_host));
        }
    }
    _ => {}
}
```

Note: `build_environment_checkout_plan` no longer receives an `EnvironmentSpec` — that comes from the `ReadEnvironmentSpec` step at runtime instead.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace --locked`
Expected: all tests pass (existing behavior preserved — all current commands use `provisioning_target: None`).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: replace Command.environment with Command.provisioning_target"
```

---

### Task 3: Host capability advertising

**Files:**
- Modify: `crates/flotilla-core/src/model.rs:49-56` (add environment_providers to collection)

- [ ] **Step 1: Write a test**

In the existing test file for `model.rs`, or inline in `model.rs` if tests are there, add:

```rust
#[test]
fn provider_names_includes_environment_providers() {
    use crate::providers::registry::ProviderRegistry;
    let mut registry = ProviderRegistry::new();
    // Insert a mock environment provider descriptor
    // Check that provider_names_from_registry includes "environment" category
    let names = provider_names_from_registry(&registry);
    // Empty registry should have no environment entries
    assert!(!names.contains_key("environment"));
}
```

The exact test structure depends on whether `ProviderSet` allows inserting test descriptors. If not, this can be a simple addition with a compile-and-verify approach.

- [ ] **Step 2: Add environment_providers to provider_names_from_registry**

In `crates/flotilla-core/src/model.rs`, after line 56 (`collect_names(&mut names, &registry.terminal_pools);`), add:

```rust
    collect_names(&mut names, &registry.environment_providers);
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-core --locked`
Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/model.rs
git commit -m "feat: advertise environment providers in host summary"
```

---

### Task 4: ReadEnvironmentSpec step action and CommandValue variant

**Files:**
- Modify: `crates/flotilla-protocol/src/step.rs:131-147` (add ReadEnvironmentSpec variant)
- Modify: `crates/flotilla-protocol/src/commands.rs:241-298` (add EnvironmentSpecRead variant)

- [ ] **Step 1: Add StepAction::ReadEnvironmentSpec**

In `crates/flotilla-protocol/src/step.rs`, add a new variant to `StepAction` (alongside the existing environment lifecycle variants):

```rust
    ReadEnvironmentSpec,
```

No fields — it reads from the repo root known to the step resolver.

- [ ] **Step 2: Add CommandValue::EnvironmentSpecRead**

In `crates/flotilla-protocol/src/commands.rs`, add a new variant to `CommandValue`:

```rust
    EnvironmentSpecRead {
        spec: crate::EnvironmentSpec,
    },
```

- [ ] **Step 3: Add roundtrip test coverage**

Add a `EnvironmentSpecRead` case to the `command_value_roundtrip_covers_all_variants` test in `crates/flotilla-protocol/src/commands.rs`:

```rust
            CommandValue::EnvironmentSpecRead {
                spec: crate::EnvironmentSpec {
                    image: crate::ImageSource::Registry("ubuntu:24.04".into()),
                    token_env_vars: vec!["GITHUB_TOKEN".into()],
                },
            },
```

Add a `ReadEnvironmentSpec` case to the step action roundtrip test if one exists in `step.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p flotilla-protocol --locked`
Expected: passes.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add ReadEnvironmentSpec step action and EnvironmentSpecRead result"
```

---

### Task 5: Update environment checkout plans in executor

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs:298-352` (update build_environment_checkout_plan)

- [ ] **Step 1: Rewrite build_environment_checkout_plan for NewEnvironment**

Replace `build_environment_checkout_plan` (which currently takes an `EnvironmentSpec` parameter) with `build_new_environment_checkout_plan` that starts with `ReadEnvironmentSpec`:

```rust
fn build_new_environment_checkout_plan(
    target: CheckoutTarget,
    issue_ids: Vec<(String, String)>,
    target_host: HostName,
    local_host: HostName,
) -> StepPlan {
    let (branch, create_branch, intent) = match target {
        CheckoutTarget::Branch(branch) => (branch, false, CheckoutIntent::ExistingBranch),
        CheckoutTarget::FreshBranch(branch) => (branch, true, CheckoutIntent::FreshBranch),
    };

    let env_id = flotilla_protocol::EnvironmentId::new(uuid::Uuid::new_v4().to_string());
    let host_context = StepExecutionContext::Host(target_host.clone());
    let env_context = StepExecutionContext::Environment(target_host.clone(), env_id.clone());

    let mut steps = vec![
        Step {
            description: "Read environment spec".to_string(),
            host: host_context.clone(),
            action: StepAction::ReadEnvironmentSpec,
        },
        Step {
            description: "Ensure environment image".to_string(),
            host: host_context.clone(),
            action: StepAction::EnsureEnvironmentImage { spec: Default::default() },
        },
        // ... remaining steps same as current build_environment_checkout_plan
    ];
    // ...
}
```

Wait — `EnsureEnvironmentImage` currently takes an `EnvironmentSpec` in its `StepAction` variant. Since the spec is now read at runtime from a prior step's outcome, `EnsureEnvironmentImage` should no longer carry the spec inline. Instead, the resolver for `EnsureEnvironmentImage` should read the spec from `prior` outcomes.

This means changing `StepAction::EnsureEnvironmentImage { spec }` to `StepAction::EnsureEnvironmentImage` (no fields) — it reads the spec from `prior`. Check the resolver code at `executor.rs:897-902` and update it to extract the spec from `prior` instead of from the action.

Similarly, the plan builder no longer needs the spec at plan-build time.

- [ ] **Step 2: Add build_existing_environment_checkout_plan**

```rust
fn build_existing_environment_checkout_plan(
    env_id: EnvironmentId,
    target: CheckoutTarget,
    issue_ids: Vec<(String, String)>,
    target_host: HostName,
    local_host: HostName,
) -> StepPlan {
    let (branch, create_branch, intent) = match target {
        CheckoutTarget::Branch(branch) => (branch, false, CheckoutIntent::ExistingBranch),
        CheckoutTarget::FreshBranch(branch) => (branch, true, CheckoutIntent::FreshBranch),
    };

    let host_context = StepExecutionContext::Host(target_host.clone());
    let env_context = StepExecutionContext::Environment(target_host.clone(), env_id.clone());

    let workspace_label = if target_host == local_host { branch.clone() } else { format!("{branch}@{target_host}") };

    let mut steps = vec![
        Step {
            description: format!("Discover providers in environment {env_id}"),
            host: host_context,
            action: StepAction::DiscoverEnvironmentProviders { env_id },
        },
        Step {
            description: format!("Create checkout for branch {branch}"),
            host: env_context.clone(),
            action: StepAction::CreateCheckout { branch: branch.clone(), create_branch, intent, issue_ids },
        },
        Step {
            description: format!("Prepare workspace for {workspace_label}"),
            host: env_context,
            action: StepAction::PrepareWorkspace { checkout_path: None, label: workspace_label },
        },
        Step {
            description: "Attach workspace".to_string(),
            host: StepExecutionContext::Host(local_host),
            action: StepAction::AttachWorkspace,
        },
    ];

    StepPlan::new(steps)
}
```

- [ ] **Step 3: Implement ReadEnvironmentSpec resolver**

In the `ExecutorStepResolver::resolve` match in `crates/flotilla-core/src/executor.rs`, add:

```rust
StepAction::ReadEnvironmentSpec => {
    let output = self
        .runner
        .run(
            "git",
            &["show", "HEAD:.flotilla/environment.yaml"],
            self.repo.root.as_path(),
            &crate::providers::ChannelLabel::Noop,
        )
        .await
        .map_err(|e| format!("failed to read .flotilla/environment.yaml: {e}"))?;
    let spec: flotilla_protocol::EnvironmentSpec =
        serde_yaml::from_str(&output).map_err(|e| format!("invalid environment.yaml: {e}"))?;
    Ok(StepOutcome::Produced(CommandValue::EnvironmentSpecRead { spec }))
}
```

- [ ] **Step 4: Update EnsureEnvironmentImage resolver to read spec from prior**

In the `EnsureEnvironmentImage` match arm, change it to extract the spec from prior outcomes:

```rust
StepAction::EnsureEnvironmentImage { .. } => {
    let spec = prior
        .iter()
        .find_map(|o| match o {
            StepOutcome::Produced(CommandValue::EnvironmentSpecRead { spec }) => Some(spec.clone()),
            _ => None,
        })
        .ok_or_else(|| "environment spec not produced by prior ReadEnvironmentSpec step".to_string())?;
    // ... rest of existing logic using spec
}
```

- [ ] **Step 5: Update CreateEnvironment resolver to resolve token env vars**

In the `CreateEnvironment` match arm, after resolving the image from prior, also extract token env var names from the spec and resolve them from the host environment:

```rust
let spec = prior
    .iter()
    .find_map(|o| match o {
        StepOutcome::Produced(CommandValue::EnvironmentSpecRead { spec }) => Some(spec.clone()),
        _ => None,
    });

let tokens: Vec<(String, String)> = spec
    .map(|s| {
        s.token_env_vars
            .iter()
            .filter_map(|name| std::env::var(name).ok().map(|val| (name.clone(), val)))
            .collect()
    })
    .unwrap_or_default();

let opts = crate::providers::environment::CreateOpts {
    tokens,
    reference_repo,
    daemon_socket_path: daemon_socket,
    working_directory: None,
};
```

Note: using `std::env::var` is acceptable here since this runs on the daemon host (the step resolver's process), not inside a container. The existing `EnvVars` trait abstraction is for provider factories; the step resolver runs in the daemon process directly.

- [ ] **Step 6: Update StepAction::EnsureEnvironmentImage to remove spec field**

In `crates/flotilla-protocol/src/step.rs`, change:

```rust
    EnsureEnvironmentImage {
        spec: crate::EnvironmentSpec,
    },
```

to:

```rust
    EnsureEnvironmentImage,
```

Fix any compilation errors from this change (the plan builder no longer passes a spec, and the resolver reads it from prior).

- [ ] **Step 7: Run tests**

Run: `cargo test --workspace --locked`
Expected: passes. Existing environment tests may need updating if they constructed `EnsureEnvironmentImage` with a spec.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: add ReadEnvironmentSpec step, resolve spec and tokens at runtime"
```

---

### Task 6: Replace UiState.target_host with provisioning_target

**Files:**
- Modify: `crates/flotilla-tui/src/app/ui_state.rs:107-154` (UiState struct, constructor, cycle method)
- Modify: `crates/flotilla-tui/src/widgets/mod.rs:95-108` (WidgetContext struct)
- Modify: `crates/flotilla-tui/src/app/test_support.rs:191-234` (TestWidgetHarness)
- Modify: `crates/flotilla-tui/src/app/mod.rs:296-298,399-412,533-544,603-612` (App methods)

- [ ] **Step 1: Update UiState**

In `crates/flotilla-tui/src/app/ui_state.rs`, change the struct field (line 109):

```rust
pub provisioning_target: ProvisioningTarget,
```

Update `UiState::new` (line 121-132) — use `HostName::local()` as default:

```rust
pub fn new(_repo_ids: &[RepoIdentity]) -> Self {
    Self {
        is_config: false,
        provisioning_target: ProvisioningTarget::Host(HostName::local()),
        // ... rest unchanged
    }
}
```

Remove `cycle_target_host` method (lines 149-154) — this was a temporary hack, no longer needed with the command palette approach.

Add the import for `ProvisioningTarget` at the top of the file.

- [ ] **Step 2: Update UiState tests**

Replace the `cycle_target_host` tests and the `ui_state_defaults_target_host_to_local` test:

```rust
#[test]
fn ui_state_defaults_to_local_provisioning_target() {
    let state = UiState::new(&[]);
    assert_eq!(state.provisioning_target, ProvisioningTarget::Host(HostName::local()));
}
```

Remove `cycle_target_host_advances_through_known_peers_and_back_to_local` and `cycle_target_host_ignores_empty_peer_list` tests.

- [ ] **Step 3: Update WidgetContext**

In `crates/flotilla-tui/src/widgets/mod.rs`, change line 100:

```rust
pub provisioning_target: &'a ProvisioningTarget,
```

Remove `target_host` field. Add import for `ProvisioningTarget`.

- [ ] **Step 4: Update TestWidgetHarness**

In `crates/flotilla-tui/src/app/test_support.rs`:

Change field (line 197) from `pub target_host: Option<HostName>` to `pub provisioning_target: ProvisioningTarget`.

Update constructor (line 212) from `target_host: app.ui.target_host` to `provisioning_target: app.ui.provisioning_target.clone()`.

Update `ctx()` method (line 225) from `target_host: self.target_host.as_ref()` to `provisioning_target: &self.provisioning_target`.

- [ ] **Step 5: Update App methods**

In `crates/flotilla-tui/src/app/mod.rs`:

Update `targeted_command` (lines 399-401):

```rust
pub fn targeted_command(&self, action: CommandAction) -> Command {
    let target = &self.ui.provisioning_target;
    Command {
        host: Some(target.host().clone()),
        provisioning_target: Some(target.clone()),
        context_repo: None,
        action,
    }
}
```

Update `targeted_repo_command` (lines 403-409) similarly.

Update `build_widget_context` (lines 535-548) — change `target_host: self.ui.target_host.as_ref()` to `provisioning_target: &self.ui.provisioning_target`.

Update `SetTarget` handler (lines 607-612):

```rust
AppAction::SetTarget(name) => {
    match name.parse::<ProvisioningTarget>() {
        Ok(target) => self.ui.provisioning_target = target,
        Err(e) => tracing::warn!(%name, %e, "invalid provisioning target"),
    }
}
```

Remove `CycleHost` handler (lines 603-606) or update it to cycle provisioning targets if still bound.

- [ ] **Step 6: Fix all remaining compilation errors**

Search for `target_host` across `crates/flotilla-tui/` and fix each reference. Key locations:

- `crates/flotilla-tui/src/widgets/branch_input.rs:53` — change `host: ctx.target_host.cloned()` to `host: Some(ctx.provisioning_target.host().clone())` and `provisioning_target: Some(ctx.provisioning_target.clone())`
- `crates/flotilla-tui/src/widgets/status_bar_widget.rs:281-284` — change the host label logic
- `crates/flotilla-tui/src/app/intent.rs` — any references to `target_host`
- `crates/flotilla-tui/src/widgets/command_palette.rs` — SetTarget handling
- `crates/flotilla-tui/tests/support/mod.rs` — test harness
- Peer disconnect handling in `crates/flotilla-tui/src/app/mod.rs` — where `target_host` is cleared on disconnect, now reset `provisioning_target` to `Host(local)`

- [ ] **Step 7: Run tests**

Run: `cargo test --workspace --locked`
Expected: passes.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: replace UiState.target_host with ProvisioningTarget"
```

---

### Task 7: Status bar display

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/status_bar_widget.rs:267-290`

- [ ] **Step 1: Update normal_mode_indicators**

In `crates/flotilla-tui/src/widgets/status_bar_widget.rs`, change the host label logic (lines 281-284):

```rust
    let host_label = ui.provisioning_target.to_string();
```

This uses the `Display` impl on `ProvisioningTarget` which produces `@local`, `+docker@feta`, etc.

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: passes. Snapshot tests may need updating — investigate any failures before accepting.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: display provisioning target in status bar"
```

---

### Task 8: Branch input widget popup title

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/branch_input.rs:81`

- [ ] **Step 1: Update popup title to show provisioning target**

In `crates/flotilla-tui/src/widgets/branch_input.rs`, the render method (line 81) currently has:

```rust
let (_outer, inner) = ui_helpers::render_popup_frame(frame, area, 50, 20, " New Branch ", theme.block_style());
```

The widget doesn't currently have access to the provisioning target at render time. The `RenderContext` (used in `render`) is different from `WidgetContext` (used in `handle_action`).

Check what `RenderContext` has available. If it includes the provisioning target (or can be extended to), use it:

```rust
let title = format!(" New Branch {} ", ctx.provisioning_target);
let (_outer, inner) = ui_helpers::render_popup_frame(frame, area, 50, 20, &title, theme.block_style());
```

If `RenderContext` doesn't have it, store the provisioning target as a field on `BranchInputWidget` (set during construction or via a method), and use `self.target_label` in render.

- [ ] **Step 2: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: show provisioning target in branch input popup title"
```

---

### Task 9: Command palette target completions

**Files:**
- Modify: `crates/flotilla-tui/src/palette.rs:76-89` (palette_local_completions)
- Modify: `crates/flotilla-tui/src/widgets/command_palette.rs` (if needed for dynamic completions)

- [ ] **Step 1: Understand current completion mechanism**

The `palette_local_completions` function in `crates/flotilla-tui/src/palette.rs:79-89` currently only provides static completions for `layout`. The `target` command has no completions.

The completions need to be dynamic (built from host summaries at runtime). Check how the command palette widget calls `palette_local_completions` and whether it can receive dynamic completions from a context.

- [ ] **Step 2: Add dynamic target completions**

The approach depends on the existing completion architecture. The completions should be built from:

```rust
fn target_completions(model: &TuiModel) -> Vec<String> {
    let mut completions = Vec::new();
    for (host_name, host_info) in &model.hosts {
        // Bare host
        completions.push(format!("@{host_name}"));
        // New environment providers
        if let Some(summary) = &host_info.summary {
            for provider in &summary.providers {
                if provider.category == "environment" && provider.healthy {
                    completions.push(format!("+{}@{host_name}", provider.name.to_lowercase()));
                }
            }
            // Existing environments
            for env in &summary.environments {
                completions.push(format!("={}@{host_name}", env.id));
            }
        }
    }
    completions
}
```

Wire this into the completion mechanism so that typing `target ` in the palette shows these options.

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add dynamic target completions from host capabilities"
```

---

### Task 10: End-to-end integration test

**Files:**
- Modify or create: `crates/flotilla-core/tests/` or `crates/flotilla-core/src/executor/tests.rs`

- [ ] **Step 1: Write a test for the new environment checkout plan shape**

Test that `build_plan` with a `NewEnvironment` provisioning target produces the 7-step plan:

```rust
#[tokio::test]
async fn build_plan_with_new_environment_target_produces_seven_step_plan() {
    // Construct a Command with provisioning_target: Some(NewEnvironment { ... })
    // Call build_plan
    // Assert step count is 7
    // Assert step 0 is ReadEnvironmentSpec
    // Assert step 1 is EnsureEnvironmentImage
    // Assert step 2 is CreateEnvironment
    // Assert step 3 is DiscoverEnvironmentProviders
    // Assert step 4 is CreateCheckout
    // Assert step 5 is PrepareWorkspace
    // Assert step 6 is AttachWorkspace
}
```

- [ ] **Step 2: Write a test for existing environment checkout plan shape**

```rust
#[tokio::test]
async fn build_plan_with_existing_environment_target_produces_four_step_plan() {
    // Construct a Command with provisioning_target: Some(ExistingEnvironment { ... })
    // Assert step count is 4
    // Assert step 0 is DiscoverEnvironmentProviders
    // Assert step 1 is CreateCheckout
    // Assert step 2 is PrepareWorkspace
    // Assert step 3 is AttachWorkspace
}
```

- [ ] **Step 3: Write a test for bare host plan unchanged**

```rust
#[tokio::test]
async fn build_plan_with_host_target_produces_standard_checkout_plan() {
    // Construct a Command with provisioning_target: Some(Host(...)) or None
    // Assert the plan matches the existing checkout plan shape
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test --workspace --locked`
Expected: all pass.

- [ ] **Step 5: Run CI checks**

Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
Expected: all clean.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "test: integration tests for environment checkout plan shapes"
```

---

### Task 11: Add serde_yaml dependency (if not already present)

**Note:** This task should be completed before Task 5 if serde_yaml is not already in flotilla-core's dependencies, since the ReadEnvironmentSpec resolver uses `serde_yaml::from_str`.

**Files:**
- Modify: `crates/flotilla-core/Cargo.toml`

- [ ] **Step 1: Check if serde_yaml is already a dependency**

Run: `grep serde_yaml crates/flotilla-core/Cargo.toml`

If not present, add it:

```toml
serde_yaml = "0.9"
```

If already present, skip this task.

- [ ] **Step 2: Run cargo build**

Run: `cargo build -p flotilla-core --locked`

If `--locked` fails because the lockfile needs updating, run `cargo build -p flotilla-core` to update it, then verify with `cargo test --workspace --locked`.

- [ ] **Step 3: Commit (if changed)**

```bash
git add crates/flotilla-core/Cargo.toml Cargo.lock
git commit -m "chore: add serde_yaml dependency for environment.yaml parsing"
```
