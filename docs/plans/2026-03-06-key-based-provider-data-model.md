# Key-Based Provider Data Model Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace index-based `Vec<T>` provider collections with `IndexMap<K, V>`, propagating keyed references through correlation, WorkItem, selection, change detection, and rendering.

**Architecture:** `ProviderData` collections become `IndexMap` keyed by natural identity. `CorrelatedItem` carries a `ProviderItemKey` instead of `source_index`. `WorkItem` stores `Option<Key>` instead of `Option<usize>`. Selection persists across refresh via `WorkItemIdentity`. Change detection uses `PartialEq` on provider data.

**Tech Stack:** Rust, `indexmap` (already in Cargo.toml), ratatui

---

### Task 1: Add PartialEq derives to provider types

**Files:**
- Modify: `src/providers/types.rs`

**Step 1: Add PartialEq + Eq derives to all provider item types**

Add `PartialEq, Eq` to the derive macros on: `Checkout`, `ChangeRequest`, `Issue`, `CloudAgentSession`, `Workspace`, `AheadBehind`, `WorkingTreeStatus`, `CommitInfo`, `ChangeRequestStatus`, `SessionStatus`, and any other types these structs contain.

Check each field type supports `PartialEq`. `PathBuf`, `String`, `Option<T>`, `Vec<T>` all do.

**Step 2: Add PartialEq derive to ProviderData**

In `src/provider_data.rs`, add `PartialEq, Eq` to `ProviderData`'s derive.

**Step 3: Run `cargo check`**

Expected: compiles cleanly. If any field type doesn't support `PartialEq`, fix it.

**Step 4: Commit**

```
feat: derive PartialEq on provider types and ProviderData
```

---

### Task 2: Add ProviderItemKey enum and convert CorrelatedItem

**Files:**
- Modify: `src/providers/correlation.rs`

**Step 1: Write test for ProviderItemKey in CorrelatedItem**

Add a test in the existing `#[cfg(test)]` module that creates a `CorrelatedItem` with a `ProviderItemKey::Checkout(PathBuf)` source_key and verifies it round-trips through correlation. Adapt one of the existing tests (e.g. `single_checkout_standalone`) to use `source_key` instead of `source_index`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla -- correlation`
Expected: compilation failure — `source_key` field doesn't exist yet.

**Step 3: Define ProviderItemKey and update CorrelatedItem**

In `src/providers/correlation.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderItemKey {
    Checkout(PathBuf),
    ChangeRequest(String),
    Session(String),
    Workspace(String),
}

pub struct CorrelatedItem {
    pub provider_name: String,
    pub kind: ItemKind,
    pub title: String,
    pub correlation_keys: Vec<CorrelationKey>,
    pub source_key: ProviderItemKey,  // replaces source_index: usize
}
```

Update all test code that constructs `CorrelatedItem` to use `source_key` instead of `source_index`. Each test item needs a `ProviderItemKey` matching its kind.

**Step 4: Run tests**

Run: `cargo test -p flotilla -- correlation`
Expected: all correlation tests pass.

**Step 5: Fix compilation in data.rs**

`src/data.rs` references `item.source_index` — update Phase 1 in `correlate()` (lines 173-211) to emit `source_key` instead:

```rust
// Checkouts
for (_, co) in providers.checkouts.iter() {  // will be IndexMap iter later; for now Vec
    items.push(CorrelatedItem {
        provider_name: "checkout".to_string(),
        kind: CorItemKind::Checkout,
        title: co.branch.clone(),
        correlation_keys: co.correlation_keys.clone(),
        source_key: ProviderItemKey::Checkout(co.path.clone()),
    });
}
```

Since `ProviderData` is still `Vec` at this point, iterate with `.iter()` and extract keys from item fields:
- Checkouts: `co.path.clone()`
- ChangeRequests: `cr.id.clone()`
- Sessions: `session.id.clone()`
- Workspaces: `ws.ws_ref.clone()`

**Step 6: Update group_to_work_item to use source_key**

In `group_to_work_item()` (lines 97-165), change `item.source_index` references to match on `item.source_key`:

```rust
CorItemKind::Checkout => {
    if let ProviderItemKey::Checkout(ref path) = item.source_key {
        worktree_idx = Some(path.clone());  // temporarily store PathBuf
        // lookup: providers.checkouts.iter().find(|co| co.path == *path)
    }
}
```

Note: This is a transitional step. `worktree_idx` type changes happen in Task 3. For now, find by linear scan since collections are still Vec. The point is to stop relying on `source_index`.

Actually — to avoid a messy transitional state, do Tasks 2 and 3 together. See Task 3.

**Step 7: Run `cargo check` and `cargo test`**

Expected: compiles and all tests pass.

**Step 8: Commit**

```
refactor: replace source_index with ProviderItemKey in CorrelatedItem
```

---

### Task 3: Convert ProviderData collections to IndexMap

**Files:**
- Modify: `src/provider_data.rs`
- Modify: `src/data.rs` — `correlate()`, `group_to_work_item()`, `build_table_view()`
- Modify: `src/refresh.rs` — snapshot building

**Step 1: Change ProviderData to use IndexMap**

```rust
use std::path::PathBuf;
use indexmap::IndexMap;
use crate::providers::types::*;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProviderData {
    pub checkouts: IndexMap<PathBuf, Checkout>,
    pub change_requests: IndexMap<String, ChangeRequest>,
    pub issues: IndexMap<String, Issue>,
    pub sessions: IndexMap<String, CloudAgentSession>,
    pub remote_branches: Vec<String>,
    pub merged_branches: Vec<String>,
    pub workspaces: IndexMap<String, Workspace>,
}
```

**Step 2: Run `cargo check` — expect many errors**

This will break everywhere that pushes to or indexes into these collections. Fix each call site systematically.

**Step 3: Fix refresh.rs — provider population**

In `src/refresh.rs`, where provider results are assigned (lines ~183-189), convert `Vec<T>` results to `IndexMap<K, T>`:

```rust
pd.checkouts = checkouts.unwrap_or_else(|e| { errors.push(...); Vec::new() })
    .into_iter()
    .map(|co| (co.path.clone(), co))
    .collect();

pd.change_requests = crs.unwrap_or_else(...)
    .into_iter()
    .map(|cr| (cr.id.clone(), cr))
    .collect();

pd.issues = issues.unwrap_or_else(...)
    .into_iter()
    .map(|issue| (issue.id.clone(), issue))
    .collect();

pd.sessions = sessions.unwrap_or_else(...)
    .into_iter()
    .map(|s| (s.id.clone(), s))
    .collect();

pd.workspaces = workspaces.unwrap_or_else(...)
    .into_iter()
    .map(|ws| (ws.ws_ref.clone(), ws))
    .collect();
```

`remote_branches` and `merged_branches` stay as `Vec<String>` — no change.

**Step 4: Fix data.rs correlate() Phase 1**

The `for (i, co) in providers.checkouts.iter().enumerate()` loops become:

```rust
for (path, co) in &providers.checkouts {
    items.push(CorrelatedItem {
        ...
        source_key: ProviderItemKey::Checkout(path.clone()),
    });
}
```

Same pattern for `change_requests`, `sessions`, `workspaces` — iterate over `(key, value)` pairs.

**Step 5: Fix data.rs group_to_work_item()**

Index lookups become key lookups. The WorkItem fields are still `Option<usize>` at this point — we'll change them in Task 4. For this transitional step, we can temporarily use `IndexMap::get_index_of()` to convert keys back to indices if needed, BUT it's cleaner to do Task 4 simultaneously. **Combine this step with Task 4.**

**Step 6: Fix data.rs post-correlation issue linking (lines 227-239)**

```rust
if let Some(pr_key) = &work_item.pr_key {
    if let Some(cr) = providers.change_requests.get(pr_key.as_str()) {
        for key in &cr.association_keys {
            let AssociationKey::IssueRef(_, issue_id) = key;
            if providers.issues.contains_key(issue_id.as_str()) {
                if !work_item.issue_keys.contains(issue_id) {
                    work_item.issue_keys.push(issue_id.clone());
                }
            }
        }
    }
}
```

Note: `linked_issue_indices: HashSet<usize>` becomes `linked_issue_keys: HashSet<String>`.

**Step 7: Fix standalone issues (lines 244-260)**

```rust
for (id, issue) in &providers.issues {
    if !linked_issue_keys.contains(id.as_str()) {
        work_items.push(WorkItem {
            kind: WorkItemKind::Issue,
            ...
            issue_keys: vec![id.clone()],
            ...
        });
    }
}
```

**Step 8: Fix build_table_view() sorting**

Sorting functions that look up provider data by index need to use keys:

```rust
// Sessions sort (lines 324-327)
session_items.sort_by(|a, b| {
    let a_time = a.session_key.as_ref()
        .and_then(|k| providers.sessions.get(k.as_str()))
        .and_then(|s| s.updated_at.as_deref());
    let b_time = b.session_key.as_ref()
        .and_then(|k| providers.sessions.get(k.as_str()))
        .and_then(|s| s.updated_at.as_deref());
    b_time.cmp(&a_time)
});

// PR sort (lines 338-341)
pr_items.sort_by(|a, b| {
    let a_num = a.pr_key.as_ref()
        .and_then(|k| k.parse::<i64>().ok());
    let b_num = b.pr_key.as_ref()
        .and_then(|k| k.parse::<i64>().ok());
    b_num.cmp(&a_num)
});

// Issue sort (lines 362-365)
issue_items.sort_by(|a, b| {
    let a_num = a.issue_keys.first().and_then(|k| k.parse::<i64>().ok());
    let b_num = b.issue_keys.first().and_then(|k| k.parse::<i64>().ok());
    b_num.cmp(&a_num)
});
```

**Step 9: Run `cargo check`**

Expected: still errors in intent.rs, executor.rs, ui.rs, main.rs, command.rs — those are Task 4-6.

**Step 10: Commit (if compiles) or continue to Task 4**

If compilation is blocked by downstream consumers, combine with Task 4 into one commit.

---

### Task 4: Convert WorkItem fields from indices to keys

**Files:**
- Modify: `src/data.rs` — `WorkItem` struct, `group_to_work_item()`
- Modify: `src/app/command.rs` — `Command` enum
- Modify: `src/app/intent.rs` — `is_available()`, `resolve()`
- Modify: `src/app/executor.rs` — all command handlers
- Modify: `src/app/mod.rs` — multi-select action handler

**Step 1: Change WorkItem fields**

```rust
pub struct WorkItem {
    pub kind: WorkItemKind,
    pub branch: Option<String>,
    pub description: String,
    pub checkout_key: Option<PathBuf>,       // was worktree_idx: Option<usize>
    pub is_main_worktree: bool,
    pub pr_key: Option<String>,              // was pr_idx: Option<usize>
    pub session_key: Option<String>,         // was session_idx: Option<usize>
    pub issue_keys: Vec<String>,             // was issue_idxs: Vec<usize>
    pub workspace_refs: Vec<String>,         // unchanged
    pub correlation_group_idx: Option<usize>, // unchanged (debug only)
}
```

**Step 2: Update group_to_work_item()**

```rust
fn group_to_work_item(providers: &ProviderData, group: &CorrelatedGroup, group_idx: usize) -> Option<WorkItem> {
    let mut checkout_key: Option<PathBuf> = None;
    let mut pr_key: Option<String> = None;
    let mut session_key: Option<String> = None;
    let mut workspace_refs: Vec<String> = Vec::new();
    let mut is_main_worktree = false;

    for item in &group.items {
        match (&item.kind, &item.source_key) {
            (CorItemKind::Checkout, ProviderItemKey::Checkout(path)) => {
                if checkout_key.is_none() {  // keep first, not last (fixes C1)
                    checkout_key = Some(path.clone());
                    if let Some(co) = providers.checkouts.get(path) {
                        is_main_worktree = co.is_trunk;
                    }
                }
            }
            (CorItemKind::ChangeRequest, ProviderItemKey::ChangeRequest(id)) => {
                pr_key = Some(id.clone());
            }
            (CorItemKind::CloudSession, ProviderItemKey::Session(id)) => {
                if session_key.is_none() {
                    session_key = Some(id.clone());
                }
            }
            (CorItemKind::Workspace, ProviderItemKey::Workspace(ws_ref)) => {
                workspace_refs.push(ws_ref.clone());
            }
            _ => {}
        }
    }

    let kind = if checkout_key.is_some() {
        WorkItemKind::Checkout
    } else if pr_key.is_some() {
        WorkItemKind::Pr
    } else if session_key.is_some() {
        WorkItemKind::Session
    } else {
        return None;
    };

    let branch = group.branch().map(|s| s.to_string());

    let pr_title = pr_key.as_ref()
        .and_then(|k| providers.change_requests.get(k.as_str()))
        .map(|cr| cr.title.clone())
        .filter(|t| !t.is_empty());
    let session_title = session_key.as_ref()
        .and_then(|k| providers.sessions.get(k.as_str()))
        .map(|s| s.title.clone())
        .filter(|t| !t.is_empty());
    let description = pr_title
        .or(session_title)
        .or_else(|| branch.clone())
        .unwrap_or_default();

    Some(WorkItem {
        kind, branch, description,
        checkout_key, is_main_worktree,
        pr_key, session_key,
        issue_keys: Vec::new(),
        workspace_refs,
        correlation_group_idx: Some(group_idx),
    })
}
```

**Step 3: Update Command enum**

In `src/app/command.rs`:

```rust
pub enum Command {
    SwitchWorktree(PathBuf),              // was usize
    SelectWorkspace(String),               // unchanged
    CreateWorktree { branch: String, create_branch: bool },  // unchanged
    FetchDeleteInfo(usize),                // keep as selectable_idx for now
    ConfirmDelete,
    OpenPr(String),                        // unchanged (already uses PR id)
    OpenIssueBrowser(String),              // unchanged (already uses issue id)
    ArchiveSession(String),                // was usize, now session id
    GenerateBranchName(Vec<String>),       // was Vec<usize>, now issue ids
    TeleportSession { session_id: String, branch: Option<String>, checkout_key: Option<PathBuf> }, // was worktree_idx
    AddRepo(PathBuf),
}
```

**Step 4: Update intent.rs is_available()**

```rust
pub fn is_available(&self, item: &WorkItem) -> bool {
    match self {
        Intent::SwitchToWorkspace => !item.workspace_refs.is_empty(),
        Intent::CreateWorkspace => item.checkout_key.is_some() && item.workspace_refs.is_empty(),
        Intent::RemoveWorktree => item.checkout_key.is_some() && !item.is_main_worktree,
        Intent::CreateWorktreeAndWorkspace => item.checkout_key.is_none() && item.branch.is_some(),
        Intent::GenerateBranchName => item.branch.is_none() && !item.issue_keys.is_empty(),
        Intent::OpenPr => item.pr_key.is_some(),
        Intent::OpenIssue => !item.issue_keys.is_empty(),
        Intent::TeleportSession => item.session_key.is_some(),
        Intent::ArchiveSession => item.session_key.is_some(),
    }
}
```

**Step 5: Update intent.rs resolve()**

```rust
Intent::CreateWorkspace => {
    item.checkout_key.clone().map(Command::SwitchWorktree)
}
Intent::GenerateBranchName => {
    if !item.issue_keys.is_empty() {
        Some(Command::GenerateBranchName(item.issue_keys.clone()))
    } else { None }
}
Intent::OpenPr => {
    item.pr_key.as_ref().map(|k| Command::OpenPr(k.clone()))
}
Intent::OpenIssue => {
    item.issue_keys.first().map(|k| Command::OpenIssueBrowser(k.clone()))
}
Intent::TeleportSession => {
    item.session_key.as_ref().map(|k| {
        Command::TeleportSession {
            session_id: k.clone(),
            branch: item.branch.clone(),
            checkout_key: item.checkout_key.clone(),
        }
    })
}
Intent::ArchiveSession => {
    item.session_key.clone().map(Command::ArchiveSession)
}
```

Note: `OpenPr` and `OpenIssue` no longer need to look up the provider data to get the id — the key IS the id. This simplifies the resolve methods.

**Step 6: Update executor.rs**

Each command handler changes from index lookup to key lookup:

```rust
Command::SwitchWorktree(path) => {
    if let Some(co) = app.model.active().data.providers.checkouts.get(&path).cloned() {
        // ... rest unchanged
    }
}

Command::FetchDeleteInfo(si) => {
    // ... get item from table as before ...
    let wt_path = item.checkout_key.as_ref()
        .and_then(|k| app.model.active().data.providers.checkouts.get(k))
        .map(|co| co.path.clone());
    let pr_id = item.pr_key.clone();
    // ... rest unchanged
}

Command::ArchiveSession(session_id) => {
    if let Some(session) = app.model.active().data.providers.sessions.get(session_id.as_str()).cloned() {
        // ... rest unchanged
    }
}

Command::TeleportSession { session_id, branch, checkout_key } => {
    let wt_path = if let Some(ref key) = checkout_key {
        app.model.active().data.providers.checkouts.get(key).map(|co| co.path.clone())
    } else if let Some(branch_name) = &branch {
        // ... create checkout as before
    } else { None };
    // ... rest unchanged
}

Command::GenerateBranchName(issue_keys) => {
    let issues: Vec<(String, String)> = issue_keys.iter()
        .filter_map(|k| app.model.active().data.providers.issues.get(k.as_str()))
        .map(|issue| (issue.id.clone(), issue.title.clone()))
        .collect();
    // ... rest unchanged
}
```

**Step 7: Update multi-select action handler in app/mod.rs**

The `action_enter_multi_select` function (around line 394) collects `issue_idxs` — change to `issue_keys`:

```rust
let mut all_issue_keys: Vec<String> = Vec::new();
for &si in &multi_selected {
    if let Some(&table_idx) = self.active_ui().table_view.selectable_indices.get(si) {
        if let Some(TableEntry::Item(item)) = self.active_ui().table_view.table_entries.get(table_idx) {
            all_issue_keys.extend(item.issue_keys.iter().cloned());
        }
    }
}
// deduplicate
all_issue_keys.sort();
all_issue_keys.dedup();
self.commands.push(Command::GenerateBranchName(all_issue_keys));
```

**Step 8: Run `cargo check`**

Expected: may still have errors in ui.rs — proceed to Task 5.

---

### Task 5: Update UI rendering to use keyed lookups

**Files:**
- Modify: `src/ui.rs` — all index-based provider lookups

**Step 1: Update table row rendering**

Every place in `src/ui.rs` that does `.get(idx)` on provider collections changes to `.get(key)`:

```rust
// Session lookup (was: item.session_idx.and_then(|idx| data.providers.sessions.get(idx)))
let session = item.session_key.as_ref()
    .and_then(|k| data.providers.sessions.get(k.as_str()));

// PR lookup (was: data.providers.change_requests.get(pr_idx))
let cr = item.pr_key.as_ref()
    .and_then(|k| data.providers.change_requests.get(k.as_str()));

// Issue lookup (was: filter_map(|&idx| data.providers.issues.get(idx)))
let issues: Vec<_> = item.issue_keys.iter()
    .filter_map(|k| data.providers.issues.get(k.as_str()))
    .collect();

// Checkout lookup (was: providers.checkouts.get(wt_idx))
let checkout = item.checkout_key.as_ref()
    .and_then(|k| data.providers.checkouts.get(k));
```

**Step 2: Update preview panel rendering**

Same pattern for the detail/preview panel (around lines 505-570).

**Step 3: Run `cargo check` and `cargo test`**

Expected: compiles and all tests pass.

**Step 4: Commit Tasks 3-5 together**

```
feat: replace index-based provider data with IndexMap keyed collections

ProviderData collections are now IndexMap<K, V> keyed by natural identity.
WorkItem stores keys instead of indices. All lookups go through map.get().
Fixes C1 (multi-checkout overwrite) by keeping first checkout in group.
```

---

### Task 6: Change detection with PartialEq

**Files:**
- Modify: `src/main.rs` — `drain_snapshots()` lines 264-282

**Step 1: Replace length-tuple comparison with PartialEq**

```rust
// Was:
// let old_snapshot = (old_providers.checkouts.len(), ...);
// let new_snapshot = (rm.data.providers.checkouts.len(), ...);
// if i != *active_repo && old_snapshot != new_snapshot {

// Now:
if i != *active_repo && *old_providers != *rm.data.providers {
    if let Some(rui) = ui.repo_ui.get_mut(path) {
        rui.has_unseen_changes = true;
    }
}
```

**Step 2: Run `cargo test`**

Expected: passes. Change detection now catches content changes (C4 fix).

**Step 3: Commit**

```
fix: detect content changes in provider data, not just count changes

Derives PartialEq on ProviderData and item types. Change detection
badge now fires on title changes, status flips, etc. Fixes C4.
```

---

### Task 7: Selection persistence via WorkItemIdentity

**Files:**
- Modify: `src/data.rs` — add `WorkItemIdentity` enum and `WorkItem::identity()`
- Modify: `src/app/ui_state.rs` — `multi_selected` type
- Modify: `src/main.rs` — selection restore in `drain_snapshots()`
- Modify: `src/app/mod.rs` — multi-select toggle, multi-select action

**Step 1: Define WorkItemIdentity**

In `src/data.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkItemIdentity {
    Checkout(PathBuf),
    ChangeRequest(String),
    Session(String),
    Issue(String),
    RemoteBranch(String),
}

impl WorkItem {
    pub fn identity(&self) -> Option<WorkItemIdentity> {
        match self.kind {
            WorkItemKind::Checkout => self.checkout_key.clone().map(WorkItemIdentity::Checkout),
            WorkItemKind::Pr => self.pr_key.clone().map(WorkItemIdentity::ChangeRequest),
            WorkItemKind::Session => self.session_key.clone().map(WorkItemIdentity::Session),
            WorkItemKind::Issue => self.issue_keys.first().cloned().map(WorkItemIdentity::Issue),
            WorkItemKind::RemoteBranch => self.branch.clone().map(WorkItemIdentity::RemoteBranch),
        }
    }
}
```

**Step 2: Write test for identity**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_identity_checkout() {
        let wi = WorkItem {
            kind: WorkItemKind::Checkout,
            checkout_key: Some(PathBuf::from("/tmp/foo")),
            ..default_work_item()
        };
        assert_eq!(wi.identity(), Some(WorkItemIdentity::Checkout(PathBuf::from("/tmp/foo"))));
    }

    #[test]
    fn work_item_identity_issue() {
        let wi = WorkItem {
            kind: WorkItemKind::Issue,
            issue_keys: vec!["42".to_string()],
            ..default_work_item()
        };
        assert_eq!(wi.identity(), Some(WorkItemIdentity::Issue("42".to_string())));
    }

    fn default_work_item() -> WorkItem {
        WorkItem {
            kind: WorkItemKind::Checkout,
            branch: None,
            description: String::new(),
            checkout_key: None,
            is_main_worktree: false,
            pr_key: None,
            session_key: None,
            issue_keys: Vec::new(),
            workspace_refs: Vec::new(),
            correlation_group_idx: None,
        }
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p flotilla -- work_item_identity`
Expected: passes.

**Step 4: Update multi_selected type**

In `src/app/ui_state.rs`, change:

```rust
pub multi_selected: HashSet<WorkItemIdentity>,
```

Add `use crate::data::WorkItemIdentity;` and `use std::collections::HashSet;`.

**Step 5: Update multi-select toggle in app/mod.rs**

The `toggle_multi_select()` function currently inserts/removes a `usize` (selectable index). Change to insert/remove `WorkItemIdentity`:

```rust
fn toggle_multi_select(&mut self) {
    if let Some(si) = self.active_ui().selected_selectable_idx {
        if let Some(&table_idx) = self.active_ui().table_view.selectable_indices.get(si) {
            if let Some(TableEntry::Item(item)) = self.active_ui().table_view.table_entries.get(table_idx) {
                if let Some(identity) = item.identity() {
                    let rui = self.active_ui_mut();
                    if !rui.multi_selected.remove(&identity) {
                        rui.multi_selected.insert(identity);
                    }
                }
            }
        }
    }
}
```

**Step 6: Update multi-select action handler**

The `action_enter_multi_select` in `app/mod.rs` iterates `multi_selected`. Now it needs to find items by identity instead of selectable index. Change to iterate table entries and check membership:

```rust
fn action_enter_multi_select(&mut self) {
    let multi_selected = self.active_ui().multi_selected.clone();
    let mut all_issue_keys: Vec<String> = Vec::new();
    for entry in &self.active_ui().table_view.table_entries {
        if let TableEntry::Item(item) = entry {
            if let Some(identity) = item.identity() {
                if multi_selected.contains(&identity) {
                    all_issue_keys.extend(item.issue_keys.iter().cloned());
                }
            }
        }
    }
    all_issue_keys.sort();
    all_issue_keys.dedup();
    // ... rest of function
}
```

**Step 7: Update multi-select check in ui.rs**

Replace the O(n²) linear scan with identity-based HashSet lookup:

```rust
// Was:
// let is_multi_selected = rui.table_view.selectable_indices.iter()
//     .position(|&idx| idx == table_idx)
//     .map(|si| rui.multi_selected.contains(&si))
//     .unwrap_or(false);

// Now:
let is_multi_selected = if let TableEntry::Item(item) = entry {
    item.identity()
        .map(|id| rui.multi_selected.contains(&id))
        .unwrap_or(false)
} else {
    false
};
```

**Step 8: Update selection restore in drain_snapshots()**

In `src/main.rs`, replace index clamping with identity-based restore:

```rust
if let Some(rui) = ui.repo_ui.get_mut(path) {
    // Save current selection identity
    let prev_identity = rui.selected_selectable_idx
        .and_then(|si| rui.table_view.selectable_indices.get(si).copied())
        .and_then(|ti| match rui.table_view.table_entries.get(ti) {
            Some(TableEntry::Item(item)) => item.identity(),
            _ => None,
        });

    rui.table_view = table_view;

    // Restore selection by identity
    if rui.table_view.selectable_indices.is_empty() {
        rui.selected_selectable_idx = None;
        rui.table_state.select(None);
    } else if let Some(ref identity) = prev_identity {
        // Find the selectable index matching this identity
        let found = rui.table_view.selectable_indices.iter().enumerate().find(|(_, &ti)| {
            matches!(
                rui.table_view.table_entries.get(ti),
                Some(TableEntry::Item(item)) if item.identity().as_ref() == Some(identity)
            )
        });
        if let Some((si, &ti)) = found {
            rui.selected_selectable_idx = Some(si);
            rui.table_state.select(Some(ti));
        } else {
            // Item was removed — select first
            rui.selected_selectable_idx = Some(0);
            rui.table_state.select(Some(rui.table_view.selectable_indices[0]));
        }
    } else {
        rui.selected_selectable_idx = Some(0);
        rui.table_state.select(Some(rui.table_view.selectable_indices[0]));
    }

    // Clear multi-select identities that no longer exist
    let current_identities: HashSet<WorkItemIdentity> = rui.table_view.table_entries.iter()
        .filter_map(|e| match e {
            TableEntry::Item(item) => item.identity(),
            _ => None,
        })
        .collect();
    rui.multi_selected.retain(|id| current_identities.contains(id));
}
```

**Step 9: Update any other places that clear/check multi_selected**

Search for `multi_selected` references in `app/mod.rs` — ESC handler clears it, etc. These should work with the new type since `.clear()` and `.is_empty()` are the same on HashSet.

**Step 10: Run `cargo check` and `cargo test`**

Expected: compiles and all tests pass.

**Step 11: Commit**

```
feat: persist selection across refresh via WorkItemIdentity

Selection and multi-selection now survive data refresh by storing
WorkItemIdentity (keyed by natural item ID) instead of positional
indices. Multi-select check in render is O(1) via HashSet.
Fixes C3 (selection drift) and P1 (O(n²) multi-select lookup).
```

---

### Task 8: Final verification and cleanup

**Step 1: Run full test suite**

Run: `cargo test`

**Step 2: Run clippy**

Run: `cargo clippy`

Fix any warnings.

**Step 3: Run the app manually**

Run: `cargo run` in a repo directory. Verify:
- Items display correctly in all sections
- Selection works (j/k navigation)
- Selection persists when data refreshes (wait for auto-refresh or press `r`)
- Multi-select works (Space to toggle)
- Actions work (Enter, p for PR, d for delete)
- Unseen-changes badge appears on inactive tabs when data changes

**Step 4: Commit any cleanup**

```
chore: clippy fixes for key-based provider data model
```
