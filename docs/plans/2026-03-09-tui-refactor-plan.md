# TUI Module Refactor (#75, #76) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Split `app/mod.rs` (1268 lines) into focused modules and extract testable helpers from `ui.rs` (1147 lines) to enable unit testing of TUI logic.

**Architecture:** Pure code-move refactoring. No behavior changes. All methods stay as `impl App` — submodules use `use super::*` to access the parent type. UI helpers become free functions in a new `ui_helpers.rs` module.

**Tech Stack:** Rust, ratatui

---

### Task 1: Create `app/navigation.rs`

**Files:**
- Create: `crates/flotilla-tui/src/app/navigation.rs`
- Modify: `crates/flotilla-tui/src/app/mod.rs`

**Step 1: Create `navigation.rs` with methods moved from `mod.rs`**

Move these `impl App` methods to `navigation.rs`:
- `switch_tab` (mod.rs:437-448)
- `next_tab` (mod.rs:450-462)
- `prev_tab` (mod.rs:464-476)
- `move_tab` (mod.rs:478-492)
- `select_next` (mod.rs:1219-1251)
- `select_prev` (mod.rs:1253-1267)
- `row_at_mouse` (mod.rs:732-750)
- `toggle_multi_select` (mod.rs:752-766)

The file should start with:
```rust
use super::*;

impl App {
    // ... moved methods
}
```

**Step 2: Add `mod navigation;` to `app/mod.rs`**

Add `mod navigation;` near the top module declarations. Remove the moved methods from mod.rs.

**Step 3: Verify**

Run: `cargo build 2>&1 | head -30`
Expected: successful build

Run: `cargo test --locked 2>&1 | tail -5`
Expected: all tests pass

**Step 4: Commit**

```
refactor: extract app/navigation.rs from app/mod.rs (#75)
```

---

### Task 2: Create `app/key_handlers.rs`

**Files:**
- Create: `crates/flotilla-tui/src/app/key_handlers.rs`
- Modify: `crates/flotilla-tui/src/app/mod.rs`

**Step 1: Create `key_handlers.rs` with methods moved from `mod.rs`**

Move these `impl App` methods:
- `handle_key` (mod.rs:508-540)
- `handle_normal_key` (mod.rs:567-631)
- `handle_config_key` (mod.rs:542-565)
- `handle_mouse` (mod.rs:635-696)
- `handle_menu_mouse` (mod.rs:700-730)
- `handle_menu_key` (mod.rs:870-902)
- `handle_branch_input_key` (mod.rs:904-944)
- `handle_issue_search_key` (mod.rs:1150-1176)
- `handle_delete_confirm_key` (mod.rs:1178-1201)
- `action_enter` (mod.rs:768-784)
- `action_enter_multi_select` (mod.rs:786-819)
- `dispatch_if_available` (mod.rs:821-828)
- `resolve_and_push` (mod.rs:830-850)
- `open_action_menu` (mod.rs:852-868)
- `execute_menu_action` (mod.rs:1203-1217)

The file needs these imports:
```rust
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use tui_input::backend::crossterm::EventHandler as InputEventHandler;
use tui_input::Input;

use flotilla_core::data::GroupEntry;
use flotilla_protocol::Command;

use super::{App, Intent, UiMode};
```

**Step 2: Add `mod key_handlers;` to `app/mod.rs`, remove moved methods**

Also remove now-unused imports from mod.rs (KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind, InputEventHandler, Input, Instant).

**Step 3: Verify**

Run: `cargo build 2>&1 | head -30`
Expected: successful build

Run: `cargo test --locked 2>&1 | tail -5`
Expected: all tests pass

**Step 4: Commit**

```
refactor: extract app/key_handlers.rs from app/mod.rs (#75)
```

---

### Task 3: Create `app/file_picker.rs`

**Files:**
- Create: `crates/flotilla-tui/src/app/file_picker.rs`
- Modify: `crates/flotilla-tui/src/app/mod.rs`

**Step 1: Create `file_picker.rs` with methods moved from `mod.rs`**

Move these `impl App` methods:
- `handle_file_picker_key` (mod.rs:946-1008)
- `activate_dir_entry` (mod.rs:1010-1054)
- `handle_file_picker_mouse` (mod.rs:1056-1088)
- `refresh_dir_listing` (mod.rs:1090-1148)

```rust
use std::path::PathBuf;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use tui_input::backend::crossterm::EventHandler as InputEventHandler;
use tui_input::Input;

use flotilla_protocol::Command;

use super::{App, DirEntry, UiMode};
```

**Step 2: Add `mod file_picker;` to `app/mod.rs`, remove moved methods**

**Step 3: Verify**

Run: `cargo build 2>&1 | head -30` and `cargo test --locked 2>&1 | tail -5`
Expected: build succeeds, all tests pass

**Step 4: Commit**

```
refactor: extract app/file_picker.rs from app/mod.rs (#75)
```

---

### Task 4: Run clippy and verify final app/mod.rs

**Step 1: Check line count**

Run: `wc -l crates/flotilla-tui/src/app/mod.rs`
Expected: ~430 lines (structs + daemon event handling + accessors)

**Step 2: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings 2>&1 | tail -20`
Expected: clean

**Step 3: Commit any clippy fixes if needed**

---

### Task 5: Create `ui_helpers.rs` with extracted pure functions

**Files:**
- Create: `crates/flotilla-tui/src/ui_helpers.rs`
- Modify: `crates/flotilla-tui/src/lib.rs`
- Modify: `crates/flotilla-tui/src/ui.rs`

**Step 1: Create `ui_helpers.rs`**

Extract these free functions from `ui.rs`:

```rust
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Color;
use flotilla_protocol::{
    ChangeRequestStatus, Checkout, SessionStatus, WorkItemKind,
};

/// Truncate a string to `max` characters, appending '…' if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    // ... moved from ui.rs:1126-1137
}

/// Calculate a centered popup area as a percentage of the parent area.
pub fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    // ... moved from ui.rs:1139-1147
}

/// Return the (icon, color) for a work item based on its kind and state.
pub fn work_item_icon(
    kind: WorkItemKind,
    has_workspace: bool,
    session_status: Option<&SessionStatus>,
) -> (&'static str, Color) {
    match kind {
        WorkItemKind::Checkout => {
            if has_workspace { ("●", Color::Green) } else { ("○", Color::Green) }
        }
        WorkItemKind::Session => match session_status {
            Some(SessionStatus::Running) => ("▶", Color::Magenta),
            Some(SessionStatus::Idle) => ("◆", Color::Magenta),
            _ => ("○", Color::Magenta),
        },
        WorkItemKind::ChangeRequest => ("⊙", Color::Blue),
        WorkItemKind::RemoteBranch => ("⊶", Color::DarkGray),
        WorkItemKind::Issue => ("◇", Color::Yellow),
    }
}

/// Return a display string for a session status.
pub fn session_status_display(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Running => "▶",
        SessionStatus::Idle => "◆",
        SessionStatus::Archived => "○",
    }
}

/// Return the status icon suffix for a change request.
pub fn change_request_status_icon(status: &ChangeRequestStatus) -> &'static str {
    match status {
        ChangeRequestStatus::Merged => "✓",
        ChangeRequestStatus::Closed => "✗",
        _ => "",
    }
}

/// Build git status indicator string (M/S/?/↑) from a checkout.
pub fn git_status_display(checkout: &Checkout) -> String {
    let mut s = String::new();
    if checkout.working_tree.as_ref().is_some_and(|w| w.modified > 0) {
        s.push('M');
    }
    if checkout.working_tree.as_ref().is_some_and(|w| w.staged > 0) {
        s.push('S');
    }
    if checkout.working_tree.as_ref().is_some_and(|w| w.untracked > 0) {
        s.push('?');
    }
    if checkout.trunk_ahead_behind.as_ref().is_some_and(|m| m.ahead > 0) {
        s.push('↑');
    }
    s
}

/// Checkout indicator for the worktree column.
pub fn checkout_indicator(is_main: bool, has_checkout: bool) -> &'static str {
    if is_main {
        "◆"
    } else if has_checkout {
        "✓"
    } else {
        ""
    }
}

/// Workspace indicator for the WS column.
pub fn workspace_indicator(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "●".to_string(),
        n => format!("{n}"),
    }
}
```

**Step 2: Add `pub mod ui_helpers;` to `lib.rs`**

**Step 3: Update `ui.rs` to use the helpers**

Replace inline logic in `build_item_row` with calls to the helpers:
- `use crate::ui_helpers::*;`
- Replace the icon match block with `work_item_icon()`
- Replace the session display match with `session_status_display()`
- Replace the CR status icon match with `change_request_status_icon()`
- Replace the git display block with `git_status_display()`
- Replace the wt_indicator block with `checkout_indicator()`
- Replace the ws_indicator block with `workspace_indicator()`
- `truncate` and `popup_area` are already top-level fns — just move them

**Step 4: Verify**

Run: `cargo build 2>&1 | head -30` and `cargo test --locked 2>&1 | tail -5`
Expected: build succeeds, all tests pass

**Step 5: Commit**

```
refactor: extract ui_helpers.rs from ui.rs (#76)
```

---

### Task 6: Add tests for ui_helpers

**Files:**
- Modify: `crates/flotilla-tui/src/ui_helpers.rs` (add `#[cfg(test)] mod tests`)

**Step 1: Write tests for `truncate`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn truncate_zero_max() {
        assert_eq!(truncate("hello", 0), "");
    }

    #[test]
    fn truncate_fits() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_too_long() {
        assert_eq!(truncate("hello world", 5), "hell…");
    }

    #[test]
    fn truncate_one_char_max() {
        // max=1 means 0 chars + ellipsis = just "…"
        assert_eq!(truncate("hello", 1), "…");
    }
}
```

**Step 2: Write tests for `popup_area`**

```rust
    #[test]
    fn popup_area_centers() {
        let area = Rect::new(0, 0, 100, 50);
        let popup = popup_area(area, 50, 50);
        // Should be centered and approximately 50% of parent
        assert!(popup.x > 0);
        assert!(popup.y > 0);
        assert!(popup.width <= 50);
        assert!(popup.height <= 25);
    }
```

**Step 3: Write tests for icon/status helpers**

```rust
    #[test]
    fn work_item_icon_checkout_with_workspace() {
        let (icon, color) = work_item_icon(WorkItemKind::Checkout, true, None);
        assert_eq!(icon, "●");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn work_item_icon_checkout_without_workspace() {
        let (icon, color) = work_item_icon(WorkItemKind::Checkout, false, None);
        assert_eq!(icon, "○");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn work_item_icon_session_running() {
        let (icon, color) = work_item_icon(
            WorkItemKind::Session,
            false,
            Some(&SessionStatus::Running),
        );
        assert_eq!(icon, "▶");
        assert_eq!(color, Color::Magenta);
    }

    #[test]
    fn work_item_icon_all_kinds() {
        // Verify every kind returns something
        for kind in [
            WorkItemKind::Checkout,
            WorkItemKind::Session,
            WorkItemKind::ChangeRequest,
            WorkItemKind::RemoteBranch,
            WorkItemKind::Issue,
        ] {
            let (icon, _) = work_item_icon(kind, false, None);
            assert!(!icon.is_empty());
        }
    }

    #[test]
    fn session_status_display_variants() {
        assert_eq!(session_status_display(&SessionStatus::Running), "▶");
        assert_eq!(session_status_display(&SessionStatus::Idle), "◆");
        assert_eq!(session_status_display(&SessionStatus::Archived), "○");
    }

    #[test]
    fn change_request_status_icon_variants() {
        assert_eq!(change_request_status_icon(&ChangeRequestStatus::Merged), "✓");
        assert_eq!(change_request_status_icon(&ChangeRequestStatus::Closed), "✗");
        assert_eq!(change_request_status_icon(&ChangeRequestStatus::Open), "");
        assert_eq!(change_request_status_icon(&ChangeRequestStatus::Draft), "");
    }

    #[test]
    fn checkout_indicator_variants() {
        assert_eq!(checkout_indicator(true, true), "◆");
        assert_eq!(checkout_indicator(false, true), "✓");
        assert_eq!(checkout_indicator(false, false), "");
    }

    #[test]
    fn workspace_indicator_variants() {
        assert_eq!(workspace_indicator(0), "");
        assert_eq!(workspace_indicator(1), "●");
        assert_eq!(workspace_indicator(3), "3");
    }

    #[test]
    fn git_status_display_empty() {
        let co = Checkout {
            working_tree: None,
            trunk_ahead_behind: None,
            ..Default::default()
        };
        assert_eq!(git_status_display(&co), "");
    }
```

**Step 4: Run tests**

Run: `cargo test -p flotilla-tui 2>&1 | tail -10`
Expected: all new tests pass

**Step 5: Commit**

```
test: add unit tests for ui_helpers (#76)
```

---

### Task 7: Final verification

**Step 1: Run full clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`

**Step 2: Run full test suite**

Run: `cargo test --locked`

**Step 3: Check line counts**

Run: `wc -l crates/flotilla-tui/src/app/*.rs crates/flotilla-tui/src/ui.rs crates/flotilla-tui/src/ui_helpers.rs`

Expected approximate sizes:
- `app/mod.rs`: ~430 lines
- `app/navigation.rs`: ~150 lines
- `app/key_handlers.rs`: ~500 lines
- `app/file_picker.rs`: ~200 lines
- `ui.rs`: ~1000 lines (reduced by ~150)
- `ui_helpers.rs`: ~200 lines (including tests)
