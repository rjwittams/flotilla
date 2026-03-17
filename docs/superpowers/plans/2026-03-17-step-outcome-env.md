# Step Outcome Environment — Fix Result Handling Regression

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the result-handling regression where workspace step failure overwrites the CheckoutCreated result. Replace shared mutable slots with an outcome environment that flows between steps.

**Architecture:** Each step receives the outcomes of all prior steps as a `Vec<StepOutcome>`. The stepper collects outcomes and determines the plan's final `CommandResult` from them. A later step failure does not overwrite an earlier step's meaningful result. The shared `Arc<Mutex<Option<PathBuf>>>` inter-step communication pattern is replaced by reading prior outcomes.

**Tech Stack:** Rust, flotilla-core

---

## The Regression

`run_step_plan` keeps a single `final_result` that `CompletedWith` overwrites, and returns `CommandResult::Error` immediately on any step failure. If step 1 produces `CheckoutCreated` and step 3 (workspace) fails, the client gets `Error` — losing the fact that the checkout exists on disk.

## Changes

### step.rs

1. **Derive `Clone` on `StepOutcome`** (requires `CommandResult` to be `Clone` — it already is).

2. **Change closure signature** to receive prior outcomes:
   ```rust
   pub type StepFuture = Pin<Box<dyn Future<Output = Result<StepOutcome, String>> + Send>>;

   // Closure now receives prior outcomes
   Closure(Box<dyn FnOnce(Vec<StepOutcome>) -> StepFuture + Send>)
   ```

3. **Change `StepResolver::resolve`** to receive prior outcomes:
   ```rust
   async fn resolve(&self, description: &str, action: StepAction, prior: &[StepOutcome]) -> Result<StepOutcome, String>;
   ```

4. **Remove `checkout_path` field from `StepAction::CreateWorkspaceForCheckout`** — it reads the path from prior outcomes instead:
   ```rust
   CreateWorkspaceForCheckout { label: String }
   ```

5. **Update `run_step_plan`:**
   - Collect outcomes: `let mut outcomes: Vec<StepOutcome> = Vec::new();`
   - Pass `outcomes.clone()` to closures, `&outcomes` to resolver
   - After all steps: pick the last `CompletedWith` result from outcomes
   - On step error: if a prior outcome had `CompletedWith`, return that result (the error is already reported via the StepFailed event). If no prior meaningful result, return the error.

### executor.rs

1. **Remove `checkout_path_slot`** from `build_create_checkout_plan`. Step 1's closure captures the pre-existing checkout path as a plain `Option<PathBuf>` for its "skip if exists" check.

2. **Step 1** (create checkout): no longer writes to a shared slot. Just returns `CompletedWith(CheckoutCreated { branch, path })` as before.

3. **Step 2** (issue linking): reads checkout path from prior outcomes:
   ```rust
   Box::new(move |prior: Vec<StepOutcome>| {
       Box::pin(async move {
           let checkout_path = prior.iter().find_map(|o| match o {
               StepOutcome::CompletedWith(CommandResult::CheckoutCreated { path, .. }) => Some(path.clone()),
               _ => None,
           });
           if let Some(path) = checkout_path {
               write_branch_issue_links(&path, &branch, &issue_ids, &*runner).await;
           }
           Ok(StepOutcome::Completed)
       })
   })
   ```
   Note: `write_branch_issue_links` currently takes `repo_root` — check if it should take checkout path instead.

4. **`CreateWorkspaceForCheckout` action** — no `checkout_path` field. Resolver reads from prior outcomes.

5. **`ExecutorStepResolver::resolve`** — updated to receive `&[StepOutcome]`, extracts `CheckoutCreated.path` from it.

6. **All other closure steps** (teleport, remove checkout, archive, branch name) — update signature to accept `Vec<StepOutcome>` even if they ignore it (`_prior`).

7. **Update all tests** that construct steps or call the resolver.

---

## Tasks

### Task 1: Update step infrastructure — outcome env and error handling

**Files:**
- Modify: `crates/flotilla-core/src/step.rs`

- [ ] **Step 1: Write failing test — prior step result preserved on later failure**

```rust
#[tokio::test]
async fn later_failure_preserves_earlier_completed_with() {
    let (cancel, tx) = setup();
    let plan = StepPlan::new(vec![
        make_step("step-a", Ok(StepOutcome::CompletedWith(CommandResult::CheckoutCreated {
            branch: "feat/x".into(),
            path: PathBuf::from("/repo/wt-feat-x"),
        }))),
        make_step("step-b", Err("workspace failed".into())),
    ]);

    let result = run_step_plan(plan, 1, HostName::local(),
        RepoIdentity { authority: "github.com".into(), path: "owner/repo".into() },
        PathBuf::from("/repo"), cancel, tx, None,
    ).await;
    // Should preserve CheckoutCreated, not return Error
    assert_eq!(result, CommandResult::CheckoutCreated {
        branch: "feat/x".into(), path: PathBuf::from("/repo/wt-feat-x"),
    });
}
```

- [ ] **Step 2: Run test — verify it fails**

Run: `cargo test -p flotilla-core --lib step::tests::later_failure_preserves_earlier_completed_with`
Expected: FAIL — currently returns `Error { message: "workspace failed" }`.

- [ ] **Step 3: Implement changes**

In `step.rs`:

a) Derive `Clone` on `StepOutcome`.

b) Change `StepAction::Closure` to take `Vec<StepOutcome>`:
```rust
Closure(Box<dyn FnOnce(Vec<StepOutcome>) -> StepFuture + Send>)
```

c) Update `StepResolver::resolve` signature:
```rust
async fn resolve(&self, description: &str, action: StepAction, prior: &[StepOutcome]) -> Result<StepOutcome, String>;
```

d) Remove `checkout_path` from `CreateWorkspaceForCheckout`:
```rust
CreateWorkspaceForCheckout { label: String }
```

e) Update `run_step_plan`:
```rust
let mut outcomes: Vec<StepOutcome> = Vec::new();

for (i, step) in plan.steps.into_iter().enumerate() {
    // ... cancellation check, emit Started ...

    let outcome = match step.action {
        StepAction::Closure(f) => f(outcomes.clone()).await,
        symbolic => match resolver {
            Some(r) => r.resolve(&step.description, symbolic, &outcomes).await,
            None => Err(format!("no resolver for symbolic step: {}", step.description)),
        },
    };

    // ... cancellation check ...

    match outcome {
        Ok(ref o) => {
            outcomes.push(o.clone());
            // ... emit Succeeded/Skipped event based on variant ...
        }
        Err(e) => {
            // ... emit Failed event ...
            // If a prior step produced a meaningful result, preserve it
            let prior_result = outcomes.iter().rev().find_map(|o| match o {
                StepOutcome::CompletedWith(r) => Some(r.clone()),
                _ => None,
            });
            return prior_result.unwrap_or(CommandResult::Error { message: e });
        }
    }
}

// Determine final result from outcomes
outcomes.into_iter().rev().find_map(|o| match o {
    StepOutcome::CompletedWith(r) => Some(r),
    _ => None,
}).unwrap_or(CommandResult::Ok)
```

f) Update `make_step` — closure takes `_prior: Vec<StepOutcome>`:
```rust
fn make_step(desc: &str, outcome: Result<StepOutcome, String>) -> Step {
    let outcome = Arc::new(tokio::sync::Mutex::new(Some(outcome)));
    Step {
        description: desc.to_string(),
        host: StepHost::Local,
        action: StepAction::Closure(Box::new(move |_prior| {
            let outcome = Arc::clone(&outcome);
            Box::pin(async move { outcome.lock().await.take().expect("step called twice") })
        })),
    }
}
```

- [ ] **Step 4: Update existing test `step_failure_stops_execution`**

This test asserts that step failure returns `Error`. After the change, this still holds when NO prior step had `CompletedWith`:
```rust
// step-a: Completed (no CompletedWith), step-b: Error
// → no prior meaningful result → still returns Error
```
So this test should still pass. Verify.

- [ ] **Step 5: Run all step tests**

Run: `cargo test -p flotilla-core --lib step`
Expected: all pass.

- [ ] **Step 6: Commit**

```
git commit -m "fix: preserve prior step results on later step failure"
```

---

### Task 2: Update executor — remove shared slot, use outcome env

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`
- Modify: `crates/flotilla-core/src/in_process.rs` (if resolver signature changed)

- [ ] **Step 1: Update all closure steps to accept `Vec<StepOutcome>`**

Every `StepAction::Closure(Box::new(move || { ... }))` becomes `StepAction::Closure(Box::new(move |_prior| { ... }))`. There are ~9 sites across the plan builders.

For the two closures in `build_create_checkout_plan` that use the shared slot:

**Step 1 (create checkout):** Replace `slot` with a captured `existing_checkout_path: Option<PathBuf>`:
```rust
let existing_checkout_path: Option<PathBuf> = providers_data.checkouts.iter().find_map(|(hp, co)| {
    if hp.host == local_host && co.branch == branch { Some(hp.path.clone()) } else { None }
});

// In the closure:
StepAction::Closure(Box::new(move |_prior| {
    Box::pin(async move {
        validate_checkout_target(&repo_root, &branch, intent, &*runner).await?;
        if existing_checkout_path.is_some() {
            if matches!(intent, CheckoutIntent::FreshBranch) {
                return Err(format!("branch already exists: {branch}"));
            }
            return Ok(StepOutcome::Skipped);
        }
        let cm = registry.checkout_managers.preferred().cloned()
            .ok_or_else(|| "No checkout manager available".to_string())?;
        let (path, _checkout) = cm.create_checkout(&repo_root, &branch, create_branch).await?;
        info!(checkout_path = %path.display(), "created checkout");
        Ok(StepOutcome::CompletedWith(CommandResult::CheckoutCreated { branch, path }))
    })
}))
```

**Step 2 (issue linking):** Read checkout path from `prior`:
```rust
StepAction::Closure(Box::new(move |prior| {
    Box::pin(async move {
        let checkout_root = prior.iter().find_map(|o| match o {
            StepOutcome::CompletedWith(CommandResult::CheckoutCreated { path, .. }) => Some(path.clone()),
            _ => None,
        }).unwrap_or(repo_root.clone());
        write_branch_issue_links(&checkout_root, &branch, &issue_ids, &*runner).await;
        Ok(StepOutcome::Completed)
    })
}))
```

Note: check what `write_branch_issue_links` actually does with the path — it might be the repo root, not the checkout path. Read the function to confirm and adjust.

**Workspace step:** Remove `checkout_path` field:
```rust
StepAction::CreateWorkspaceForCheckout { label: branch.clone() }
```

Delete the `checkout_path_slot` variable entirely.

- [ ] **Step 2: Update `ExecutorStepResolver::resolve`**

Add `prior: &[StepOutcome]` parameter. Extract checkout path from prior outcomes:
```rust
StepAction::CreateWorkspaceForCheckout { label } => {
    let path = prior.iter().find_map(|o| match o {
        StepOutcome::CompletedWith(CommandResult::CheckoutCreated { path, .. }) => Some(path.clone()),
        _ => None,
    });
    match path {
        Some(p) => create_workspace_for_checkout_impl(&p, &label, ...).await,
        None => Ok(StepOutcome::Skipped),
    }
}
```

- [ ] **Step 3: Update tests**

Fix all tests that construct `StepAction::CreateWorkspaceForCheckout` — remove the `checkout_path` field. Fix resolver test calls to pass prior outcomes.

- [ ] **Step 4: Run all tests**

Run: `cargo test -p flotilla-core --locked`

- [ ] **Step 5: Run CI gates**

Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`

- [ ] **Step 6: Commit**

```
git commit -m "refactor: replace shared mutable slot with step outcome environment"
```
