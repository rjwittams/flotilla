# Provider Follow-ups Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete the provider migration by fixing per-repo registry, deduplicating workspace creation, abstracting remaining direct subprocess calls, and optionally wiring the correlation engine.

**Architecture:** Five independent follow-up tasks from the provider migration, ordered by priority. Tasks 1-3 are small, task 4 is medium, task 5 is large. Each task produces a compilable, testable commit.

**Tech Stack:** Rust, async_trait, tokio, serde, IndexMap

---

## Task 1: Per-repo Provider Registry

Currently `App` has a single `registry: ProviderRegistry` detected from the first repo. Different repos may have different git remotes (GitHub vs GitLab vs none), so each repo needs its own registry.

**Files:**
- Modify: `src/app.rs` — move `registry` from `App` to `RepoState`, remove from `App`
- Modify: `src/main.rs` — run `detect_providers()` per repo, update all action handlers to read registry from `RepoState`
- Modify: `src/data.rs` — no changes needed (`refresh()` already takes `&ProviderRegistry` as a parameter)

**Step 1: Move `registry` into `RepoState`**

In `src/app.rs`, add `registry` field to `RepoState`:

```rust
pub struct RepoState {
    pub repo_root: PathBuf,
    pub registry: ProviderRegistry,  // ADD THIS
    pub data: DataStore,
    pub table_state: TableState,
    pub selected_selectable_idx: Option<usize>,
    pub has_unseen_changes: bool,
    pub multi_selected: BTreeSet<usize>,
}
```

Update `RepoState::new()` to accept and store it:

```rust
pub fn new(repo_root: PathBuf, registry: ProviderRegistry) -> Self {
    Self {
        repo_root,
        registry,
        data: DataStore::default(),
        // ... rest unchanged
    }
}
```

Remove `pub registry: ProviderRegistry` from `App`.

Add a convenience accessor on App:

```rust
pub fn active_registry(&self) -> &ProviderRegistry {
    &self.active().registry
}
```

**Step 2: Run `cargo check` to find all compilation errors**

Run: `cargo check 2>&1`
Expected: Errors in main.rs where `app.registry` is used. Fix each one.

**Step 3: Update `App::new()` to detect providers per repo**

In `src/app.rs`, update `App::new()`:

```rust
pub fn new(repos: Vec<PathBuf>) -> Self {
    let mut map = HashMap::new();
    let mut order = Vec::new();
    for path in repos {
        if !map.contains_key(&path) {
            let registry = crate::providers::discovery::detect_providers(&path);
            map.insert(path.clone(), RepoState::new(path.clone(), registry));
            order.push(path);
        }
    }
    Self {
        repos: map,
        repo_order: order,
        ..Default::default()
    }
}
```

Update `App::add_repo()` similarly:

```rust
pub fn add_repo(&mut self, path: PathBuf) {
    if !self.repos.contains_key(&path) {
        let registry = crate::providers::discovery::detect_providers(&path);
        self.repos.insert(path.clone(), RepoState::new(path.clone(), registry));
        self.repo_order.push(path);
    }
}
```

**Step 4: Update main.rs — remove single detect_providers call, fix all action handlers**

In `src/main.rs`:

1. Remove the startup detection block (lines 82-84):
```rust
// DELETE THIS:
if let Some(first_repo) = app.repo_order.first().cloned() {
    app.registry = detect_providers(&first_repo);
}
```

2. Remove `use providers::discovery::detect_providers;` (moved to app.rs).

3. For every action handler that accesses `app.registry`, change to read from `app.active().registry` or a local binding. Examples:

```rust
// BEFORE:
if let Some((_, ws_mgr)) = &app.registry.workspace_manager {
// AFTER:
if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
```

Apply this pattern to ALL registry usages in main.rs action handlers:
- `SelectWorkspace` → `app.active().registry.workspace_manager`
- `ConfirmDelete` → `app.active().registry.checkout_managers`
- `OpenPr` → `app.active().registry.code_review`
- `OpenIssueBrowser` → `app.active().registry.issue_trackers`
- `CreateWorktree` → `app.active().registry.checkout_managers`
- `ArchiveSession` → `app.active().registry.coding_agents`
- `TeleportSession` → `app.active().registry.checkout_managers`
- `GenerateBranchName` → `app.active().registry.ai_utilities`

**Important borrow checker note:** Some handlers need both mutable access to `app` and immutable access to registry. For these, extract the registry reference into a local clone or extract the needed values before mutably borrowing app. The pattern from the existing code works: extract data first, then use registry.

**Step 5: Update `refresh_all()` in main.rs**

The `refresh_all()` function currently borrows `app.registry` immutably. Change it to borrow each repo's registry instead:

```rust
async fn refresh_all(app: &mut app::App) {
    let snapshots: Vec<_> = app.repo_order.iter()
        .map(|path| app.repos[path].data_snapshot())
        .collect();

    // Extract data stores AND registries
    let items: Vec<(PathBuf, data::DataStore, &ProviderRegistry)> = ... // tricky with lifetimes
```

Actually, the borrow checker makes this hard. The simplest approach: extract DataStore values, then iterate repo_order to get registry refs:

```rust
async fn refresh_all(app: &mut app::App) {
    let snapshots: Vec<_> = app.repo_order.iter()
        .map(|path| app.repos[path].data_snapshot())
        .collect();

    let items: Vec<(PathBuf, data::DataStore)> = app.repo_order.iter()
        .map(|path| {
            let ds = std::mem::take(&mut app.repos.get_mut(path).unwrap().data);
            (path.clone(), ds)
        })
        .collect();

    // Now repos still has registries but no data stores
    let results = futures::future::join_all(
        items.into_iter().map(|(root, mut ds)| {
            let registry = &app.repos[&root].registry;
            async move {
                let errors = ds.refresh(&root, registry).await;
                (root, ds, errors)
            }
        })
    ).await;
    // ... rest unchanged
}
```

If the borrow checker complains about `app.repos` being borrowed both mutably (for take) and immutably (for registry), use unsafe or restructure. The simplest fix: clone the registries or use Arc. But since ProviderRegistry contains `Box<dyn Trait>`, it can't be cloned.

Alternative approach: just keep a single refresh loop instead of join_all:

```rust
async fn refresh_all(app: &mut app::App) {
    let snapshots: Vec<_> = app.repo_order.iter()
        .map(|path| app.repos[path].data_snapshot())
        .collect();

    // Refresh each repo sequentially using its own registry
    for (i, path) in app.repo_order.clone().iter().enumerate() {
        let mut ds = std::mem::take(&mut app.repos.get_mut(path).unwrap().data);
        let errors = ds.refresh(path, &app.repos[path].registry).await;
        // ... error handling, change detection same as before
        let rs = app.repos.get_mut(path).unwrap();
        rs.data = ds;
        // ... selection restore
    }
}
```

Wait — this won't work either because we need to take `data` mutably while borrowing `registry` immutably from the same `RepoState`.

**Best approach**: Temporarily move the registry out alongside the data:

```rust
let items: Vec<(PathBuf, data::DataStore, ProviderRegistry)> = app.repo_order.iter()
    .map(|path| {
        let rs = app.repos.get_mut(path).unwrap();
        let ds = std::mem::take(&mut rs.data);
        let reg = std::mem::take(&mut rs.registry);
        (path.clone(), ds, reg)
    })
    .collect();

let results = futures::future::join_all(
    items.into_iter().map(|(root, mut ds, registry)| {
        async move {
            let errors = ds.refresh(&root, &registry).await;
            (root, ds, registry, errors)
        }
    })
).await;

for (i, (path, data, registry, errors)) in results.into_iter().enumerate() {
    let rs = app.repos.get_mut(&path).unwrap();
    rs.data = data;
    rs.registry = registry;
    // ... rest unchanged
}
```

This is clean — take both out, refresh with both, put both back.

**Step 6: Verify and commit**

Run: `cargo check && cargo test && cargo clippy`
Expected: All pass.

```bash
git add src/app.rs src/main.rs
git commit -m "refactor: per-repo provider registry for multi-repo correctness"
```

---

## Task 2: Migrate create_cmux_workspace Through Registry

`actions::create_cmux_workspace()` (~170 lines) duplicates `CmuxWorkspaceManager::create_workspace()`. All call sites should use the registry's workspace manager instead.

**Files:**
- Modify: `src/main.rs` — replace `actions::create_cmux_workspace()` calls with `registry.workspace_manager.create_workspace()`
- Modify: `src/actions.rs` — delete `create_cmux_workspace()`, `cmux_cmd()`, `parse_ok_ref()`; if empty, delete file entirely
- Modify: `src/main.rs` — remove `mod actions` if actions.rs is deleted

**Step 1: Update call sites in main.rs**

There are 3 call sites for `actions::create_cmux_workspace()`:

1. **SwitchWorktree handler** (main.rs ~line 169):
```rust
// BEFORE:
let tmpl = template::WorkspaceTemplate::load(app.active_repo_root());
if let Err(e) = actions::create_cmux_workspace(&tmpl, &wt.path, "claude", &wt.branch).await {

// AFTER:
if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
    let tmpl_path = app.active_repo_root().join(".cmux/workspace.yaml");
    let template_yaml = std::fs::read_to_string(&tmpl_path).ok();
    let mut template_vars = std::collections::HashMap::new();
    template_vars.insert("main_command".to_string(), "claude".to_string());
    let config = crate::providers::types::WorkspaceConfig {
        name: wt.branch.clone(),
        working_directory: wt.path.clone(),
        template_vars,
        template_yaml,
    };
    if let Err(e) = ws_mgr.create_workspace(&config).await {
        app.status_message = Some(e);
    }
}
```

2. **CreateWorktree handler** (main.rs ~line 239):
```rust
// BEFORE:
let tmpl = template::WorkspaceTemplate::load(app.active_repo_root());
if let Err(e) = actions::create_cmux_workspace(&tmpl, &checkout.path, "claude", &branch).await {

// AFTER:
if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
    let tmpl_path = app.active_repo_root().join(".cmux/workspace.yaml");
    let template_yaml = std::fs::read_to_string(&tmpl_path).ok();
    let mut template_vars = std::collections::HashMap::new();
    template_vars.insert("main_command".to_string(), "claude".to_string());
    let config = crate::providers::types::WorkspaceConfig {
        name: branch.clone(),
        working_directory: checkout.path.clone(),
        template_vars,
        template_yaml,
    };
    if let Err(e) = ws_mgr.create_workspace(&config).await {
        app.status_message = Some(e);
    }
}
```

3. **TeleportSession handler** (main.rs ~line 282):
```rust
// BEFORE:
let tmpl = template::WorkspaceTemplate::load(app.active_repo_root());
if let Err(e) = actions::create_cmux_workspace(&tmpl, &path, &teleport_cmd, name).await {

// AFTER:
if let Some((_, ws_mgr)) = &app.active().registry.workspace_manager {
    let tmpl_path = app.active_repo_root().join(".cmux/workspace.yaml");
    let template_yaml = std::fs::read_to_string(&tmpl_path).ok();
    let mut template_vars = std::collections::HashMap::new();
    template_vars.insert("main_command".to_string(), teleport_cmd.clone());
    let config = crate::providers::types::WorkspaceConfig {
        name: name.to_string(),
        working_directory: path.clone(),
        template_vars,
        template_yaml,
    };
    if let Err(e) = ws_mgr.create_workspace(&config).await {
        app.status_message = Some(e);
    }
}
```

**DRY helper**: To avoid repeating the template-loading logic, extract a helper in main.rs:

```rust
fn workspace_config(repo_root: &Path, name: &str, working_dir: &Path, main_command: &str) -> crate::providers::types::WorkspaceConfig {
    let tmpl_path = repo_root.join(".cmux/workspace.yaml");
    let template_yaml = std::fs::read_to_string(&tmpl_path).ok();
    let mut template_vars = std::collections::HashMap::new();
    template_vars.insert("main_command".to_string(), main_command.to_string());
    crate::providers::types::WorkspaceConfig {
        name: name.to_string(),
        working_directory: working_dir.to_path_buf(),
        template_vars,
        template_yaml,
    }
}
```

**Step 2: Delete the old code**

In `src/actions.rs`, delete `create_cmux_workspace()`, `cmux_cmd()`, and `parse_ok_ref()`.

If actions.rs is now empty, delete the file entirely and remove `mod actions;` from `src/main.rs`.

Also remove `mod template;` from `src/main.rs` and delete `src/template.rs` since the template rendering is now handled inside `CmuxWorkspaceManager::create_workspace()`.

Actually — check if `template.rs` types (`WorkspaceTemplate`) are used in `workspace/cmux.rs`. Looking at cmux.rs line 7: `use crate::template::WorkspaceTemplate;`. Yes, it's still needed by the provider. Keep `template.rs` and `mod template;` in main.rs.

**Step 3: Verify and commit**

Run: `cargo check && cargo test && cargo clippy`
Expected: All pass.

```bash
git add -A
git commit -m "refactor: route workspace creation through provider registry"
```

---

## Task 3: Abstract fetch_merged_pr_branches Into CodeReview Trait

`fetch_merged_pr_branches()` in data.rs calls `gh pr list --state merged` directly. This should go through the CodeReview trait.

**Files:**
- Modify: `src/providers/code_review/mod.rs` — add `list_merged_branch_names()` to trait
- Modify: `src/providers/code_review/github.rs` — implement the method
- Modify: `src/data.rs` — call through registry, delete `fetch_merged_pr_branches()`

**Step 1: Add trait method**

In `src/providers/code_review/mod.rs`:

```rust
#[async_trait]
pub trait CodeReview: Send + Sync {
    fn display_name(&self) -> &str;
    async fn list_change_requests(&self, repo_root: &Path, limit: usize) -> Result<Vec<ChangeRequest>, String>;
    async fn get_change_request(&self, repo_root: &Path, id: &str) -> Result<ChangeRequest, String>;
    async fn open_in_browser(&self, repo_root: &Path, id: &str) -> Result<(), String>;
    async fn list_merged_branch_names(&self, repo_root: &Path, limit: usize) -> Result<Vec<String>, String>;  // ADD
}
```

**Step 2: Implement in GitHubCodeReview**

In `src/providers/code_review/github.rs`, add the implementation:

```rust
async fn list_merged_branch_names(
    &self,
    repo_root: &Path,
    limit: usize,
) -> Result<Vec<String>, String> {
    let limit_str = limit.to_string();
    let output = self
        .run_cmd(
            "gh",
            &[
                "pr", "list", "--state", "merged", "--limit", &limit_str,
                "--json", "headRefName",
            ],
            repo_root,
        )
        .await?;
    let prs: Vec<serde_json::Value> =
        serde_json::from_str(&output).map_err(|e| e.to_string())?;
    Ok(prs
        .iter()
        .filter_map(|p| p.get("headRefName").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect())
}
```

**Step 3: Call through registry in DataStore::refresh()**

In `src/data.rs`, replace the `fetch_merged_pr_branches` call:

```rust
// BEFORE:
let merged_fut = fetch_merged_pr_branches(repo_root);

// AFTER:
let merged_fut = async {
    if let Some(cr) = registry.code_review.values().next() {
        cr.list_merged_branch_names(repo_root.as_path(), 50).await
    } else {
        Ok(vec![])
    }
};
```

Delete the `fetch_merged_pr_branches()` function from data.rs.

**Step 4: Verify and commit**

Run: `cargo check && cargo test && cargo clippy`
Expected: All pass.

```bash
git add src/providers/code_review/mod.rs src/providers/code_review/github.rs src/data.rs
git commit -m "refactor: route merged branch listing through CodeReview trait"
```

---

## Task 4: Enrich Checkout Type and Replace data::Worktree

`DataStore` uses a custom `data::Worktree` type from direct `wt list --format=json` calls. This should be replaced by the provider's `Checkout` type, enriched with optional status fields.

**Files:**
- Modify: `src/providers/types.rs` — add optional enrichment fields to `Checkout`
- Modify: `src/providers/vcs/wt.rs` — deserialize full wt JSON, populate enrichment fields in `list_checkouts()`
- Modify: `src/data.rs` — replace `Vec<Worktree>` with `Vec<Checkout>`, delete `Worktree`/`AheadBehind`/`RemoteStatus`/`WorkingTree`/`CommitInfo` types and `fetch_worktrees()`
- Modify: `src/ui.rs` — adapt rendering to use Checkout + provider types
- Modify: `src/main.rs` — update action handlers that read worktree data
- Modify: `src/app.rs` — update `data_snapshot()` if needed

**Step 1: Add enrichment fields to Checkout**

In `src/providers/types.rs`, add optional fields to `Checkout`:

```rust
#[derive(Debug, Clone)]
pub struct Checkout {
    pub branch: String,
    pub path: PathBuf,
    pub is_trunk: bool,
    pub trunk_ahead_behind: Option<AheadBehind>,
    pub remote_ahead_behind: Option<AheadBehind>,
    pub working_tree: Option<WorkingTreeStatus>,
    pub last_commit: Option<CommitInfo>,
    pub correlation_keys: Vec<CorrelationKey>,
}
```

Note: `WorkingTreeStatus` uses `usize` for counts. The UI currently checks booleans, so update the UI to check `> 0`. Also, `CommitInfo` uses non-Optional `String` fields; the wt JSON may have nulls, so map `None` → empty string during deserialization.

**Step 2: Update WtCheckoutManager to populate enrichment fields**

In `src/providers/vcs/wt.rs`, expand the `WtWorktree` struct to capture all fields from `wt list --format=json`:

```rust
#[derive(Debug, Deserialize)]
struct WtWorktree {
    branch: String,
    path: PathBuf,
    #[serde(default)]
    is_main: bool,
    #[serde(default)]
    is_current: bool,
    #[serde(default)]
    main: Option<WtAheadBehind>,
    #[serde(default)]
    remote: Option<WtRemote>,
    #[serde(default)]
    working_tree: Option<WtWorkingTree>,
    #[serde(default)]
    commit: Option<WtCommit>,
}

#[derive(Debug, Deserialize)]
struct WtAheadBehind {
    ahead: i64,
    behind: i64,
}

#[derive(Debug, Deserialize)]
struct WtRemote {
    #[allow(dead_code)]
    name: Option<String>,
    #[allow(dead_code)]
    branch: Option<String>,
    ahead: i64,
    behind: i64,
}

#[derive(Debug, Deserialize)]
struct WtWorkingTree {
    #[serde(default)]
    staged: bool,
    #[serde(default)]
    modified: bool,
    #[serde(default)]
    untracked: bool,
}

#[derive(Debug, Deserialize)]
struct WtCommit {
    short_sha: Option<String>,
    message: Option<String>,
}
```

Update `list_checkouts()` to populate enrichment fields:

```rust
Ok(worktrees
    .into_iter()
    .map(|wt| {
        let correlation_keys = vec![
            CorrelationKey::Branch(wt.branch.clone()),
            CorrelationKey::RepoPath(wt.path.clone()),
        ];
        Checkout {
            branch: wt.branch,
            path: wt.path,
            is_trunk: wt.is_main,
            trunk_ahead_behind: wt.main.map(|m| AheadBehind { ahead: m.ahead, behind: m.behind }),
            remote_ahead_behind: wt.remote.map(|r| AheadBehind { ahead: r.ahead, behind: r.behind }),
            working_tree: wt.working_tree.map(|w| WorkingTreeStatus {
                staged: if w.staged { 1 } else { 0 },
                modified: if w.modified { 1 } else { 0 },
                untracked: if w.untracked { 1 } else { 0 },
            }),
            last_commit: wt.commit.map(|c| CommitInfo {
                short_sha: c.short_sha.unwrap_or_default(),
                message: c.message.unwrap_or_default(),
            }),
            correlation_keys,
        }
    })
    .collect())
```

**Step 3: Update DataStore**

In `src/data.rs`:

1. Replace `pub worktrees: Vec<Worktree>` with `pub checkouts: Vec<Checkout>` in `DataStore`
2. Add `Checkout` to the imports from `providers::types`
3. In `refresh()`, replace the `fetch_worktrees()` call with a registry call:

```rust
// BEFORE:
let wt_fut = fetch_worktrees(repo_root);

// AFTER:
let checkouts_fut = async {
    if let Some(cm) = registry.checkout_managers.values().next() {
        cm.list_checkouts(repo_root.as_path()).await
    } else {
        Ok(vec![])
    }
};
```

4. Replace `self.worktrees = wt.unwrap_or_else(...)` with `self.checkouts = checkouts.unwrap_or_else(...)`

5. Update `correlate()` — replace all `self.worktrees` references with `self.checkouts`:
   - Loop: `for (i, co) in self.checkouts.iter().enumerate()`
   - Branch: `co.branch.clone()`
   - is_main: `co.is_trunk`
   - Workspace matching in `find_workspaces_for_worktree()` → rename to `find_workspaces_for_checkout()`, use `co.path`

6. Delete types: `Worktree`, `AheadBehind`, `RemoteStatus`, `WorkingTree`, `CommitInfo`
7. Delete `fetch_worktrees()` function
8. Delete `run_command()` if no longer used (check if `fetch_delete_confirm_info` still uses it — yes, it does, keep it)

**Step 4: Update UI**

In `src/ui.rs`:

1. Update the git status column rendering in `build_item_row()`:

```rust
// BEFORE:
if wt.working_tree.as_ref().is_some_and(|w| w.modified) { s.push('M'); }

// AFTER:
if co.working_tree.as_ref().is_some_and(|w| w.modified > 0) { s.push('M'); }
```

Same pattern for `staged` and `untracked`. For `main.ahead`:

```rust
// BEFORE:
if wt.main.as_ref().is_some_and(|m| m.ahead > 0) { s.push('↑'); }

// AFTER:
if co.trunk_ahead_behind.as_ref().is_some_and(|m| m.ahead > 0) { s.push('↑'); }
```

2. Update `render_preview()`:

```rust
// BEFORE:
if let Some(wt) = app.active().data.worktrees.get(wt_idx) {
    lines.push(format!("Path: {}", wt.path.display()));
    if let Some(commit) = &wt.commit {
        let sha = commit.short_sha.as_deref().unwrap_or("?");

// AFTER:
if let Some(co) = app.active().data.checkouts.get(wt_idx) {
    lines.push(format!("Path: {}", co.path.display()));
    if let Some(commit) = &co.last_commit {
        let sha = if commit.short_sha.is_empty() { "?" } else { &commit.short_sha };
```

Similar updates for `main` → `trunk_ahead_behind`, `remote` → `remote_ahead_behind`.

Note: `CommitInfo` fields are now `String` not `Option<String>`, so use `.is_empty()` instead of `.as_deref().unwrap_or("?")`.

**Step 5: Update main.rs action handlers**

Update references to `data.worktrees` → `data.checkouts`:

1. **SwitchWorktree**: `app.active().data.worktrees.get(i)` → `app.active().data.checkouts.get(i)`, `wt.path` → `co.path`, `wt.branch` → `co.branch`
2. **FetchDeleteInfo**: `app.active().data.worktrees.get(idx)` → `app.active().data.checkouts.get(idx)`, `wt.path` → `co.path`
3. **TeleportSession**: `app.active().data.worktrees.get(wt_idx)` → `app.active().data.checkouts.get(wt_idx)`, `wt.path` → `co.path`

**Step 6: Update app.rs data_snapshot()**

In `src/app.rs`, update `data_snapshot()`:

```rust
// BEFORE:
self.data.worktrees.len(),

// AFTER:
self.data.checkouts.len(),
```

**Step 7: Verify and commit**

Run: `cargo check && cargo test && cargo clippy`
Expected: All pass.

```bash
git add -A
git commit -m "refactor: replace data::Worktree with enriched providers::types::Checkout"
```

---

## Task 5: Wire Correlation Engine Into DataStore

Replace the manual HashMap-based `DataStore::correlate()` with the union-find `correlation::correlate()` engine. This enables transitive grouping (e.g., a session linked to a branch linked to a PR linked to an issue all appear in one WorkItem).

**Complexity:** HIGH. The current correlate() is ~220 lines of manual grouping. Replacing it requires converting all data sources to `CorrelatedItem`, calling the engine, and mapping groups back to the sectioned table format.

**Files:**
- Modify: `src/data.rs` — rewrite `correlate()` to use `correlation::correlate()`
- No changes to `src/providers/correlation.rs` (engine is complete and tested)

**Step 1: Build CorrelatedItems from all data sources**

In `src/data.rs`, import the correlation module:

```rust
use crate::providers::correlation::{correlate as correlate_items, CorrelatedItem, ItemKind};
```

Rewrite `correlate()` to first build a flat list of `CorrelatedItem`s:

```rust
fn correlate(&mut self) {
    let mut items: Vec<CorrelatedItem> = Vec::new();

    // Checkouts
    for co in &self.checkouts {
        items.push(CorrelatedItem {
            provider_name: "local".to_string(),
            kind: ItemKind::Checkout,
            title: co.branch.clone(),
            correlation_keys: co.correlation_keys.clone(),
        });
    }

    // Change requests
    for cr in &self.change_requests {
        items.push(CorrelatedItem {
            provider_name: "github".to_string(),
            kind: ItemKind::ChangeRequest,
            title: cr.title.clone(),
            correlation_keys: cr.correlation_keys.clone(),
        });
    }

    // Sessions
    for ses in &self.sessions {
        items.push(CorrelatedItem {
            provider_name: "claude".to_string(),
            kind: ItemKind::CloudSession,
            title: ses.title.clone(),
            correlation_keys: ses.correlation_keys.clone(),
        });
    }

    // Issues
    for issue in &self.issues {
        items.push(CorrelatedItem {
            provider_name: "github".to_string(),
            kind: ItemKind::Issue,
            title: issue.title.clone(),
            correlation_keys: issue.correlation_keys.clone(),
        });
    }

    // Workspaces
    for ws in &self.workspaces {
        items.push(CorrelatedItem {
            provider_name: "cmux".to_string(),
            kind: ItemKind::Workspace,
            title: ws.name.clone(),
            correlation_keys: ws.correlation_keys.clone(),
        });
    }

    let groups = correlate_items(items);
    self.build_table_from_groups(groups);
}
```

**Step 2: Map groups back to WorkItems and table entries**

Add a new method `build_table_from_groups()`:

```rust
fn build_table_from_groups(&mut self, groups: Vec<crate::providers::correlation::CorrelatedGroup>) {
    // For each group, determine:
    // - Primary kind (Worktree > Session > PR > RemoteBranch > Issue priority)
    // - Linked indices into self.checkouts, self.change_requests, etc.
    // This requires matching group items back to their source indices.

    // Strategy: track original indices when building CorrelatedItems (add index to title
    // or use a side map). Simplest: build items with a tag that encodes source index.
    // ... (detailed implementation follows)
}
```

**Implementation approach**: When building CorrelatedItems in step 1, include the source type and index as metadata. Since `CorrelatedItem` doesn't have a metadata field, use a wrapper or build a parallel index map:

```rust
// Track which CorrelatedItem corresponds to which source
enum SourceRef {
    Checkout(usize),
    ChangeRequest(usize),
    Session(usize),
    Issue(usize),
    Workspace(usize),
}

let mut source_refs: Vec<SourceRef> = Vec::new();
// Push SourceRef::Checkout(i) alongside each checkout CorrelatedItem, etc.
```

After correlation, iterate groups and resolve source refs to build WorkItems:

```rust
for group in &groups {
    let mut work_item = WorkItem { /* defaults */ };
    for item in &group.items {
        let source_idx = /* look up in source_refs by matching position */;
        match source_idx {
            SourceRef::Checkout(i) => {
                work_item.worktree_idx = Some(i);
                work_item.kind = WorkItemKind::Worktree;
                work_item.branch = Some(self.checkouts[i].branch.clone());
                work_item.is_main_worktree = self.checkouts[i].is_trunk;
                // find workspace refs
                work_item.workspace_refs = self.find_workspaces_for_checkout(&self.checkouts[i]);
            }
            SourceRef::ChangeRequest(i) => {
                work_item.pr_idx = Some(i);
                if work_item.kind != WorkItemKind::Worktree {
                    work_item.kind = WorkItemKind::Pr;
                    work_item.branch = Some(self.change_requests[i].branch.clone());
                    work_item.description = self.change_requests[i].title.clone();
                }
            }
            // ... etc for Session, Issue, Workspace
        }
    }
    // Determine final kind based on what the group contains (priority: Worktree > Session > PR)
    // Add to appropriate section
}
```

**Step 3: Assign to sections and build table entries**

After building WorkItems from groups, partition them into sections (same order as current: Worktrees, Sessions, PRs, Remote Branches, Issues). Remote branches are special — they come from `self.remote_branches` and are NOT passed through the correlation engine (they don't have correlation keys). Handle them separately, filtering out branches already claimed by a group.

Unlinked issues (not part of any group) go into the Issues section.

**Step 4: Handle remote branches (not correlated)**

Remote branches don't go through the correlation engine. After building groups, collect all branches claimed by groups, then filter remote branches as before:

```rust
let known_branches: HashSet<&str> = /* branches from all groups */;
let merged_set: HashSet<&str> = self.merged_branches.iter().map(|s| s.as_str()).collect();
let remote_items: Vec<WorkItem> = self.remote_branches.iter()
    .filter(|b| b != "HEAD" && b != "main" && b != "master"
        && !known_branches.contains(b.as_str())
        && !merged_set.contains(b.as_str()))
    .map(|b| WorkItem {
        kind: WorkItemKind::RemoteBranch,
        branch: Some(b.clone()),
        description: b.clone(),
        ..Default::default()
    })
    .collect();
```

**Step 5: Verify and commit**

Run: `cargo check && cargo test && cargo clippy`
Expected: All pass. The table should render identically for the common cases (branch-based grouping). The new benefit is transitive grouping through non-branch keys.

```bash
git add src/data.rs
git commit -m "refactor: wire correlation engine into DataStore for transitive grouping"
```

---

## Summary

| Task | Priority | Complexity | Lines changed (est.) |
|------|----------|------------|---------------------|
| 1. Per-repo registry | HIGH (blocker) | Medium | ~80 |
| 2. Migrate create_cmux_workspace | Medium | Low | ~-150 |
| 3. Abstract fetch_merged_pr_branches | Low | Low | ~30 |
| 4. Enrich Checkout / replace Worktree | Medium | Medium | ~120 |
| 5. Wire correlation engine | Low (optional) | High | ~250 |

Tasks 1-4 are recommended. Task 5 is optional — the current branch-based correlation works well for existing use cases.
