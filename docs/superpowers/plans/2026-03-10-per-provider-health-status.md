# Per-Provider Health Status Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix health status to track per-provider instead of per-category, so that a single failing provider (e.g. Cursor) doesn't mark all providers in that category (e.g. Claude) as unhealthy.

**Architecture:** Change the health key from category-only (`"cloud_agent" → bool`) to category+provider (`"cloud_agent" → {"Claude" → true, "Cursor" → false}`). Add a `provider` field to `RefreshError` so errors can be attributed to individual providers. The TUI already stores status per `(path, category, provider_name)` — this change makes the data feeding it actually granular.

**Tech Stack:** Rust, serde, ratatui, tokio

**Issue:** #195

---

## File Map

| File | Action | Role |
|------|--------|------|
| `crates/flotilla-core/src/data.rs` | Modify:15-19 | Add `provider` field to `RefreshError`, update `DataStore.provider_health` type |
| `crates/flotilla-core/src/refresh.rs` | Modify:107-306 | Update `refresh_providers` to capture provider names, rewrite `compute_provider_health` per-provider |
| `crates/flotilla-protocol/src/snapshot.rs` | Modify:50,61,71-75 | Change `provider_health` to nested HashMap, add `provider` to `ProviderError` |
| `crates/flotilla-protocol/src/delta.rs` | Modify:66-69 | Add `category` field to `Change::ProviderHealth` |
| `crates/flotilla-core/src/delta.rs` | Modify:579-584 | Update `provider_error` test helper with `provider` field |
| `crates/flotilla-core/src/convert.rs` | Modify:69-96 | Update health and error conversions for new types |
| `crates/flotilla-core/src/in_process.rs` | Modify:78-102,148-226,340-400,745-820,940-978,1036-1045 | Update health diffing, snapshot building, and RepoInfo construction |
| `crates/flotilla-tui/src/app/mod.rs` | Modify:53-65,80-108,220-251,283-346 | Update `TuiRepoModel`, simplify health→status propagation to 1:1 |
| `crates/flotilla-tui/src/cli.rs` | Modify:22-26 | Update health display for nested map |
| `crates/flotilla-tui/src/app/test_builders.rs` | Modify:118-126 | Update `repo_info` builder |
| `crates/flotilla-tui/src/app/test_support.rs` | Modify:89 | Update test default |
| `crates/flotilla-client/src/lib.rs` | Modify:706 | Update placeholder snapshot |

---

## Chunk 1: Type Foundation Changes

### Task 1: Add `provider` field to `RefreshError`

**Files:**
- Modify: `crates/flotilla-core/src/data.rs:15-25`

- [ ] **Step 1: Update `RefreshError` struct**

In `crates/flotilla-core/src/data.rs`, add a `provider` field:

```rust
#[derive(Debug, Clone)]
pub struct RefreshError {
    pub category: &'static str,
    pub provider: String,
    pub message: String,
}

impl fmt::Display for RefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}: {}", self.category, self.provider, self.message)
    }
}
```

- [ ] **Step 2: Update `DataStore.provider_health` type**

In the same file, change the `provider_health` field in `DataStore` (~line 208):

```rust
// Before:
pub provider_health: HashMap<&'static str, bool>,

// After:
pub provider_health: HashMap<(&'static str, String), bool>,
```

- [ ] **Step 3: Fix all compilation errors from `RefreshError` changes**

Every place that constructs `RefreshError` needs a `provider` field. These are all in `crates/flotilla-core/src/refresh.rs`. For now, add `provider: String::new()` as a placeholder to each construction site — we'll fill in the correct values in Task 4. The construction sites are:
- Line ~196: `category: "checkouts"`
- Line ~206: `category: "PRs"`
- Line ~216: `category: "workspaces"`
- Line ~226: `category: "terminals"`
- Line ~237: `category: "sessions"`
- Line ~246: `category: "branches"`
- Line ~253: `category: "merged"`

Also fix the test helper `refresh_error()` at ~line 623:

```rust
fn refresh_error(category: &'static str) -> RefreshError {
    RefreshError {
        category,
        provider: String::new(),
        message: format!("{category} failure"),
    }
}
```

- [ ] **Step 4: Run `cargo check -p flotilla-core` to verify compilation**

Run: `cargo check -p flotilla-core`
Expected: Compilation errors in `in_process.rs` and `convert.rs` (they reference the old health type). The `data.rs` and `refresh.rs` changes themselves should be clean.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/data.rs crates/flotilla-core/src/refresh.rs
git commit -m "feat: add provider field to RefreshError and update DataStore health type (#195)"
```

### Task 2: Update protocol types

**Files:**
- Modify: `crates/flotilla-protocol/src/snapshot.rs:50,61,71-75`
- Modify: `crates/flotilla-protocol/src/delta.rs:66-69`

- [ ] **Step 1: Change `provider_health` in `Snapshot` and `RepoInfo`**

In `crates/flotilla-protocol/src/snapshot.rs`:

```rust
// In RepoInfo (~line 50):
pub provider_health: HashMap<String, HashMap<String, bool>>,

// In Snapshot (~line 61):
pub provider_health: HashMap<String, HashMap<String, bool>>,
```

- [ ] **Step 2: Add `provider` field to `ProviderError`**

In the same file (~line 71-75):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderError {
    pub category: String,
    pub provider: String,
    pub message: String,
}
```

- [ ] **Step 3: Add `category` to `Change::ProviderHealth`**

In `crates/flotilla-protocol/src/delta.rs` (~line 66-69):

```rust
ProviderHealth {
    category: String,
    provider: String,
    op: EntryOp<bool>,
},
```

- [ ] **Step 4: Fix protocol tests**

In `crates/flotilla-protocol/src/snapshot.rs`, update the test data that constructs `provider_health`. Find tests that use `HashMap::from([("git".to_string(), true)])` and update to nested form:

```rust
// Before:
provider_health: HashMap::from([("git".to_string(), true)]),

// After:
provider_health: HashMap::from([
    ("vcs".to_string(), HashMap::from([("Git".to_string(), true)])),
]),
```

Also update `ProviderError` constructions in tests to include `provider: String::new()` or an appropriate value.

Also update the `provider_error` test helper in `crates/flotilla-core/src/delta.rs` (~line 579-584):

```rust
fn provider_error(category: &str, message: &str) -> ProviderError {
    ProviderError {
        category: category.into(),
        provider: String::new(),
        message: message.into(),
    }
}
```

In `crates/flotilla-protocol/src/lib.rs`, find the test (~line 272) that constructs `provider_health` with `HashMap::from([("git".to_string(), true), ("github".to_string(), false)])` and update similarly.

- [ ] **Step 5: Run `cargo check -p flotilla-protocol` to verify**

Run: `cargo check -p flotilla-protocol`
Expected: Protocol crate compiles. Downstream crates will have errors.

- [ ] **Step 6: Run protocol tests**

Run: `cargo test -p flotilla-protocol`
Expected: All protocol tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/flotilla-protocol/
git commit -m "feat: make protocol health types per-provider (#195)"
```

### Task 3: Update conversion layer

**Files:**
- Modify: `crates/flotilla-core/src/convert.rs:69-96`

- [ ] **Step 1: Update `error_to_proto`**

```rust
pub fn error_to_proto(error: &RefreshError) -> ProviderError {
    ProviderError {
        category: error.category.to_string(),
        provider: error.provider.clone(),
        message: error.message.clone(),
    }
}
```

- [ ] **Step 2: Update `snapshot_to_proto` health conversion**

Convert from `HashMap<(&'static str, String), bool>` to `HashMap<String, HashMap<String, bool>>`:

```rust
pub fn snapshot_to_proto(repo: &Path, seq: u64, refresh: &RefreshSnapshot) -> Snapshot {
    // Build nested health map
    let mut provider_health: HashMap<String, HashMap<String, bool>> = HashMap::new();
    for ((category, provider), &healthy) in &refresh.provider_health {
        provider_health
            .entry(category.to_string())
            .or_default()
            .insert(provider.clone(), healthy);
    }

    Snapshot {
        seq,
        repo: repo.to_path_buf(),
        work_items: refresh
            .work_items
            .iter()
            .map(|item| correlation_result_to_work_item(item, &refresh.correlation_groups))
            .collect(),
        providers: (*refresh.providers).clone(),
        provider_health,
        errors: refresh.errors.iter().map(error_to_proto).collect(),
        issue_total: None,
        issue_has_more: false,
        issue_search_results: None,
    }
}
```

- [ ] **Step 3: Run `cargo check -p flotilla-core` (expect remaining errors in in_process.rs)**

Run: `cargo check -p flotilla-core`
Expected: `convert.rs` compiles. `in_process.rs` still has errors from old health map type.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/convert.rs
git commit -m "feat: update conversion layer for per-provider health (#195)"
```

## Chunk 2: Core Logic Changes

### Task 4: Update `refresh_providers` to track provider names

**Files:**
- Modify: `crates/flotilla-core/src/refresh.rs:107-278`

The key change: capture the display name of the provider that produced each error. Currently `sessions_fut` already iterates per-provider but merges errors. Other futures use `.values().next()` (single provider) but don't capture the name.

- [ ] **Step 1: Update `sessions_fut` to emit per-provider errors**

In `refresh_providers`, change the session error handling (~lines 131-149, 235-242):

```rust
let sessions_fut = async {
    if registry.cloud_agents.is_empty() {
        return (vec![], vec![]);
    }
    let results = futures::future::join_all(registry.cloud_agents.values().map(|ca| {
        let display_name = ca.display_name().to_string();
        async move { (display_name, ca.list_sessions(criteria).await) }
    }))
    .await;

    let mut sessions = Vec::new();
    let mut session_errors = Vec::new();
    for (display_name, result) in results {
        match result {
            Ok(mut entries) => sessions.append(&mut entries),
            Err(e) => session_errors.push((display_name, e)),
        }
    }
    (sessions, session_errors)
};
```

And where session errors are pushed (~line 235-241):

```rust
let (sessions, session_errors) = sessions_bundle;
for (provider, msg) in session_errors {
    errors.push(RefreshError {
        category: "sessions",
        provider,
        message: msg,
    });
}
```

- [ ] **Step 2: Capture provider display names for single-provider futures**

For the other futures, capture the display name of the provider used. Add name capture at each future and pass it through to the error.

For `checkouts_fut` (~line 115):
```rust
let checkouts_fut = async {
    if let Some(cm) = registry.checkout_managers.values().next() {
        (cm.display_name().to_string(), cm.list_checkouts(repo_root).await)
    } else {
        (String::new(), Ok(vec![]))
    }
};
```

For `cr_fut` (~line 123):
```rust
let cr_fut = async {
    if let Some(cr) = registry.code_review.values().next() {
        (cr.display_name().to_string(), cr.list_change_requests(repo_root, 20).await)
    } else {
        (String::new(), Ok(vec![]))
    }
};
```

For `branches_fut` (~line 152):
```rust
let branches_fut = async {
    if let Some(vcs) = registry.vcs.values().next() {
        (vcs.display_name().to_string(), vcs.list_remote_branches(repo_root).await)
    } else {
        (String::new(), Ok(vec![]))
    }
};
```

For `merged_fut` (~line 160):
```rust
let merged_fut = async {
    if let Some(cr) = registry.code_review.values().next() {
        (cr.display_name().to_string(), cr.list_merged_branch_names(repo_root, 50).await)
    } else {
        (String::new(), Ok(vec![]))
    }
};
```

For `ws_fut` (~line 168):
```rust
let ws_fut = async {
    if let Some((_, ws_mgr)) = &registry.workspace_manager {
        (ws_mgr.display_name().to_string(), ws_mgr.list_workspaces().await)
    } else {
        (String::new(), Ok(vec![]))
    }
};
```

For `tp_fut` (~line 176):
```rust
let tp_fut = async {
    if let Some((_, tp)) = &registry.terminal_pool {
        (tp.display_name().to_string(), tp.list_terminals().await)
    } else {
        (String::new(), Ok(vec![]))
    }
};
```

- [ ] **Step 3: Update the destructuring and error construction after `tokio::join!`**

The `tokio::join!` result now returns tuples with display names. Update the destructuring and error pushes:

```rust
let (
    (cm_name, checkouts),
    (cr_name, crs),
    sessions_bundle,
    (vcs_name, branches),
    (merged_name, merged),
    (ws_name, workspaces),
    (tp_name, managed_terminals),
) = tokio::join!(
    checkouts_fut,
    cr_fut,
    sessions_fut,
    branches_fut,
    merged_fut,
    ws_fut,
    tp_fut
);

pd.checkouts = checkouts
    .unwrap_or_else(|e| {
        errors.push(RefreshError {
            category: "checkouts",
            provider: cm_name.clone(),
            message: e,
        });
        Vec::new()
    })
    .into_iter()
    .collect();
pd.change_requests = crs
    .unwrap_or_else(|e| {
        errors.push(RefreshError {
            category: "PRs",
            provider: cr_name.clone(),
            message: e,
        });
        Vec::new()
    })
    .into_iter()
    .collect();
pd.workspaces = workspaces
    .unwrap_or_else(|e| {
        errors.push(RefreshError {
            category: "workspaces",
            provider: ws_name.clone(),
            message: e,
        });
        Vec::new()
    })
    .into_iter()
    .collect();
pd.managed_terminals = managed_terminals
    .unwrap_or_else(|e| {
        errors.push(RefreshError {
            category: "terminals",
            provider: tp_name.clone(),
            message: e,
        });
        Vec::new()
    })
    .into_iter()
    .map(|t| (t.id.to_string(), t))
    .collect();

// Sessions (already per-provider from Step 1)
let (sessions, session_errors) = sessions_bundle;
for (provider, msg) in session_errors {
    errors.push(RefreshError {
        category: "sessions",
        provider,
        message: msg,
    });
}
pd.sessions = sessions.into_iter().collect();

{
    use flotilla_protocol::delta::{Branch, BranchStatus};
    let remote = branches.unwrap_or_else(|e| {
        errors.push(RefreshError {
            category: "branches",
            provider: vcs_name.clone(),
            message: e,
        });
        Vec::new()
    });
    let merged_names = merged.unwrap_or_else(|e| {
        errors.push(RefreshError {
            category: "merged",
            provider: merged_name.clone(),
            message: e,
        });
        Vec::new()
    });
    // ... rest of branch/merged handling unchanged ...
}
```

- [ ] **Step 4: Run `cargo check -p flotilla-core` (refresh.rs should be clean)**

Run: `cargo check -p flotilla-core`
Expected: `refresh.rs` compiles (some errors may remain in `in_process.rs`).

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/refresh.rs
git commit -m "feat: capture provider display names in refresh errors (#195)"
```

### Task 5: Rewrite `compute_provider_health` per-provider

**Files:**
- Modify: `crates/flotilla-core/src/refresh.rs:280-306`

- [ ] **Step 1: Write a failing test for per-provider health**

Add a new test in `refresh.rs` that verifies two cloud agents get independent health:

```rust
#[test]
fn compute_provider_health_per_provider() {
    let mut registry = ProviderRegistry::new();
    registry.cloud_agents.insert(
        "claude".to_string(),
        Arc::new(MockCloudAgent::ok(vec![])),
    );
    registry.cloud_agents.insert(
        "cursor".to_string(),
        Arc::new(MockCloudAgent::ok(vec![])),
    );

    // Only cursor fails
    let errors = vec![RefreshError {
        category: "sessions",
        provider: "Cursor".to_string(),
        message: "auth failed".to_string(),
    }];

    let health = compute_provider_health(&registry, &errors);
    // Claude should be healthy, Cursor should not
    assert_eq!(health.get(&("cloud_agent", "Claude".to_string())), Some(&true));
    assert_eq!(health.get(&("cloud_agent", "Cursor".to_string())), Some(&false));
}
```

Note: `MockCloudAgent` needs to return different display names. Check `MockCloudAgent` — if its `display_name()` is hardcoded, you'll need two separate mock types or make the display name configurable. The simplest fix: add a `name` field to `MockCloudAgent`:

```rust
struct MockCloudAgent {
    result: Result<Vec<(String, CloudAgentSession)>, String>,
    display_name: String,
}

impl MockCloudAgent {
    fn ok(sessions: Vec<(String, CloudAgentSession)>) -> Self {
        Self { result: Ok(sessions), display_name: "MockCA".into() }
    }
    fn ok_named(name: &str, sessions: Vec<(String, CloudAgentSession)>) -> Self {
        Self { result: Ok(sessions), display_name: name.into() }
    }
    fn failing(msg: &str) -> Self {
        Self { result: Err(msg.to_string()), display_name: "MockCA".into() }
    }
    fn failing_named(name: &str, msg: &str) -> Self {
        Self { result: Err(msg.to_string()), display_name: name.into() }
    }
}
```

And update the `CloudAgentService` impl to use it:
```rust
fn display_name(&self) -> &str {
    &self.display_name
}
```

Do the same for `MockCodeReview` if it doesn't already have a configurable name.

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test -p flotilla-core compute_provider_health_per_provider -- --nocapture`
Expected: FAIL — current `compute_provider_health` returns category-level keys.

- [ ] **Step 3: Rewrite `compute_provider_health`**

```rust
fn compute_provider_health(
    registry: &ProviderRegistry,
    errors: &[RefreshError],
) -> HashMap<(&'static str, String), bool> {
    let mut health = HashMap::new();

    for ca in registry.cloud_agents.values() {
        let name = ca.display_name().to_string();
        let has_error = errors
            .iter()
            .any(|e| e.category == "sessions" && e.provider == name);
        health.insert(("cloud_agent", name), !has_error);
    }

    for cr in registry.code_review.values() {
        let name = cr.display_name().to_string();
        let has_error = errors
            .iter()
            .any(|e| (e.category == "PRs" || e.category == "merged") && e.provider == name);
        health.insert(("code_review", name), !has_error);
    }

    if let Some((_, tp)) = &registry.terminal_pool {
        let name = tp.display_name().to_string();
        let has_error = errors
            .iter()
            .any(|e| e.category == "terminals" && e.provider == name);
        health.insert(("terminal_pool", name), !has_error);
    }

    health
}
```

- [ ] **Step 4: Update `RefreshSnapshot.provider_health` type**

In the same file (~line 21):

```rust
// Before:
pub provider_health: HashMap<&'static str, bool>,

// After:
pub provider_health: HashMap<(&'static str, String), bool>,
```

And the Default impl (~line 31):
```rust
provider_health: HashMap::new(),  // no change needed, same default
```

- [ ] **Step 5: Update existing `compute_provider_health` tests**

Update `compute_provider_health_maps_error_categories` to use the new key format. The test constructs `RefreshError` values — add the `provider` field. And assertions change from `health.get("cloud_agent")` to `health.get(&("cloud_agent", "MockCA".to_string()))` (or whatever the mock's display name is).

Also update `spawn_with_failing_provider_sets_error_and_unhealthy_health` (~line 845): its assertion `health.get("cloud_agent")` needs to become `health.get(&("cloud_agent", "MockCA".to_string()))`.

- [ ] **Step 6: Run all refresh tests**

Run: `cargo test -p flotilla-core -- refresh`
Expected: All pass, including the new per-provider test.

- [ ] **Step 7: Commit**

```bash
git add crates/flotilla-core/src/refresh.rs
git commit -m "feat: rewrite compute_provider_health to per-provider granularity (#195)"
```

## Chunk 3: Plumbing (in_process.rs)

### Task 6: Update `InProcessDaemon` health handling

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs`

This is the largest change. The daemon stores, diffs, and broadcasts health data. All references to `HashMap<String, bool>` for health become `HashMap<String, HashMap<String, bool>>`.

- [ ] **Step 1: Update `RepoState` tracking fields**

Change `last_broadcast_health` (~line 159):

```rust
// Before:
last_broadcast_health: HashMap<String, bool>,

// After:
last_broadcast_health: HashMap<String, HashMap<String, bool>>,
```

- [ ] **Step 2: Update `record_delta` to diff nested health maps**

The signature changes (~line 169-172):

```rust
fn record_delta(
    &mut self,
    new_providers: &ProviderData,
    new_health: &HashMap<String, HashMap<String, bool>>,
    new_errors: &[ProviderError],
    work_items: Vec<flotilla_protocol::snapshot::WorkItem>,
) -> DeltaEntry {
```

Replace the health diff block (~lines 178-199) with nested diffing:

```rust
// Diff provider health (nested: category → provider → bool)
for (category, new_providers_map) in new_health {
    let old_providers_map = self.last_broadcast_health.get(category);
    for (provider, &val) in new_providers_map {
        let old_val = old_providers_map.and_then(|m| m.get(provider));
        match old_val {
            Some(&prev) if prev == val => {}
            Some(_) => changes.push(flotilla_protocol::Change::ProviderHealth {
                category: category.clone(),
                provider: provider.clone(),
                op: flotilla_protocol::EntryOp::Updated(val),
            }),
            None => changes.push(flotilla_protocol::Change::ProviderHealth {
                category: category.clone(),
                provider: provider.clone(),
                op: flotilla_protocol::EntryOp::Added(val),
            }),
        }
    }
}
// Detect removals
for (category, old_providers_map) in &self.last_broadcast_health {
    let new_providers_map = new_health.get(category);
    for provider in old_providers_map.keys() {
        if new_providers_map.map_or(true, |m| !m.contains_key(provider)) {
            changes.push(flotilla_protocol::Change::ProviderHealth {
                category: category.clone(),
                provider: provider.clone(),
                op: flotilla_protocol::EntryOp::Removed,
            });
        }
    }
}
```

- [ ] **Step 3: Update `build_repo_snapshot`**

Change the health parameter type (~line 83):

```rust
fn build_repo_snapshot(
    path: &Path,
    seq: u64,
    base: &RefreshSnapshot,
    health: &HashMap<(&'static str, String), bool>,
    cache: &IssueCache,
    search_results: &Option<Vec<(String, Issue)>>,
) -> Snapshot {
```

Note: currently `snapshot_to_proto` converts `RefreshSnapshot.provider_health` and then `build_repo_snapshot` immediately overwrites `snapshot.provider_health` with its own conversion from the `health` parameter. These two health sources should be identical after refresh, so the overwrite is redundant. Remove the overwrite — `snapshot_to_proto` already handles the conversion via `health_to_proto` (added in Step 6). Delete the line:

```rust
// DELETE this line (~line 97) — snapshot_to_proto already converts health:
snapshot.provider_health = health.iter().map(|(k, v)| (k.to_string(), *v)).collect();
```

Also remove the `health` parameter entirely from `build_repo_snapshot` since it's no longer used — `RefreshSnapshot.provider_health` already carries the same data:

```rust
fn build_repo_snapshot(
    path: &Path,
    seq: u64,
    base: &RefreshSnapshot,
    cache: &IssueCache,
    search_results: &Option<Vec<(String, Issue)>>,
) -> Snapshot {
```

Then update all call sites to remove the `health` argument (there are 3: `poll_refreshes`, `broadcast_snapshot`, `catch_up_events`).

- [ ] **Step 4: Update `RepoInfo` construction in `list_repos` and `add_repo`**

In `list_repos` (~lines 808-814), change the health conversion:

```rust
// Before:
provider_health: state
    .model
    .data
    .provider_health
    .iter()
    .map(|(k, v)| (k.to_string(), *v))
    .collect(),

// After:
provider_health: {
    let mut h: HashMap<String, HashMap<String, bool>> = HashMap::new();
    for ((category, provider), &healthy) in &state.model.data.provider_health {
        h.entry(category.to_string())
            .or_default()
            .insert(provider.clone(), healthy);
    }
    h
},
```

Apply the same change to `add_repo` (~lines 972-977).

- [ ] **Step 5: Update the refresh poll loop health storage**

In `poll_refreshes` (~line 379), where `state.model.data.provider_health` is assigned — this is a no-op since both types already match after earlier changes:

```rust
state.model.data.provider_health = snapshot.provider_health.clone();
```

The proto_snapshot construction (~lines 382-389) that previously converted health inline now uses `health_to_proto` (or can be removed since `snapshot_to_proto` already handles it). Replace:

```rust
// Before:
proto_snapshot.provider_health = state
    .model
    .data
    .provider_health
    .iter()
    .map(|(k, v)| (k.to_string(), *v))
    .collect();

// After — use the helper:
proto_snapshot.provider_health = health_to_proto(&state.model.data.provider_health);
```

- [ ] **Step 6: Extract a helper for the repeated core→proto health conversion**

The pattern `HashMap<(&'static str, String), bool>` → `HashMap<String, HashMap<String, bool>>` appears 3-4 times. Add a helper in `convert.rs`:

```rust
pub fn health_to_proto(
    health: &HashMap<(&'static str, String), bool>,
) -> HashMap<String, HashMap<String, bool>> {
    let mut nested = HashMap::new();
    for ((category, provider), &healthy) in health {
        nested
            .entry(category.to_string())
            .or_default()
            .insert(provider.clone(), healthy);
    }
    nested
}
```

Then use it everywhere instead of inline conversions.

- [ ] **Step 7: Run `cargo check -p flotilla-core`**

Run: `cargo check -p flotilla-core`
Expected: Core crate compiles. TUI and client crates may still have errors.

- [ ] **Step 8: Run core tests**

Run: `cargo test -p flotilla-core`
Expected: All core tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/flotilla-core/src/in_process.rs crates/flotilla-core/src/convert.rs
git commit -m "feat: update daemon and plumbing for per-provider health (#195)"
```

## Chunk 4: TUI and Client Updates

### Task 7: Update TUI health propagation

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:53-65,80-108,220-251,283-346`
- Modify: `crates/flotilla-tui/src/cli.rs:22-26`
- Modify: `crates/flotilla-tui/src/app/test_builders.rs:118-126`
- Modify: `crates/flotilla-tui/src/app/test_support.rs:89`

- [ ] **Step 1: Update `TuiRepoModel.provider_health` type**

In `crates/flotilla-tui/src/app/mod.rs` (~line 57):

```rust
// Before:
pub provider_health: HashMap<String, bool>,

// After:
pub provider_health: HashMap<String, HashMap<String, bool>>,
```

- [ ] **Step 2: Simplify `apply_snapshot` health→status propagation**

Replace the broadcast loop (~lines 239-251) with direct 1:1 mapping:

```rust
// Provider health -> model-level statuses (now 1:1)
for (category, providers) in &rm.provider_health {
    for (provider_name, &healthy) in providers {
        let status = if healthy {
            ProviderStatus::Ok
        } else {
            ProviderStatus::Error
        };
        let key = (path.clone(), category.clone(), provider_name.clone());
        self.model.provider_statuses.insert(key, status);
    }
}
```

- [ ] **Step 3: Update `apply_delta` health handling**

Replace the delta health application (~lines 302-322):

```rust
flotilla_protocol::Change::ProviderHealth {
    category,
    provider,
    op: flotilla_protocol::EntryOp::Added(v)
      | flotilla_protocol::EntryOp::Updated(v),
} => {
    rm.provider_health
        .entry(category.clone())
        .or_default()
        .insert(provider.clone(), *v);
}
flotilla_protocol::Change::ProviderHealth {
    category,
    provider,
    op: flotilla_protocol::EntryOp::Removed,
} => {
    if let Some(providers) = rm.provider_health.get_mut(category) {
        providers.remove(provider);
        if providers.is_empty() {
            rm.provider_health.remove(category);
        }
    }
}
```

And replace the delta health→status propagation (~lines 334-346) with the same 1:1 loop as in Step 2.

- [ ] **Step 4: Update CLI health display**

In `crates/flotilla-tui/src/cli.rs` (~lines 22-26):

```rust
// Before:
let health: Vec<String> = repo
    .provider_health
    .iter()
    .map(|(k, v)| format!("{k}: {}", if *v { "ok" } else { "error" }))
    .collect();

// After:
let health: Vec<String> = repo
    .provider_health
    .iter()
    .flat_map(|(category, providers)| {
        providers.iter().map(move |(name, v)| {
            format!("{category}/{name}: {}", if *v { "ok" } else { "error" })
        })
    })
    .collect();
```

- [ ] **Step 5: Update test builders**

In `crates/flotilla-tui/src/app/test_builders.rs` (~line 123):
```rust
provider_health: HashMap::new(),  // no change needed — same empty default
```

In `crates/flotilla-tui/src/app/test_support.rs` (~line 89):
```rust
provider_health: HashMap::new(),  // no change needed
```

- [ ] **Step 6: Update client placeholder**

In `crates/flotilla-client/src/lib.rs` (~line 706):
```rust
provider_health: HashMap::new(),  // no change needed
```

- [ ] **Step 7: Run full workspace check**

Run: `cargo check --workspace`
Expected: Everything compiles.

- [ ] **Step 8: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass. If any snapshot tests fail (insta), review the diff and update if correct.

- [ ] **Step 9: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: No warnings.

- [ ] **Step 10: Commit**

```bash
git add crates/flotilla-tui/ crates/flotilla-client/
git commit -m "feat: update TUI and client for per-provider health status (#195)"
```

### Task 8: Add integration test for per-provider health isolation

**Files:**
- Modify: `crates/flotilla-core/src/refresh.rs` (tests section)

- [ ] **Step 1: Write integration test with two cloud agents, one failing**

Add to the tests in `refresh.rs`:

```rust
#[tokio::test]
async fn spawn_with_mixed_provider_health_isolates_failures() {
    let mut registry = ProviderRegistry::new();
    registry.cloud_agents.insert(
        "claude".to_string(),
        Arc::new(MockCloudAgent::ok_named("Claude", vec![])),
    );
    registry.cloud_agents.insert(
        "cursor".to_string(),
        Arc::new(MockCloudAgent::failing_named("Cursor", "auth failed")),
    );

    let handle = RepoRefreshHandle::spawn(
        repo_root(),
        Arc::new(registry),
        criteria(),
        Duration::from_secs(3600),
    );

    let mut rx = handle.snapshot_rx.clone();
    let snapshot = wait_for_snapshot(&mut rx).await;

    // Cursor error should exist
    assert!(snapshot.errors.iter().any(|e| e.provider == "Cursor"));
    // Claude healthy, Cursor not
    assert_eq!(
        snapshot.provider_health.get(&("cloud_agent", "Claude".to_string())),
        Some(&true)
    );
    assert_eq!(
        snapshot.provider_health.get(&("cloud_agent", "Cursor".to_string())),
        Some(&false)
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p flotilla-core spawn_with_mixed_provider_health -- --nocapture`
Expected: PASS

- [ ] **Step 3: Run full test suite + clippy + fmt**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --workspace`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/refresh.rs
git commit -m "test: add integration test for per-provider health isolation (#195)"
```

### Task 9: Final verification

- [ ] **Step 1: Run full pre-push checks**

```bash
cargo fmt
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Expected: All pass, zero warnings.

- [ ] **Step 2: Verify the fix conceptually**

Trace the data flow mentally:
1. Two cloud agents registered: Claude (ok) and Cursor (fails)
2. `refresh_providers` creates `RefreshError { category: "sessions", provider: "Cursor", ... }`
3. `compute_provider_health` returns `{("cloud_agent", "Claude"): true, ("cloud_agent", "Cursor"): false}`
4. Protocol snapshot has `{"cloud_agent": {"Claude": true, "Cursor": false}}`
5. TUI maps directly: `(path, "cloud_agent", "Claude") → Ok`, `(path, "cloud_agent", "Cursor") → Error`
6. UI renders Claude with green check, Cursor with red X
