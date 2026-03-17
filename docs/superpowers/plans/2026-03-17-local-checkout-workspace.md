# Local Checkout Workspace — Symbolic Multi-Host Step Plans

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix auto-workspace-after-checkout by moving domain logic from TUI into symbolic step plans that support per-step host targeting.

**Architecture:** Replace opaque closure-based steps with a symbolic `StepAction` enum that the stepper resolves and executes at runtime. Each step declares its target host. The stepper runs on the first-hop daemon and dispatches remote steps through the existing command-forwarding infrastructure. For the immediate bug fix, the checkout plan gains a final "create workspace" step that runs on the originating (local) host.

**Tech Stack:** Rust, flotilla-protocol (serde types), flotilla-core (executor, step runner)

---

## Context

### The Bug

When creating a checkout (local or remote), the TUI is supposed to auto-chain a workspace creation command. Two problems:

1. **Local checkouts — race condition:** The TUI auto-chains `CreateWorkspaceForCheckout` on receiving `CheckoutCreated`, but that command validates the checkout exists in `providers_data.checkouts`. The providers data hasn't been refreshed yet (the refresh is async), so it fails with "checkout not found".

2. **Remote checkouts — no auto-chain path:** The TUI only auto-chains workspace creation when `is_local == true`. Remote `CheckoutCreated` events are silently ignored.

### The Design Problem

The TUI currently contains domain orchestration logic (auto-chaining workspace creation after checkout) that belongs in the daemon. This creates fragile async round-trips: daemon creates checkout → event to TUI → TUI chains next command → back to daemon → command races with refresh.

### The Solution

Make checkout plans self-contained: include workspace creation as a step in the checkout plan itself. To support the remote case (checkout on host A, workspace on host B), extend the step infrastructure with per-step host targeting via symbolic step actions.

## File Structure

| File | Change | Responsibility |
|------|--------|---------------|
| `crates/flotilla-core/src/step.rs` | Modify | Add `StepAction` enum, `StepHost` enum, `StepResolver` trait, update `Step` struct, update `run_step_plan` |
| `crates/flotilla-core/src/executor.rs` | Modify | Add `ExecutorStepResolver`, extract workspace helper, add workspace step to checkout plan, update existing tests |
| `crates/flotilla-core/src/in_process.rs` | Modify | Construct resolver and pass to `run_step_plan` |
| `crates/flotilla-tui/src/app/mod.rs` | Modify | Remove auto-workspace chaining logic, update tests |

## Design Details

### StepAction Enum

The `CreateWorkspaceForCheckout` variant takes an `Arc<Mutex<Option<PathBuf>>>` for the checkout path — a shared slot populated by the prior checkout step. This is the same pattern used for data flow between existing steps (see `checkout_path_slot` in `build_create_checkout_plan`). At resolution time, the resolver locks the mutex and extracts the path; if `None`, the step is skipped.

```rust
pub enum StepAction {
    /// An opaque async closure (existing pattern).
    Closure(Box<dyn FnOnce() -> StepFuture + Send>),
    /// Create a workspace for a checkout path produced by a prior step.
    CreateWorkspaceForCheckout {
        /// Shared slot populated by the checkout creation step.
        /// If None at resolution time, the step is skipped.
        checkout_path: Arc<tokio::sync::Mutex<Option<PathBuf>>>,
        label: String,
    },
}
```

### StepHost Enum

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepHost {
    /// Run on the same host as the stepper (the daemon executing the plan).
    Local,
    /// Run on a specific named remote host.
    /// The stepper pauses, forwards the symbolic action to the named host
    /// for resolution and execution, then resumes with the result.
    Remote(HostName),
}
```

At plan-build time, the builder knows `local_host`. Steps that should run "here" use `StepHost::Local`. Steps that need another host use `StepHost::Remote(that_host)`. For this PR all steps are `Local`. Future remote checkout plans will use `Remote(originating_host)` for the workspace step.

To support this, `build_plan` gains an `originating_host: Option<HostName>` parameter — `None` for local commands, `Some(requester)` for forwarded commands. The forwarding code in `execute_forwarded_command` (server.rs) already has `requester_host` available. For this PR the parameter is threaded through but unused; the workspace step always uses `StepHost::Local`.

### StepResolver Trait

```rust
#[async_trait::async_trait]
pub trait StepResolver: Send + Sync {
    async fn resolve(&self, description: &str, action: StepAction) -> Result<StepOutcome, String>;
}
```

### Updated `run_step_plan` Signature

```rust
pub async fn run_step_plan(
    plan: StepPlan,
    command_id: u64,
    host: HostName,
    repo_identity: RepoIdentity,
    repo: PathBuf,
    cancel: CancellationToken,
    event_tx: broadcast::Sender<DaemonEvent>,
    resolver: Option<&dyn StepResolver>,
) -> CommandResult
```

### Updated `build_plan` Signature

The `originating_host` parameter is threaded through for future use. For local commands it is `None`. For commands forwarded from another host, it carries the requester's hostname so that plan builders can stamp `StepHost::Remote(originator)` on steps that need to run back on the presentation host.

```rust
pub async fn build_plan(
    cmd: Command,
    repo: RepoExecutionContext,
    registry: Arc<ProviderRegistry>,
    providers_data: Arc<ProviderData>,
    runner: Arc<dyn CommandRunner>,
    config_base: PathBuf,
    attachable_store: SharedAttachableStore,
    local_host: HostName,
    originating_host: Option<HostName>,  // NEW
) -> ExecutionPlan
```

---

## Tasks

### Task 1: Add `StepAction`, `StepHost`, `StepResolver` and update stepper

This task adds the full infrastructure in one commit: the types, the trait, and the updated `run_step_plan`. All existing call sites are updated (passing `None` for the resolver).

**Files:**
- Modify: `crates/flotilla-core/src/step.rs`
- Modify: `crates/flotilla-core/src/executor.rs` (only `Step` construction sites)
- Modify: `crates/flotilla-core/src/in_process.rs` (only `run_step_plan` call site)

- [ ] **Step 1: Write the failing test — closure step with new struct**

In `crates/flotilla-core/src/step.rs` tests, add:

```rust
#[tokio::test]
async fn closure_step_action_succeeds() {
    let (cancel, tx) = setup();
    let plan = StepPlan::new(vec![Step {
        description: "closure step".to_string(),
        host: StepHost::Local,
        action: StepAction::Closure(Box::new(|| Box::pin(async { Ok(StepOutcome::Completed) }))),
    }]);

    let result = run_step_plan(
        plan, 1, HostName::local(),
        RepoIdentity { authority: "github.com".into(), path: "owner/repo".into() },
        PathBuf::from("/repo"), cancel, tx, None,
    ).await;
    assert_eq!(result, CommandResult::Ok);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core --lib step::tests::closure_step_action_succeeds`
Expected: compile error — `StepAction`, `StepHost` don't exist yet.

- [ ] **Step 3: Implement StepAction, StepHost, StepResolver, and update Step**

In `crates/flotilla-core/src/step.rs`, add the types and trait shown in Design Details above. Update the `Step` struct. Add `use std::path::PathBuf; use std::sync::Arc; use flotilla_protocol::HostName;` to imports.

Update `run_step_plan` to:
- Accept `resolver: Option<&dyn StepResolver>` parameter
- Dispatch `StepAction::Closure` the same as before
- For other variants, delegate to the resolver (or error if no resolver):

```rust
let outcome = match step.action {
    StepAction::Closure(f) => f().await,
    symbolic => match resolver {
        Some(r) => r.resolve(&step.description, symbolic).await,
        None => Err(format!("no resolver for symbolic step: {}", step.description)),
    },
};
```

Update the `make_step` test helper to use the new struct:

```rust
fn make_step(desc: &str, outcome: Result<StepOutcome, String>) -> Step {
    let outcome = Arc::new(tokio::sync::Mutex::new(Some(outcome)));
    Step {
        description: desc.to_string(),
        host: StepHost::Local,
        action: StepAction::Closure(Box::new(move || {
            let outcome = Arc::clone(&outcome);
            Box::pin(async move { outcome.lock().await.take().expect("step called twice") })
        })),
    }
}
```

- [ ] **Step 4: Update all existing `run_step_plan` call sites**

In `crates/flotilla-core/src/step.rs` tests: add `None` as the last argument to all 7 existing `run_step_plan` calls (lines ~160, 189, 208, 241, 263, 290, 308).

In `crates/flotilla-core/src/in_process.rs` (~line 2237): add `None` as the last argument to the `run_step_plan` call. (This will change in Task 2.)

- [ ] **Step 5: Update all `Step { description, action }` construction sites in executor.rs**

Every `Step { description: ..., action: ... }` in `crates/flotilla-core/src/executor.rs` needs `host: StepHost::Local` added. Use `use crate::step::{StepAction, StepHost};` in the imports. Search for `Step {` — there are ~8 sites across the plan builder functions.

Wrap each existing `action: Box::new(...)` in `action: StepAction::Closure(Box::new(...))`.

- [ ] **Step 6: Add `originating_host` parameter to `build_plan`**

Update the `build_plan` signature to accept `originating_host: Option<HostName>`. Thread it through to the plan builder functions that will need it in the future (pass it to `build_create_checkout_plan`). For now it's unused — add `_originating_host` to suppress the warning, or store it for future use in the checkout plan builder signature.

Update all call sites of `build_plan`:
- `crates/flotilla-core/src/in_process.rs` (~line 2192): pass `None` (local commands don't have an originator — this will change when the daemon server passes the requester host)
- `crates/flotilla-core/src/executor.rs` test helper `run_build_plan`: pass `None`

- [ ] **Step 7: Run all tests**

Run: `cargo test -p flotilla-core --locked`
Expected: all pass — no behavioural change yet.

- [ ] **Step 8: Commit**

```bash
git add crates/flotilla-core/src/step.rs crates/flotilla-core/src/executor.rs crates/flotilla-core/src/in_process.rs
git commit -m "refactor: add StepAction, StepHost, and StepResolver to step infrastructure"
```

---

### Task 2: Implement ExecutorStepResolver and wire into InProcessDaemon

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`
- Modify: `crates/flotilla-core/src/in_process.rs`

- [ ] **Step 1: Write the failing test — symbolic step resolves workspace creation**

In `crates/flotilla-core/src/executor.rs` tests, add a helper and test:

```rust
fn repo_identity() -> flotilla_protocol::RepoIdentity {
    flotilla_protocol::RepoIdentity { authority: "github.com".into(), path: "owner/repo".into() }
}

#[tokio::test]
async fn executor_step_resolver_creates_workspace() {
    let ws_mgr = Arc::new(MockWorkspaceManager::succeeding());
    let mut registry = empty_registry();
    registry.workspace_managers.insert("cmux", desc("cmux"), Arc::clone(&ws_mgr) as Arc<dyn WorkspaceManager>);

    let config_base = config_base();
    let resolver = ExecutorStepResolver {
        repo: RepoExecutionContext { identity: repo_identity(), root: repo_root() },
        registry: Arc::new(registry),
        config_base: config_base.clone(),
        attachable_store: test_attachable_store(&config_base),
        local_host: local_host(),
    };

    let checkout_path = Arc::new(tokio::sync::Mutex::new(Some(PathBuf::from("/repo/wt-feat"))));
    let action = StepAction::CreateWorkspaceForCheckout { checkout_path, label: "feat".into() };
    let outcome = resolver.resolve("create workspace", action).await;
    assert!(outcome.is_ok(), "resolve should succeed: {outcome:?}");

    let calls = ws_mgr.calls.lock().await;
    assert!(calls.iter().any(|c| c.starts_with("create_workspace")), "should call create_workspace, got: {calls:?}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core --lib executor::tests::executor_step_resolver_creates_workspace`
Expected: compile error — `ExecutorStepResolver` doesn't exist yet.

- [ ] **Step 3: Extract workspace creation helper from execute()**

In `crates/flotilla-core/src/executor.rs`, extract the body of the `CreateWorkspaceForCheckout` arm in `execute()` into a shared helper. The helper **does not** validate against `providers_data` (the checkout was just created by a prior step — we know it exists).

```rust
/// Core workspace creation logic, shared by the step resolver and the
/// standalone CreateWorkspaceForCheckout command.
async fn create_workspace_for_checkout_impl(
    checkout_path: &Path,
    label: &str,
    repo: &RepoExecutionContext,
    registry: &ProviderRegistry,
    config_base: &Path,
    attachable_store: &SharedAttachableStore,
    local_host: &HostName,
) -> Result<StepOutcome, String> {
    if let Some((provider_name, ws_mgr)) = preferred_workspace_manager(registry) {
        if select_existing_workspace(ws_mgr.as_ref(), checkout_path).await {
            return Ok(StepOutcome::Completed);
        }
        let mut config = workspace_config(&repo.root, label, checkout_path, "claude", config_base);
        if let Some(tp) = registry.terminal_pools.preferred() {
            resolve_terminal_pool(&mut config, tp.as_ref()).await;
        }
        match ws_mgr.create_workspace(&config).await {
            Ok((ws_ref, _workspace)) => {
                persist_workspace_binding(attachable_store, provider_name, &ws_ref, local_host, checkout_path);
                Ok(StepOutcome::Completed)
            }
            Err(e) => Err(e),
        }
    } else {
        Ok(StepOutcome::Skipped)
    }
}
```

Update the `CreateWorkspaceForCheckout` arm in `execute()` to call this helper after its existing `providers_data` validation:

```rust
CommandAction::CreateWorkspaceForCheckout { checkout_path, label } => {
    let host_key = HostPath::new(local_host.clone(), checkout_path.clone());
    if !providers_data.checkouts.contains_key(&host_key) {
        return CommandResult::Error { message: format!("checkout not found: {}", checkout_path.display()) };
    }
    info!(%label, "entering workspace");
    match create_workspace_for_checkout_impl(
        &checkout_path, &label, repo, registry, config_base, attachable_store, local_host,
    ).await {
        Ok(_) => CommandResult::Ok,
        Err(e) => CommandResult::Error { message: e },
    }
}
```

- [ ] **Step 4: Implement ExecutorStepResolver**

```rust
pub(crate) struct ExecutorStepResolver {
    pub repo: RepoExecutionContext,
    pub registry: Arc<ProviderRegistry>,
    pub config_base: PathBuf,
    pub attachable_store: SharedAttachableStore,
    pub local_host: HostName,
}

#[async_trait::async_trait]
impl StepResolver for ExecutorStepResolver {
    async fn resolve(&self, _description: &str, action: StepAction) -> Result<StepOutcome, String> {
        match action {
            StepAction::Closure(_) => unreachable!("closures handled by stepper directly"),
            StepAction::CreateWorkspaceForCheckout { checkout_path, label } => {
                let path = checkout_path.lock().await.clone();
                match path {
                    Some(p) => {
                        create_workspace_for_checkout_impl(
                            &p, &label, &self.repo, &self.registry,
                            &self.config_base, &self.attachable_store, &self.local_host,
                        ).await
                    }
                    None => Ok(StepOutcome::Skipped),
                }
            }
        }
    }
}
```

Add `use crate::step::{StepAction, StepOutcome, StepResolver};` to the executor imports.

- [ ] **Step 5: Wire resolver into InProcessDaemon**

In `crates/flotilla-core/src/in_process.rs`, in the spawned task (~line 2191), **clone** the values the resolver needs **before** `build_plan` consumes them:

```rust
tokio::spawn(async move {
    // Clone values needed by both build_plan and the resolver
    let resolver_registry = Arc::clone(&registry);
    let resolver_config_base = config_base.clone();
    let resolver_attachable_store = attachable_store.clone();
    let resolver_local_host = local_host.clone();
    let resolver_repo = executor::RepoExecutionContext {
        identity: repo_identity.clone(),
        root: repo_path.clone(),
    };

    let plan = executor::build_plan(
        command, executor::RepoExecutionContext { identity: repo_identity.clone(), root: repo_path.clone() },
        registry, providers_data, runner, config_base, attachable_store, local_host, None,
    ).await;

    match plan {
        ExecutionPlan::Immediate(result) => { /* unchanged */ }
        ExecutionPlan::Steps(step_plan) => {
            // ... existing single-slot check unchanged ...

            let resolver = executor::ExecutorStepResolver {
                repo: resolver_repo,
                registry: resolver_registry,
                config_base: resolver_config_base,
                attachable_store: resolver_attachable_store,
                local_host: resolver_local_host,
            };
            let result = run_step_plan(
                step_plan, id, command_host.clone(), repo_identity.clone(), repo_path.clone(),
                token, event_tx.clone(), Some(&resolver),
            ).await;
            // ... rest unchanged ...
        }
    }
});
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p flotilla-core --locked`
Expected: all pass, including the new resolver test.

- [ ] **Step 7: Commit**

```bash
git add crates/flotilla-core/src/executor.rs crates/flotilla-core/src/in_process.rs
git commit -m "feat: implement ExecutorStepResolver for symbolic step actions"
```

---

### Task 3: Add workspace creation step to checkout plan

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`

- [ ] **Step 1: Write the failing test — checkout plan includes workspace step**

```rust
#[tokio::test]
async fn checkout_plan_includes_workspace_step() {
    let mut registry = empty_registry();
    registry.checkout_managers.insert("wt", desc("wt"), Arc::new(MockCheckoutManager::succeeding("feat-x", "/repo/wt-feat-x")));
    registry.workspace_managers.insert("cmux", desc("cmux"), Arc::new(MockWorkspaceManager::succeeding()));

    let plan = run_build_plan(fresh_checkout_action("feat-x"), registry, empty_data(), runner_ok()).await;

    match plan {
        ExecutionPlan::Steps(step_plan) => {
            assert_eq!(step_plan.steps.len(), 2, "expected checkout + workspace steps");
            assert_eq!(step_plan.steps[0].description, "Create checkout for branch feat-x");
            assert_eq!(step_plan.steps[1].description, "Create workspace");
        }
        ExecutionPlan::Immediate(_) => panic!("expected Steps"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core --lib executor::tests::checkout_plan_includes_workspace_step`
Expected: FAIL — plan has 1 step, not 2.

- [ ] **Step 3: Add workspace step to `build_create_checkout_plan`**

At the end of `build_create_checkout_plan` in `executor.rs`, after the existing steps, add:

```rust
// Final step: create workspace for the new checkout.
// The checkout path comes from the shared slot populated by step 1.
steps.push(Step {
    description: "Create workspace".to_string(),
    host: StepHost::Local,
    action: StepAction::CreateWorkspaceForCheckout {
        checkout_path: Arc::clone(&checkout_path_slot),
        label: branch.clone(),
    },
});
```

Also update the doc comment on `build_create_checkout_plan` (around line 107-115). Replace:

```rust
/// Workspace creation is NOT included here because this plan may execute on a
/// remote host.  The TUI handles workspace creation locally when it receives
/// `CheckoutCreated` from a local command.
```

With:

```rust
/// The final step creates a workspace for the new checkout. This is a symbolic
/// step resolved by the `ExecutorStepResolver` at execution time, so it has
/// access to the registry and config without needing pre-refreshed provider data.
```

- [ ] **Step 4: Update existing `build_plan` tests**

Update `build_plan_create_checkout_returns_steps` (~line 2875):
```rust
assert_eq!(step_plan.steps.len(), 2, "checkout + workspace steps");
assert_eq!(step_plan.steps[0].description, "Create checkout for branch feat-x");
assert_eq!(step_plan.steps[1].description, "Create workspace");
```

Update `build_plan_create_checkout_skips_existing` (~line 2893):
```rust
assert_eq!(step_plan.steps.len(), 2, "checkout + workspace steps");
assert_eq!(step_plan.steps[0].description, "Create checkout for branch feat-x");
assert_eq!(step_plan.steps[1].description, "Create workspace");
```

- [ ] **Step 5: Write end-to-end test — full plan creates workspace**

```rust
#[tokio::test]
async fn checkout_plan_end_to_end_creates_workspace() {
    let ws_mgr = Arc::new(MockWorkspaceManager::succeeding());
    let mut registry = ProviderRegistry::new();
    registry.checkout_managers.insert("wt", desc("wt"), Arc::new(MockCheckoutManager::succeeding("feat-x", "/repo/wt-feat-x")));
    registry.workspace_managers.insert("cmux", desc("cmux"), Arc::clone(&ws_mgr) as Arc<dyn WorkspaceManager>);
    let registry = Arc::new(registry);
    let runner = Arc::new(MockRunner::new(vec![Err("missing".into()), Err("missing".into())]));
    let cb = config_base();
    let attachable = test_attachable_store(&cb);
    let lh = local_host();
    let repo = RepoExecutionContext { identity: repo_identity(), root: repo_root() };

    let plan = build_plan(
        local_command(fresh_checkout_action("feat-x")),
        RepoExecutionContext { identity: repo_identity(), root: repo_root() },
        Arc::clone(&registry), Arc::new(empty_data()),
        runner, cb.clone(), attachable.clone(), lh.clone(), None,
    ).await;

    let (cancel, tx) = (CancellationToken::new(), broadcast::channel(64).0);
    let resolver = ExecutorStepResolver {
        repo, registry, config_base: cb, attachable_store: attachable, local_host: lh.clone(),
    };

    let result = match plan {
        ExecutionPlan::Steps(step_plan) => {
            run_step_plan(step_plan, 1, lh, repo_identity(), repo_root(), cancel, tx, Some(&resolver)).await
        }
        _ => panic!("expected steps"),
    };

    assert!(matches!(result, CommandResult::CheckoutCreated { .. }));
    let calls = ws_mgr.calls.lock().await;
    assert!(calls.iter().any(|c| c.starts_with("create_workspace")), "should create workspace, got: {calls:?}");
}
```

- [ ] **Step 6: Run all tests**

Run: `cargo test -p flotilla-core --locked`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/flotilla-core/src/executor.rs
git commit -m "feat: add workspace creation step to checkout plan"
```

---

### Task 4: Remove auto-workspace chaining from TUI

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs`

- [ ] **Step 1: Remove the auto-workspace chaining code**

In `crates/flotilla-tui/src/app/mod.rs`, in the `CommandFinished` handler (~lines 414-429), remove the `is_local` check and `auto_workspace` chaining. The block:

```rust
// Auto-create workspace for local checkouts. Remote checkouts
// go through the PrepareTerminal → TerminalPrepared flow instead.
let is_local = self.model.my_host().is_some_and(|my| *my == host);
let auto_workspace = match (&result, is_local) {
    (CommandResult::CheckoutCreated { branch, path }, true) => Some((path.clone(), branch.clone())),
    _ => None,
};
executor::handle_result(result, self);
if let Some((checkout_path, label)) = auto_workspace {
    self.proto_commands.push(
        self.repo_command_for_identity(repo_identity, CommandAction::CreateWorkspaceForCheckout {
            checkout_path,
            label,
        }),
    );
}
```

Should become just:

```rust
executor::handle_result(result, self);
```

- [ ] **Step 2: Update tests**

Replace `local_checkout_created_queues_workspace_creation` with:

```rust
#[test]
fn local_checkout_created_does_not_queue_workspace() {
    let mut app = stub_app();
    insert_local_host(&mut app.model, "my-desktop");
    let repo_identity = app.model.repo_order[0].clone();
    let repo_path = app.model.repos[&repo_identity].path.clone();

    app.in_flight.insert(42, InFlightCommand {
        repo_identity: repo_identity.clone(),
        repo: repo_path.clone(),
        description: "test".into(),
    });

    app.handle_daemon_event(DaemonEvent::CommandFinished {
        command_id: 42,
        host: HostName::new("my-desktop"),
        repo_identity,
        repo: repo_path,
        result: CommandResult::CheckoutCreated { branch: "feat".into(), path: PathBuf::from("/tmp/repo/wt-feat") },
    });

    assert!(app.proto_commands.take_next().is_none(), "workspace creation is now handled by checkout plan, not TUI");
}
```

The existing `remote_checkout_created_does_not_queue_workspace` test should still pass unchanged.

- [ ] **Step 3: Run all TUI tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-tui/src/app/mod.rs
git commit -m "fix: remove auto-workspace chaining from TUI, now handled by checkout plan"
```

---

### Task 5: CI gates

**Files:** None (verification only)

- [ ] **Step 1: Format check**

Run: `cargo +nightly-2026-03-12 fmt --check`
Expected: no issues. If issues, run `cargo +nightly-2026-03-12 fmt` and commit.

- [ ] **Step 2: Clippy**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: no warnings. Watch for unused variables, unused imports after the refactor.

- [ ] **Step 3: Full test suite**

Run: `cargo test --workspace --locked`
Expected: all pass.

- [ ] **Step 4: Fix any issues and commit**

---

## Future Work (not in this PR)

### Remote Step Routing

When the stepper encounters `StepHost::Remote(host_name)`, it needs to:

1. Package the symbolic `StepAction` into a message and forward it to `host_name` via the peer mesh
2. The remote daemon resolves the action using its own `ExecutorStepResolver`
3. The result flows back to the stepper, which continues with the next step

This requires:
- A new routed peer message type (e.g. `StepForwardRequest` / `StepForwardResponse`)
- The stepper to suspend on remote steps and await the response
- The daemon server's `execute_forwarded_command` to pass `requester_host` as `originating_host` into `build_plan`, so the plan builder can stamp `StepHost::Remote(requester)` on the workspace step

### Remote Checkout Auto-Workspace

With `StepHost::Remote` routing and `originating_host` threaded through, the checkout plan for a remote checkout (user on host A, checkout on host B) would be:

1. Step 1 (`Local` on B): Create checkout on remote
2. Step 2 (`Local` on B): PrepareTerminalForCheckout on remote (wraps through shpool)
3. Step 3 (`Remote(host_A)`): CreateWorkspaceFromPreparedTerminal (wraps SSH, creates cmux on host A)

The plan builder uses `originating_host.unwrap_or(local_host)` to decide the workspace step's host.

### Template Command Sourcing

The TUI's `local_template_commands()` call in the `CreateWorkspace` intent can be removed once the daemon always handles template resolution. The `PrepareTerminalForCheckout` command already has a fallback path that reads the template locally.
