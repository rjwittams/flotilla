# TUI Performance Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate redundant re-correlation on the TUI side and batch-process the event loop to prevent scroll stalls.

**Architecture:** Two independent changes: (1) add `work_items` to `SnapshotDelta` so the TUI skips correlation, and (2) restructure the event loop to drain+coalesce pending events and draw once per batch. Both changes are independent and can be done in either order.

**Tech Stack:** Rust, ratatui, crossterm, tokio mpsc channels

---

### Task 1: Add work_items field to SnapshotDelta

**Files:**
- Modify: `crates/flotilla-protocol/src/lib.rs:157-166` (SnapshotDelta struct)

**Step 1: Add the field**

Add `work_items: Vec<WorkItem>` to `SnapshotDelta` after `changes`:

```rust
pub struct SnapshotDelta {
    pub seq: u64,
    pub prev_seq: u64,
    pub repo: std::path::PathBuf,
    pub changes: Vec<Change>,
    /// Pre-correlated work items from the daemon (avoids re-correlation on TUI side).
    pub work_items: Vec<snapshot::WorkItem>,
    /// Issue metadata (not part of delta log, but needed by TUI).
    pub issue_total: Option<u32>,
    pub issue_has_more: bool,
    pub issue_search_results: Option<Vec<(String, Issue)>>,
}
```

**Step 2: Fix the roundtrip test**

Update `snapshot_delta_event_roundtrip` test at line 309 to include `work_items: vec![]`.

**Step 3: Build and verify**

Run: `cargo build 2>&1 | head -40`

This will produce compile errors in the two `SnapshotDelta` construction sites in `in_process.rs` — that's expected, we fix them in Task 2.

**Step 4: Commit**

```
feat: add work_items field to SnapshotDelta protocol type
```

---

### Task 2: Populate work_items in daemon SnapshotDelta construction

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs:116-124` (choose_event)
- Modify: `crates/flotilla-core/src/in_process.rs:1009-1027` (replay_since delta path)

**Step 1: Update choose_event**

At line 116, add `work_items` from the snapshot:

```rust
    let snapshot_delta = flotilla_protocol::SnapshotDelta {
        seq: delta.seq,
        prev_seq: delta.prev_seq,
        repo: snapshot.repo.clone(),
        changes: delta.changes,
        work_items: snapshot.work_items.clone(),
        issue_total: snapshot.issue_total,
        issue_has_more: snapshot.issue_has_more,
        issue_search_results: snapshot.issue_search_results.clone(),
    };
```

**Step 2: Simplify replay_since**

The delta log (`DeltaEntry`) doesn't store work_items, and replay is rare (reconnect only). Replace the delta replay loop at lines 1009-1027 with a full snapshot fallback:

```rust
                    if let Some(_start_idx) = replay_start {
                        // Delta log entries don't carry work_items, so replay
                        // as full snapshot instead of sending incomplete deltas.
                        events.push(DaemonEvent::SnapshotFull(Box::new(snapshot())));
                    } else if client_seq == state.seq {
```

**Step 3: Build and test**

Run: `cargo build && cargo test --locked 2>&1 | tail -20`

Expected: all tests pass, no compile errors.

**Step 4: Commit**

```
feat: populate work_items in SnapshotDelta, simplify replay_since
```

---

### Task 3: Remove re-correlation from TUI apply_delta

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:278-373` (apply_delta method)

**Step 1: Replace the re-correlation block with direct work_items usage**

Delete lines 319-326 (the `data::correlate` + `correlation_result_to_work_item` block) and use `delta.work_items` directly. The new apply_delta body from the table-build section onward:

```rust
    fn apply_delta(&mut self, delta: SnapshotDelta) {
        let path = delta.repo;
        let rm = match self.model.repos.get_mut(&path) {
            Some(rm) => rm,
            None => return,
        };

        // Apply provider data changes
        let mut providers = (*rm.providers).clone();
        flotilla_core::delta::apply_changes(&mut providers, delta.changes.clone());
        rm.providers = Arc::new(providers);

        // Update issue metadata
        rm.issue_has_more = delta.issue_has_more;
        rm.issue_total = delta.issue_total;
        rm.issue_search_active = delta.issue_search_results.is_some();
        rm.issue_fetch_pending = false;

        // Apply provider health and error changes from the delta
        for change in &delta.changes {
            match change {
                flotilla_protocol::Change::ProviderHealth {
                    provider,
                    op:
                        flotilla_protocol::EntryOp::Added(v) | flotilla_protocol::EntryOp::Updated(v),
                } => {
                    rm.provider_health.insert(provider.clone(), *v);
                }
                flotilla_protocol::Change::ProviderHealth {
                    provider,
                    op: flotilla_protocol::EntryOp::Removed,
                } => {
                    rm.provider_health.remove(provider);
                }
                flotilla_protocol::Change::ErrorsChanged(errors) => {
                    self.model.status_message = format_error_status(errors, &path);
                }
                _ => {}
            }
        }

        // Use daemon's pre-correlated work items directly (no re-correlation)
        let section_labels = SectionLabels {
            checkouts: rm.labels.checkouts.section.clone(),
            code_review: rm.labels.code_review.section.clone(),
            issues: rm.labels.issues.section.clone(),
            sessions: rm.labels.sessions.section.clone(),
        };
        let table_view =
            data::group_work_items(&delta.work_items, &rm.providers, &section_labels);

        // Provider health -> model-level statuses
        for (kind, healthy) in &rm.provider_health {
            let provider_name = rm.provider_names.get(kind.as_str()).cloned();
            if let Some(pname) = provider_name {
                let key = (path.clone(), kind.clone(), pname);
                let status = if *healthy {
                    ProviderStatus::Ok
                } else {
                    ProviderStatus::Error
                };
                self.model.provider_statuses.insert(key, status);
            }
        }

        // Change detection badge — any non-empty delta on inactive tab
        let has_data_changes = delta.changes.iter().any(|c| {
            !matches!(
                c,
                flotilla_protocol::Change::ProviderHealth { .. }
                    | flotilla_protocol::Change::ErrorsChanged(_)
            )
        });
        if has_data_changes {
            let active_idx = self.model.active_repo;
            let i = self.model.repo_order.iter().position(|p| p == &path);
            if let Some(idx) = i {
                if idx != active_idx {
                    if let Some(rui) = self.ui.repo_ui.get_mut(&path) {
                        rui.has_unseen_changes = true;
                    }
                }
            }
        }

        if let Some(rui) = self.ui.repo_ui.get_mut(&path) {
            rui.update_table_view(table_view);
        }
    }
```

**Step 2: Remove unused imports**

In `mod.rs` line 16, the `data` import no longer needs the `correlate` function. Check if `flotilla_core::convert` is still used anywhere — if not, remove that import too.

**Step 3: Build and test**

Run: `cargo build && cargo test --locked 2>&1 | tail -20`
Run: `cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -20`

**Step 4: Commit**

```
perf: remove re-correlation from TUI apply_delta — use daemon's work_items
```

---

### Task 4: Add try_next to EventHandler

**Files:**
- Modify: `crates/flotilla-tui/src/event.rs:81-84`

**Step 1: Add try_next method**

After the existing `next()` method (line 83), add:

```rust
    /// Non-blocking: returns the next queued event if one is available.
    pub fn try_next(&mut self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
```

**Step 2: Build**

Run: `cargo build 2>&1 | tail -5`

**Step 3: Commit**

```
feat: add non-blocking try_next to EventHandler
```

---

### Task 5: Restructure main event loop to drain+coalesce

**Files:**
- Modify: `src/main.rs:159-308` (main event loop)

**Step 1: Add initial draw before the loop**

Before line 159 (`loop {`), add:

```rust
    terminal.draw(|f| ui::render(&app.model, &mut app.ui, &app.in_flight, f))?;
```

**Step 2: Restructure the loop**

Replace the entire loop body (lines 159-308) with the drain+coalesce pattern. The new loop:

1. Waits for first event (blocking)
2. Drains all pending events into a batch
3. Coalesces: sums scroll delta, keeps latest drag, drops redundant ticks
4. Processes all events in order
5. Processes command queue
6. Draws once

```rust
    loop {
        // ── Wait for the first event (blocking) ──
        let first = match events.next().await {
            Some(evt) => evt,
            None => break,
        };

        // ── Drain all pending events ──
        let mut batch = vec![first];
        while let Some(evt) = events.try_next() {
            batch.push(evt);
        }

        // ── Coalesce ──
        // Scroll: accumulate net delta, remember last position
        let mut scroll_delta: i32 = 0;
        let mut last_scroll_pos: Option<(u16, u16)> = None;
        // Drag: keep only the latest drag event
        let mut last_drag: Option<crossterm::event::MouseEvent> = None;
        // Other events: preserve in order
        let mut other_events: Vec<event::Event> = Vec::new();
        let mut had_tick = false;

        for evt in batch {
            match &evt {
                event::Event::Mouse(m) => match m.kind {
                    crossterm::event::MouseEventKind::ScrollDown => {
                        scroll_delta += 1;
                        last_scroll_pos = Some((m.column, m.row));
                    }
                    crossterm::event::MouseEventKind::ScrollUp => {
                        scroll_delta -= 1;
                        last_scroll_pos = Some((m.column, m.row));
                    }
                    crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                        last_drag = Some(*m);
                    }
                    _ => other_events.push(evt),
                },
                event::Event::Tick => {
                    had_tick = true;
                }
                _ => other_events.push(evt),
            }
        }

        // ── Process coalesced events ──
        // 1. Process all non-scroll/drag/tick events in order
        for evt in other_events {
            match evt {
                event::Event::Daemon(daemon_evt) => {
                    app.handle_daemon_event(daemon_evt);
                }
                event::Event::Key(k) => {
                    let is_normal = matches!(app.ui.mode, app::UiMode::Normal);
                    if k.code == crossterm::event::KeyCode::Char('r') && is_normal {
                        let repo = app.model.active_repo_root().clone();
                        let daemon = app.daemon.clone();
                        tokio::spawn(async move {
                            let _ = daemon.refresh(&repo).await;
                        });
                    } else {
                        app.handle_key(k);
                    }
                }
                event::Event::Mouse(m) => {
                    use crossterm::event::{MouseButton, MouseEventKind};
                    match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let x = m.column;
                            let y = m.row;
                            let mut tab_clicked = false;

                            let ef = app.ui.layout.event_log_filter_area;
                            if x >= ef.x
                                && x < ef.x + ef.width
                                && y >= ef.y
                                && y < ef.y + ef.height
                            {
                                app.ui.event_log.filter = app.ui.event_log.filter.cycle();
                                app.ui.event_log.count = 0;
                                tab_clicked = true;
                            }

                            if !tab_clicked {
                                let hit = app
                                    .ui
                                    .layout
                                    .tab_areas
                                    .iter()
                                    .find(|(_, r)| {
                                        x >= r.x
                                            && x < r.x + r.width
                                            && y >= r.y
                                            && y < r.y + r.height
                                    })
                                    .map(|(id, _)| id.clone());

                                match hit {
                                    Some(app::TabId::Flotilla) => {
                                        app.ui.mode = app::UiMode::Config;
                                        app.ui.drag.dragging_tab = None;
                                        tab_clicked = true;
                                    }
                                    Some(app::TabId::Repo(i)) => {
                                        app.switch_tab(i);
                                        app.ui.drag.dragging_tab = Some(i);
                                        app.ui.drag.start_x = x;
                                        app.ui.drag.active = false;
                                        tab_clicked = true;
                                    }
                                    Some(app::TabId::Gear) if !app.ui.mode.is_config() => {
                                        let sp = app.active_ui().show_providers;
                                        app.active_ui_mut().show_providers = !sp;
                                        tab_clicked = true;
                                    }
                                    Some(app::TabId::Add) => {
                                        let mut input = tui_input::Input::default();
                                        if let Some(parent) =
                                            app.model.active_repo_root().parent()
                                        {
                                            let parent_str =
                                                format!("{}/", parent.display());
                                            input =
                                                tui_input::Input::from(parent_str.as_str());
                                        }
                                        app.ui.mode = app::UiMode::FilePicker {
                                            input,
                                            dir_entries: Vec::new(),
                                            selected: 0,
                                        };
                                        app.refresh_dir_listing();
                                        tab_clicked = true;
                                    }
                                    _ => {}
                                }
                            }
                            if !tab_clicked {
                                app.ui.drag.dragging_tab = None;
                                app.handle_mouse(m);
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            if app.ui.drag.dragging_tab.take().is_some() {
                                if app.ui.drag.active {
                                    app.config.save_tab_order(&app.model.repo_order);
                                }
                                app.ui.drag.active = false;
                            }
                        }
                        _ => {
                            app.handle_mouse(m);
                        }
                    }
                }
                event::Event::Tick => {} // handled via had_tick flag (unused for now)
            }
        }

        // 2. Apply coalesced drag (latest position only)
        if let Some(drag_m) = last_drag {
            use crossterm::event::MouseButton;
            if let Some(dragging_idx) = app.ui.drag.dragging_tab {
                if !app.ui.drag.active {
                    let dx =
                        (drag_m.column as i16 - app.ui.drag.start_x as i16).unsigned_abs();
                    if dx >= 2 {
                        app.ui.drag.active = true;
                    }
                }
                if app.ui.drag.active {
                    for (id, r) in &app.ui.layout.tab_areas {
                        if let app::TabId::Repo(i) = *id {
                            if drag_m.column >= r.x
                                && drag_m.column < r.x + r.width
                                && drag_m.row >= r.y
                                && drag_m.row < r.y + r.height
                                && i != dragging_idx
                            {
                                app.model.repo_order.swap(dragging_idx, i);
                                app.model.active_repo = i;
                                app.ui.drag.dragging_tab = Some(i);
                                break;
                            }
                        }
                    }
                }
            } else {
                app.handle_mouse(drag_m);
            }
        }

        // 3. Apply coalesced scroll
        if scroll_delta != 0 {
            let (col, row) = last_scroll_pos.unwrap_or((0, 0));
            let abs = scroll_delta.unsigned_abs() as usize;
            let kind = if scroll_delta > 0 {
                crossterm::event::MouseEventKind::ScrollDown
            } else {
                crossterm::event::MouseEventKind::ScrollUp
            };
            let synthetic = crossterm::event::MouseEvent {
                kind,
                column: col,
                row,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };
            for _ in 0..abs {
                app.handle_mouse(synthetic);
            }
        }

        // ── Process queued commands ──
        while let Some(cmd) = app.proto_commands.take_next() {
            app::executor::dispatch(cmd, &mut app).await;
        }

        // ── Draw once ──
        terminal.draw(|f| ui::render(&app.model, &mut app.ui, &app.in_flight, f))?;

        if app.should_quit {
            break;
        }
    }
```

**Step 3: Build and test**

Run: `cargo build && cargo test --locked 2>&1 | tail -20`
Run: `cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -20`

**Step 4: Commit**

```
perf: batch-process event loop — drain pending events, draw once per batch
```

---

### Task 6: Remove unused correlation import from TUI

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs:1-20` (imports)

**Step 1: Check and remove unused imports**

After the changes in Task 3, check if `flotilla_core::convert` or `flotilla_core::data::correlate` are still referenced. If `correlate` is no longer used anywhere in the TUI crate, the import can be cleaned up. The `data` import itself is still needed for `group_work_items`, `GroupEntry`, and `SectionLabels`.

Run: `cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -30`

Fix any unused import warnings.

**Step 2: Final verification**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`

**Step 3: Commit**

```
chore: remove unused correlation imports from TUI
```

---

### Task 7: Manual smoke test

**Step 1: Run the app**

```
cargo run
```

Verify:
- Table renders correctly with all sections (checkouts, sessions, PRs, issues)
- Scrolling is responsive (no multi-second stalls)
- Tab switching works
- Tab drag-reordering works
- Action menu opens on right-click or `.`
- Refresh (`r`) still triggers data update
- Switching to a tab with changes clears the unseen badge

**Step 2: Final commit if any fixes needed**
