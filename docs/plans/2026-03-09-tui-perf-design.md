# TUI Performance: Skip Re-correlation and Batch Event Processing

Issue: #154

## Problem

Two performance problems cause multi-second stalled scrolling:

1. **Redundant re-correlation in apply_delta.** The daemon already correlates work items (union-find + issue linking) and sends pre-correlated `Vec<WorkItem>` in full snapshots. But `SnapshotDelta` omits work items, so `apply_delta()` re-runs the entire correlation pipeline from raw `ProviderData` — identical work done twice.

2. **One-event-per-draw loop.** The main loop draws after every single event. When the loop falls behind (e.g. during expensive delta processing), scroll events queue up unboundedly. Each queued scroll triggers another full draw, creating a feedback loop.

## Changes

### 1. Include work_items in SnapshotDelta

**Protocol** (`flotilla-protocol/src/lib.rs`):
- Add `work_items: Vec<WorkItem>` field to `SnapshotDelta`.

**Daemon** (`flotilla-core/src/in_process.rs`):
- In `choose_event()`, populate `snapshot_delta.work_items` from `snapshot.work_items`.

**TUI** (`flotilla-tui/src/app/mod.rs`):
- In `apply_delta()`, delete the re-correlation block (lines 320-326) that calls `data::correlate()` and `correlation_result_to_work_item()`.
- Pass `delta.work_items` directly to `data::group_work_items()`, matching what `apply_snapshot()` already does.

**Tests** (`flotilla-protocol/src/lib.rs` tests, `flotilla-core/src/in_process.rs` tests):
- Update `SnapshotDelta` construction in roundtrip tests to include `work_items`.

### 2. Batch-process event loop

**EventHandler** (`flotilla-tui/src/event.rs`):
- Add `pub fn try_next(&mut self) -> Option<Event>` that calls `self.rx.try_recv().ok()`.

**Main loop** (`src/main.rs`):
- Restructure from draw-wait-handle to wait-drain-handle-draw:
  1. Wait for first event (`events.next().await`)
  2. Drain all pending events via `events.try_next()` into a batch
  3. Coalesce within the batch:
     - **Scroll:** Sum net scroll delta (ScrollUp = -1, ScrollDown = +1). Apply as N calls to `handle_mouse` with a single synthetic scroll event at the last scroll position.
     - **Mouse drag:** Keep only the latest Drag event (preserves final cursor position for tab reorder hit-testing).
     - **Ticks:** Discard all but the last.
     - **Keys, MouseDown, MouseUp, daemon events:** Preserve in order (semantic actions).
  4. Process all coalesced events
  5. Process command queue
  6. Draw once
- Add one `terminal.draw()` before the loop to render the initial frame.

## What we are NOT changing

- **Full snapshots** — `apply_snapshot()` already uses daemon's work_items. No fix needed.
- **Daemon-side refresh intervals** — Orthogonal concern; the batch-process loop handles bursts.
- **Bounded channels** — The drain pattern handles backlog without needing backpressure.
- **Daemon-side grouping** — `group_work_items()` (sort + section) is cheap. Not worth sending pre-grouped data.
