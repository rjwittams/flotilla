# Remove RepoUiState Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the widget tree migration by eliminating `RepoUiState`, the sync bridge, and the `UiMode` enum — making `RepoPage` the single source of truth for per-repo UI state.

**Architecture:** The dual-write pattern between `RepoUiState` and `RepoPage` is the last vestige of the pre-widget-tree architecture. This plan removes all `RepoUiState` consumers one by one (fragment cascade, has_unseen_changes, pending_actions, old widget methods, old render paths), then deletes the sync bridge, `RepoUiState` itself, and replaces `UiMode` with a simple `is_config: bool`. Each task keeps tests green.

**Tech Stack:** Rust, ratatui, flotilla-tui crate

**Closes:** #431, #433

---

### Task 1: Delete status_fallback_label — cascade already works

The fragment cascade at `Screen::render()` lines 361-369 already correctly resolves fragments from modals, the overview page, and repo pages. The `status_fallback_label()` method (lines 119-153) is a legacy bridge that reads from `RepoUiState` and `UiMode` to compute labels that the fragment cascade already provides. It's used as the fallback in `resolve_status_section()` when `fragment.status` is `None`. Since the cascade already gets the right fragment, the fallback just needs to be `"/ for commands"`.

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/screen.rs:96-98,119-153,382-383`

- [ ] **Step 1: Replace status_fallback_label call with constant default**

In `Screen::render()` at line 382, replace:
```rust
let fallback_label = self.status_fallback_label(ctx);
```
with:
```rust
let fallback_label = "/ for commands";
```

- [ ] **Step 2: Delete status_fallback_label method**

Delete the entire `status_fallback_label()` method (lines 119-153). It has no other callers.

- [ ] **Step 3: Delete active_status_fragment method**

Delete `active_status_fragment()` (lines 96-98) — it's not used in the render path (the cascade at lines 361-369 does the same work inline) and has no other callers.

- [ ] **Step 4: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS — snapshot tests may need updating if status bar content was previously falling through to the legacy path.

- [ ] **Step 5: Commit**

```
refactor: delete status_fallback_label — fragment cascade is sufficient (#431)
```

---

### Task 2: Remove UiMode::IssueSearch

Widgets still set `*ctx.mode = UiMode::IssueSearch { input }` even though the `IssueSearchWidget` owns its own input and provides `status_fragment()`. Remove all writes and the variant itself.

**Files:**
- Modify: `crates/flotilla-tui/src/app/ui_state.rs:28-41` (UiMode enum)
- Modify: `crates/flotilla-tui/src/widgets/repo_page.rs:329` (OpenIssueSearch)
- Modify: `crates/flotilla-tui/src/widgets/work_item_table.rs:454-456` (OpenIssueSearch)
- Modify: `crates/flotilla-tui/src/widgets/command_palette.rs:105` (OpenIssueSearch dispatch)
- Modify: `crates/flotilla-tui/src/widgets/issue_search.rs:51,64` (mode reset on confirm/dismiss)
- Modify: `crates/flotilla-tui/src/keymap.rs:228-236` (BindingModeId::from)
- Modify: various test files

- [ ] **Step 1: Stop setting UiMode::IssueSearch in widgets**

In `repo_page.rs:328-331`, remove the `*ctx.mode = ...` line — just return the Push:
```rust
Action::OpenIssueSearch => {
    Outcome::Push(Box::new(super::issue_search::IssueSearchWidget::new()))
}
```

Same in `work_item_table.rs:454-457` — remove the `*ctx.mode` line.

Same in `command_palette.rs:105` — remove the `*ctx.mode` line.

- [ ] **Step 2: Stop resetting UiMode in IssueSearchWidget**

In `issue_search.rs:51` and `:64`, remove `*ctx.mode = UiMode::Normal`. The widget gets popped from the modal stack via `Outcome::Finished`, which is sufficient.

- [ ] **Step 3: Remove the UiMode::IssueSearch variant**

In `ui_state.rs`, remove the `IssueSearch { input: Input }` variant and the `use tui_input::Input` import (if it's only used for this).

Remove the `BindingModeId::from(&UiMode)` match arm for `IssueSearch` in `keymap.rs:233`.

- [ ] **Step 4: Update tests**

- `ui_state.rs:254-259` — remove `UiMode::IssueSearch` from `is_config` test cases
- `keymap.rs:736` — remove the `BindingModeId::from(&UiMode::IssueSearch{...})` test
- `repo_page.rs:805` — test that checks `harness.mode` is `IssueSearch` after opening search — change to verify an `IssueSearchWidget` was pushed onto the modal stack instead
- `key_handlers.rs:1262,1269` — tests that set `app.ui.mode = UiMode::IssueSearch{...}` — remove or change to test different state

Remove any remaining `tui_input::Input` imports that are now unused.

- [ ] **Step 5: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 6: Commit**

```
refactor: remove UiMode::IssueSearch — widget owns its own state (#431)
```

---

### Task 3: Move has_unseen_changes to TuiRepoModel

`has_unseen_changes` tracks whether an inactive tab has received data updates since last viewed. Move it from `RepoUiState` to `TuiRepoModel` (model-level data).

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:98-115` (TuiRepoModel)
- Modify: `crates/flotilla-tui/src/app/mod.rs:752-755` (handle_repo_snapshot writer)
- Modify: `crates/flotilla-tui/src/app/mod.rs:873-877` (handle_delta writer)
- Modify: `crates/flotilla-tui/src/widgets/tabs.rs:73,77` (render reader)
- Modify: `crates/flotilla-tui/src/widgets/tabs.rs:241` (switch_to clears it)
- Modify: `crates/flotilla-tui/src/app/ui_state.rs:78` (remove field from RepoUiState)
- Modify: various test files

- [ ] **Step 1: Add field to TuiRepoModel and update constructor**

Add `pub has_unseen_changes: bool` to `TuiRepoModel`. Initialize to `false` in the constructor in `handle_repo_added` (mod.rs:929-942).

- [ ] **Step 2: Migrate writers**

In `handle_repo_snapshot` (mod.rs:752): change `self.ui.repo_ui.get_mut(&repo_identity).unwrap().has_unseen_changes = true` to `self.model.repos.get_mut(&repo_identity).unwrap().has_unseen_changes = true`.

Same in `handle_delta` (mod.rs:873).

- [ ] **Step 3: Migrate readers**

In `tabs.rs` render (line 73,77): change `let rui = &ui.repo_ui[repo_identity]` / `rui.has_unseen_changes` to `let rm = &model.repos[repo_identity]` / `rm.has_unseen_changes`.

In `tabs.rs:switch_to` (line 241): change `ui.repo_ui.get_mut(key)...has_unseen_changes = false` to accept `model` as parameter and use `model.repos.get_mut(key)...has_unseen_changes = false`. This requires updating `switch_to`'s signature to take `&mut TuiModel` (it already does).

- [ ] **Step 4: Remove field from RepoUiState**

Delete `pub has_unseen_changes: bool` from `RepoUiState` (ui_state.rs:78).

- [ ] **Step 5: Update tests**

Tests in `mod.rs:1329,1427,1443` that check `app.ui.repo_ui[&repo].has_unseen_changes` → change to `app.model.repos[&repo].has_unseen_changes`.

Tests in `tabs.rs:423,425` and `navigation.rs:129,131` — same migration.

Test in `ui_state.rs:369` — remove `has_unseen_changes` assertion from `repo_ui_state_default` test.

- [ ] **Step 6: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 7: Commit**

```
refactor: move has_unseen_changes from RepoUiState to TuiRepoModel (#433)
```

---

### Task 4: Move pending_actions to RepoPage-only

Eliminate the dual-write pattern for `pending_actions`. Currently both `RepoUiState` and `RepoPage` maintain copies. Make `RepoPage` the single owner.

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:329` (needs_animation)
- Modify: `crates/flotilla-tui/src/app/mod.rs:626-654` (CommandFinished handler)
- Modify: `crates/flotilla-tui/src/app/executor.rs:42-48` (dispatch dual-write)
- Modify: `crates/flotilla-tui/src/app/ui_state.rs:80` (remove field from RepoUiState)

- [ ] **Step 1: Update needs_animation to check RepoPage**

In `mod.rs:329`, change:
```rust
if self.ui.repo_ui.values().any(|rui| rui.pending_actions.values().any(|a| matches!(a.status, PendingStatus::InFlight))) {
```
to:
```rust
if self.screen.repo_pages.values().any(|page| page.pending_actions.values().any(|a| matches!(a.status, PendingStatus::InFlight))) {
```

- [ ] **Step 2: Update CommandFinished handler to use RepoPage**

In `mod.rs:626-654`, change the search and update logic to use `self.screen.repo_pages` instead of `self.ui.repo_ui`:

```rust
let found: Option<(RepoIdentity, WorkItemIdentity)> = self.screen.repo_pages.iter().find_map(|(repo_identity, page)| {
    page.pending_actions
        .iter()
        .find(|(_, a)| a.command_id == command_id)
        .map(|(id, _)| (repo_identity.clone(), id.clone()))
});

if let Some((repo_identity, identity)) = found {
    if let Some(ref message) = error_message {
        if let Some(page) = self.screen.repo_pages.get_mut(&repo_identity) {
            if let Some(entry) = page.pending_actions.get_mut(&identity) {
                entry.status = PendingStatus::Failed(message.clone());
            }
        }
    } else {
        if let Some(page) = self.screen.repo_pages.get_mut(&repo_identity) {
            page.pending_actions.remove(&identity);
        }
    }
}
```

- [ ] **Step 3: Update executor::dispatch to write only to RepoPage**

In `executor.rs:40-49`, remove the `rui` half of the dual-write:
```rust
Ok(command_id) => {
    if let Some(ctx) = pending_ctx {
        let action = PendingAction { command_id, status: PendingStatus::InFlight, description: ctx.description };
        if let Some(page) = app.screen.repo_pages.get_mut(&ctx.repo_identity) {
            page.pending_actions.insert(ctx.identity, action);
        }
    }
}
```

- [ ] **Step 4: Remove pending_actions from RepoUiState**

Delete `pub pending_actions: HashMap<WorkItemIdentity, PendingAction>` from `RepoUiState` (ui_state.rs:80).

Remove `pending_actions` from `RepoUiState::update_table_view()` pruning logic (ui_state.rs:131).

- [ ] **Step 5: Update tests**

Tests in `mod.rs:1698-1797` that set/check `app.ui.repo_ui[&repo].pending_actions` → change to `app.screen.repo_pages[&repo].pending_actions`.

Test in `ui_state.rs:394-447` (`pending_actions_default_is_empty`, `pending_actions_cleaned_on_table_view_update`) — the pruning is now done in `RepoPage::reconcile_if_changed()`. Delete the `RepoUiState` versions if equivalent tests exist on `RepoPage`, or move them to `repo_page.rs` tests.

- [ ] **Step 6: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 7: Commit**

```
refactor: move pending_actions to RepoPage-only, eliminate dual-write (#433)
```

---

### Task 5: Remove RepoUiState dual-writes from process_app_actions

`ToggleProviders` and `ToggleMultiSelect` in `process_app_actions` dual-write to both `RepoPage` and `RepoUiState`. Remove the `RepoUiState` half.

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:508-540` (process_app_actions)

- [ ] **Step 1: Remove ToggleProviders dual-write**

In `mod.rs:508-519`, remove lines 513-518 (the "Keep RepoUiState in sync for status bar" block):
```rust
AppAction::ToggleProviders => {
    let identity = &self.model.repo_order[self.model.active_repo];
    if let Some(page) = self.screen.repo_pages.get_mut(identity) {
        page.show_providers = !page.show_providers;
    }
}
```

- [ ] **Step 2: Remove ToggleMultiSelect dual-write**

In `mod.rs:520-541`, remove lines 532-536 (the rui sync block):
```rust
AppAction::ToggleMultiSelect => {
    let repo_identity = self.model.repo_order[self.model.active_repo].clone();
    if let Some(page) = self.screen.repo_pages.get_mut(&repo_identity) {
        if let Some(si) = page.table.selected_selectable_idx {
            if let Some(&table_idx) = page.table.grouped_items.selectable_indices.get(si) {
                if let Some(flotilla_core::data::GroupEntry::Item(item)) = page.table.grouped_items.table_entries.get(table_idx) {
                    let item_identity = item.identity.clone();
                    if !page.multi_selected.remove(&item_identity) {
                        page.multi_selected.insert(item_identity);
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 4: Commit**

```
refactor: remove RepoUiState dual-writes from process_app_actions (#433)
```

---

### Task 6: Delete old WorkItemTable ctx-based methods, render path, and InteractiveWidget impl

`RepoPage` handles all events using `_self()` methods and renders via `render_table_owned()`. The old `ctx.repo_ui`-based methods, the old `render_table()` (which calls `active_rui()`), and the `InteractiveWidget` impl on `WorkItemTable` are all dead code.

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/work_item_table.rs`

- [ ] **Step 1: Delete old selection helpers**

Delete these ctx-based methods (lines 169-220):
- `select_next(&self, ctx: &mut WidgetContext)`
- `select_prev(&self, ctx: &mut WidgetContext)`
- `toggle_multi_select(&self, ctx: &mut WidgetContext)`

- [ ] **Step 2: Delete old mouse helper and toggle_providers**

Delete:
- `row_at_mouse(&self, x, y, ctx)` at line 408-424
- `toggle_providers(ctx)` at line 428-433

- [ ] **Step 3: Delete old render_table and active_rui**

Delete:
- `render_table(&mut self, model, ui, theme, frame, area)` at line 370-376 — reads from `active_rui()` to get `show_providers`, `multi_selected`, `pending_actions`, then calls `render_table_owned()`. Dead code since RepoPage calls `render_table_owned()` directly.
- `active_rui()` helper at line 544-546 — reads `UiState::active_repo_ui()`. Only caller was `render_table()`.

- [ ] **Step 4: Delete InteractiveWidget impl on WorkItemTable**

Delete the entire `impl InteractiveWidget for WorkItemTable` block (lines 436-534). This includes `handle_action`, `handle_mouse`, `render`, `binding_mode`, `as_any`, `as_any_mut`.

- [ ] **Step 5: Remove unused imports**

Remove `UiMode`, `BranchInputKind`, `AppAction`, `WidgetContext`, `RenderContext`, `Outcome`, `InteractiveWidget`, and any other imports only needed by the deleted code. Keep imports used by `render_table_owned()` and `render_providers()`.

- [ ] **Step 6: Delete or update tests**

WorkItemTable tests that test the old `InteractiveWidget` impl need to be deleted — the equivalent behavior is now tested through RepoPage tests. Keep tests for `select_next_self`, `select_prev_self`, `select_row_self`, `row_at_mouse_self`, `update_items`, and `render_table_owned`.

- [ ] **Step 7: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 8: Commit**

```
refactor: delete dead WorkItemTable ctx-based methods, render path, and InteractiveWidget impl (#433)
```

---

### Task 7: Delete old PreviewPanel render path

`RepoPage` calls `preview.render_with_item()` directly. The old `render_bespoke()` path (called from `InteractiveWidget::render`) reads from `RepoUiState` via `selected_work_item()` and is dead code.

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/preview_panel.rs`

- [ ] **Step 1: Delete the selected_work_item helper**

Delete the standalone `selected_work_item()` function (lines 205-212) that reads from `RepoUiState`.

- [ ] **Step 2: Simplify InteractiveWidget::render**

`PreviewPanel::render()` currently calls `self.render_bespoke()` which uses the deleted helper. Replace it to render nothing (empty), or redirect to `render_with_item` using a placeholder:
```rust
fn render(&mut self, _frame: &mut Frame, _area: Rect, _ctx: &mut RenderContext) {
    // PreviewPanel is rendered by RepoPage via render_with_item().
    // This trait method exists only to satisfy InteractiveWidget but is never called.
}
```

Also delete `render_bespoke` if it exists as a separate method.

- [ ] **Step 3: Remove unused imports**

Remove `UiState`, `GroupEntry`, and any imports only needed by the deleted code.

- [ ] **Step 4: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 5: Commit**

```
refactor: delete old PreviewPanel render path that read from RepoUiState (#433)
```

---

### Task 8: Remove update_table_view calls, sync bridge, and test-only navigation helpers

With all `RepoUiState` readers migrated, `rui.update_table_view()` calls in snapshot/delta handlers are dead (RepoPage's `reconcile_if_changed()` handles table rebuilding). The sync bridge in `key_handlers.rs` is also dead. The `#[cfg(test)]` navigation helpers in `navigation.rs` that use `active_ui()`/`active_ui_mut()` are also dead.

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:758-760,880-882` (update_table_view calls)
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs:120-123,148-151,184-205` (sync bridge)
- Modify: `crates/flotilla-tui/src/app/navigation.rs:35-100` (test-only helpers)

- [ ] **Step 1: Remove update_table_view calls**

In `handle_repo_snapshot` (mod.rs:758-760), delete:
```rust
if let Some(rui) = self.ui.repo_ui.get_mut(&repo_identity) {
    rui.update_table_view(table_view);
}
```

Same in `handle_delta` (mod.rs:880-882).

Note: the `table_view` variable is computed earlier and was only consumed by this call. Check if it's still needed for anything else — if not, also remove the `group_work_items` call that produces it (in `handle_repo_snapshot` and `handle_delta`). The same work is done by `RepoPage::reconcile_if_changed()` via `Shared<RepoData>`.

- [ ] **Step 2: Delete the sync bridge**

In `key_handlers.rs`, delete `sync_repo_page_state()` (lines 184-205) and remove the calls at lines 123 and 151.

- [ ] **Step 3: Delete RepoUiState::update_table_view**

In `ui_state.rs:85-132`, delete the entire `update_table_view` method — it has no callers.

- [ ] **Step 4: Migrate or delete test-only navigation helpers**

In `navigation.rs:35-100`, the `#[cfg(test)]` methods `select_next()`, `select_prev()`, and `row_at_mouse()` use `active_ui()`/`active_ui_mut()`. Either:
- **Migrate** them to use `self.screen.repo_pages` (preferred — tests that call these are testing key dispatch, not RepoUiState), or
- **Delete** them and update the tests in `navigation.rs:265-443` that use them to work through the `handle_key` dispatch path instead.

The simplest migration for `select_next`/`select_prev`: use `self.screen.repo_pages.get_mut(&identity).table.select_next_self()` / `select_prev_self()`. For infinite scroll logic, keep it in the migrated `select_next`.

For `row_at_mouse`: migrate to read `table_state.offset()` and `grouped_items.selectable_indices` from the active RepoPage.

Also update `navigation.rs` tests (lines 265-443) that call `app.active_ui()` to read selection state — change to `app.screen.repo_pages[&identity].table.selected_selectable_idx`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 6: Commit**

```
refactor: remove update_table_view, sync bridge, and migrate test navigation helpers (#433)
```

---

### Task 9: Delete RepoUiState

All consumers are gone. Delete the struct, its field on `UiState`, the `repo_ui` field on `WidgetContext`, and related accessors. Also update integration test harness.

**Files:**
- Modify: `crates/flotilla-tui/src/app/ui_state.rs` (delete RepoUiState, update UiState)
- Modify: `crates/flotilla-tui/src/widgets/mod.rs:100` (remove repo_ui from WidgetContext)
- Modify: `crates/flotilla-tui/src/app/mod.rs` (remove accessors, update constructors)
- Modify: `crates/flotilla-tui/src/app/test_support.rs` (update TestWidgetHarness)
- Modify: `crates/flotilla-tui/tests/support/mod.rs` (update integration TestHarness)
- Modify: `crates/flotilla-tui/tests/snapshots.rs` (update snapshot tests)
- Modify: various files that import RepoUiState

- [ ] **Step 1: Delete RepoUiState struct**

In `ui_state.rs`, delete:
- The `RepoUiState` struct (lines 72-83)
- Its `update_table_view` method (already deleted in task 8)
- All test methods that test RepoUiState behaviour (`repo_ui_state_default`, `pending_actions_default_is_empty`, `pending_actions_cleaned_on_table_view_update`, `active_repo_ui` tests)

- [ ] **Step 2: Remove repo_ui from UiState**

In `ui_state.rs:188-212`:
- Remove `pub repo_ui: HashMap<RepoIdentity, RepoUiState>` from `UiState`
- Remove `repo_ui` initialization from `UiState::new()`
- Delete `active_repo_ui()` method (lines 214-216)

- [ ] **Step 3: Remove repo_ui from WidgetContext**

In `widgets/mod.rs:100`, remove `pub repo_ui: &'a mut HashMap<RepoIdentity, RepoUiState>`.

Update `App::build_widget_context()` (mod.rs:456-470) to not pass `repo_ui`.

- [ ] **Step 4: Remove convenience accessors**

In `mod.rs`, delete:
- `active_ui()` (line 970-972)
- `active_ui_mut()` (line 974-977)

Remove `repo_ui` operations from:
- `handle_repo_added` (line 944) — remove `self.ui.repo_ui.insert(...)`
- `handle_repo_removed` (line 950) — remove `self.ui.repo_ui.remove(...)`

- [ ] **Step 5: Update test_support.rs (unit test harness)**

- Remove `repo_ui` field from `TestWidgetHarness` (line 230)
- Remove `repo_ui` from `TestWidgetHarness::new()` (line 244)
- Remove `repo_ui` from `ctx()` (line 260)
- Simplify `setup_selectable_table()` — only set up via `Shared<RepoData>` and `RepoPage`, no more `RepoUiState` writes
- Delete `set_active_table_view()` (only used for `RepoUiState`)
- Remove `use RepoUiState` imports

- [ ] **Step 6: Update integration test harness**

In `crates/flotilla-tui/tests/support/mod.rs`:
- Remove `UiState::new()` dependency on `repo_ui` (line 39)
- In `with_provider_data()` (lines 144-169): remove the `rui.update_table_view()` call (lines 158-160) — `RepoPage` picks up data via `Shared<RepoData>` and `reconcile_if_changed()`
- Remove `RepoUiState` import

In `crates/flotilla-tui/tests/snapshots.rs`:
- Line 483 and similar: any test that writes `show_providers` to `repo_ui` must write to `screen.repo_pages` instead
- Remove all `RepoUiState` imports

- [ ] **Step 7: Fix all compilation errors across the crate**

Search for remaining references to `repo_ui`, `RepoUiState`, `active_ui`, `active_ui_mut` across `crates/flotilla-tui/` (both `src/` and `tests/`). Most will be in test code that asserted on `RepoUiState` fields — update to check `RepoPage` or `TuiRepoModel` fields instead.

Key areas with many assertions to migrate:
- `app/key_handlers.rs` tests (~30 assertions using `active_ui()` for selection state) → read from `app.screen.repo_pages`
- `app/mod.rs` tests (pending_actions assertions already migrated in Task 4)

- [ ] **Step 8: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 9: Commit**

```
refactor: delete RepoUiState and repo_ui from WidgetContext (#433)
```

---

### Task 10: Replace UiMode with is_config bool

With `IssueSearch` removed (task 2), `UiMode` is just `Normal | Config` — a boolean. Replace it with `is_config: bool` on `UiState`.

**Files:**
- Modify: `crates/flotilla-tui/src/app/ui_state.rs` (delete UiMode, add is_config)
- Modify: `crates/flotilla-tui/src/widgets/mod.rs:101` (WidgetContext)
- Modify: `crates/flotilla-tui/src/widgets/tabs.rs` (all mode reads/writes)
- Modify: `crates/flotilla-tui/src/widgets/overview_page.rs:49` (dismiss)
- Modify: `crates/flotilla-tui/src/widgets/repo_page.rs` (mouse guards)
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs` (resolve_action, dispatch_action)
- Modify: `crates/flotilla-tui/src/app/mod.rs:556` (SwitchToConfig)
- Modify: `crates/flotilla-tui/src/keymap.rs:228-236` (BindingModeId::from)
- Modify: `crates/flotilla-tui/src/app/test_support.rs` (harness)
- Modify: various test files

- [ ] **Step 1: Replace UiMode with is_config on UiState**

In `ui_state.rs`:
- Delete `UiMode` enum and its `impl` block
- On `UiState`, replace `pub mode: UiMode` with `pub is_config: bool`
- In `UiState::new()`, initialize `is_config: false`

- [ ] **Step 2: Replace mode on WidgetContext**

In `widgets/mod.rs:101`, replace `pub mode: &'a mut UiMode` with `pub is_config: &'a mut bool`.

Update `App::build_widget_context()` to pass `is_config: &mut self.ui.is_config`.

- [ ] **Step 3: Migrate tabs.rs**

Replace all `UiMode` usage:
- `ui.mode.is_config()` → `ui.is_config`
- `ui.mode = UiMode::Normal` → `ui.is_config = false`
- `ui.mode = UiMode::Config` → `ui.is_config = true`

- [ ] **Step 4: Migrate overview_page.rs**

Replace `*ctx.mode = UiMode::Normal` → `*ctx.is_config = false`.

- [ ] **Step 5: Migrate repo_page.rs mouse guards**

Replace `matches!(*ctx.mode, UiMode::Normal)` → `!*ctx.is_config`.

- [ ] **Step 6: Migrate key_handlers.rs**

In `resolve_action()`:
```rust
fn resolve_action(&self, key: KeyEvent) -> Option<Action> {
    let mode_id = if self.ui.is_config { BindingModeId::Overview } else { BindingModeId::Normal };
    let mode: KeyBindingMode = mode_id.into();
    self.keymap.resolve(&mode, crokey::KeyCombination::from(key))
}
```

In `dispatch_action()`:
Replace `matches!(self.ui.mode, UiMode::Normal)` → `!self.ui.is_config`.

- [ ] **Step 7: Migrate mod.rs**

In `process_app_actions`:
- Replace `self.ui.mode = UiMode::Config` → `self.ui.is_config = true` (SwitchToConfig at ~line 556)
- Replace `self.ui.mode.is_config()` → `self.ui.is_config` (MoveTabLeft/MoveTabRight guards at ~lines 577,582)

- [ ] **Step 8: Delete BindingModeId::from(&UiMode)**

In `keymap.rs:228-236`, delete the `From<&UiMode>` impl entirely — it's replaced by the inline logic in `resolve_action()`.

- [ ] **Step 9: Update TestWidgetHarness**

Replace `pub mode: UiMode` → `pub is_config: bool`.
Initialize as `is_config: false`.
In `ctx()`, pass `is_config: &mut self.is_config`.

- [ ] **Step 10: Update all tests (unit and integration)**

Search for `UiMode::Normal`, `UiMode::Config`, `harness.mode`, `app.ui.mode` across all test code in both `src/` and `tests/` and replace:
- `app.ui.mode = UiMode::Config` → `app.ui.is_config = true`
- `assert!(matches!(app.ui.mode, UiMode::Normal))` → `assert!(!app.ui.is_config)`
- `assert!(matches!(app.ui.mode, UiMode::Config))` → `assert!(app.ui.is_config)`
- `app.ui.mode.is_config()` → `app.ui.is_config`
- `harness.mode = UiMode::Config` → `harness.is_config = true`

Key files with many UiMode references:
- `app/key_handlers.rs` tests (~15 references)
- `app/navigation.rs` tests (~10 references)
- `widgets/tabs.rs` tests (~8 references)
- `widgets/overview_page.rs` tests (~3 references)
- `tests/support/mod.rs` — `with_mode(UiMode)` → `with_config_mode(bool)` or similar
- `tests/snapshots.rs` — `UiMode::Config` in snapshot setup (~2 references). Also remove `UiMode::IssueSearch` reference at ~line 364 (should already be gone from Task 2).

- [ ] **Step 11: Remove re-export**

In `mod.rs:30`, remove `UiMode` from the `pub use ui_state::...` line.

Clean up unused `tui_input::Input` imports if they were only needed by `UiMode::IssueSearch`.

- [ ] **Step 12: Run tests**

Run: `cargo test -p flotilla-tui --locked`
Expected: PASS

- [ ] **Step 13: Run CI checks**

Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS

- [ ] **Step 14: Commit**

```
refactor: replace UiMode enum with is_config bool (#433)
```

---

### Final: Verify and format

- [ ] **Step 1: Run full CI suite**

```bash
cargo +nightly-2026-03-12 fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

- [ ] **Step 2: Apply formatting if needed**

```bash
cargo +nightly-2026-03-12 fmt
```

- [ ] **Step 3: Final commit if formatting changed**

```
chore: apply rustfmt
```
