# Widget Tree Restructure Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the BaseView god-widget with a proper widget tree (Screen → Tabs → TabPage → RepoPage/OverviewPage), where each widget owns its UI state and reads daemon data via `Shared<T>` handles.

**Architecture:** Incremental migration that keeps the app working at each step. Screen becomes the new root, Tabs owns pages, RepoPage/OverviewPage own their content widgets and UI state. Daemon data flows through `Shared<T>` — a newtype around `AtomicU64` + `Mutex<T>` with generation-based change detection.

**Tech Stack:** Rust, ratatui, crossterm, tokio (single-threaded event loop)

**Spec:** `docs/superpowers/specs/2026-03-20-widget-tree-restructure-design.md`

---

## File Structure

### New files
| File | Responsibility |
|------|---------------|
| `crates/flotilla-tui/src/shared.rs` | `Shared<T>` newtype with `changed()`, `read()`, `mutate()`, `generation()` |
| `crates/flotilla-tui/src/widgets/screen.rs` | Root widget: owns Tabs, StatusBar, modal stack; handles global actions |
| `crates/flotilla-tui/src/widgets/tabs.rs` | Tab container: owns `Vec<TabPage>`, renders tab bar strip, routes to active page |
| `crates/flotilla-tui/src/widgets/repo_page.rs` | Per-repo page: owns WorkItemTable, PreviewPanel, selection/multi-select state |
| `crates/flotilla-tui/src/widgets/overview_page.rs` | Flotilla overview: composes ProvidersWidget, HostsWidget, EventLogWidget |
| `crates/flotilla-tui/src/widgets/providers_widget.rs` | Renders aggregate provider status table |
| `crates/flotilla-tui/src/widgets/hosts_widget.rs` | Renders connected hosts and health |

### Files to heavily modify
| File | Changes |
|------|---------|
| `crates/flotilla-tui/src/widgets/mod.rs` | Slim down `WidgetContext` and `RenderContext`, add new module declarations |
| `crates/flotilla-tui/src/app/mod.rs` | Remove `widget_stack`, `with_base_view()`, modal helpers; delegate to Screen |
| `crates/flotilla-tui/src/app/key_handlers.rs` | Simplify: delegate to Screen instead of managing stack dispatch |
| `crates/flotilla-tui/src/app/ui_state.rs` | Remove `RepoUiState`, `UiMode`, shrink `UiState` |
| `crates/flotilla-tui/src/app/navigation.rs` | Move tab navigation logic into Tabs widget |
| `crates/flotilla-tui/src/run.rs` | Simplify render_frame — Screen is the sole root widget |
| `crates/flotilla-tui/src/widgets/work_item_table.rs` | Own selection state directly (`selected_identity`, `grouped_items`) |
| `crates/flotilla-tui/src/widgets/event_log.rs` | Slim down — overview layout moves to OverviewPage |

### Files to delete
| File | Reason |
|------|--------|
| `crates/flotilla-tui/src/widgets/base_view.rs` | Replaced by Screen + Tabs + RepoPage + OverviewPage |
| `crates/flotilla-tui/src/widgets/tab_bar.rs` | Absorbed into Tabs widget |

---

## Task 1: Introduce `Shared<T>`

**Files:**
- Create: `crates/flotilla-tui/src/shared.rs`
- Modify: `crates/flotilla-tui/src/lib.rs` (add `pub mod shared;`)

This is a standalone utility with no dependencies on the rest of the widget system.

- [ ] **Step 1: Write failing tests for Shared<T>**

Create `crates/flotilla-tui/src/shared.rs` with the struct and tests:

```rust
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};

/// Shared mutable state with generation-based change detection.
///
/// The event loop calls `mutate()` to update data and bump the generation.
/// Widgets call `changed()` to detect updates since their last read.
/// The generation counter is an `AtomicU64` outside the mutex, so
/// `generation()` is lock-free.
#[derive(Debug)]
pub struct Shared<T> {
    inner: Arc<SharedInner<T>>,
}

#[derive(Debug)]
struct SharedInner<T> {
    generation: AtomicU64,
    data: Mutex<T>,
}

impl<T> Shared<T> {
    pub fn new(data: T) -> Self {
        Self {
            inner: Arc::new(SharedInner {
                generation: AtomicU64::new(1), // start at 1 so 0 means "never seen"
                data: Mutex::new(data),
            }),
        }
    }

    /// Lock and return the data unconditionally.
    pub fn read(&self) -> MutexGuard<'_, T> {
        self.inner.data.lock().expect("shared data poisoned")
    }

    /// Current generation (lock-free).
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// If the generation advanced since `*since`, lock the data, update
    /// `*since`, and return the guard. Otherwise return `None`.
    pub fn changed(&self, since: &mut u64) -> Option<MutexGuard<'_, T>> {
        let current = self.generation();
        if current > *since {
            *since = current;
            Some(self.read())
        } else {
            None
        }
    }

    /// Lock the data, apply `f`, and bump the generation.
    pub fn mutate(&self, f: impl FnOnce(&mut T)) {
        let mut guard = self.inner.data.lock().expect("shared data poisoned");
        f(&mut *guard);
        self.inner.generation.fetch_add(1, Ordering::Release);
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_at_generation_1() {
        let s = Shared::new(42);
        assert_eq!(s.generation(), 1);
    }

    #[test]
    fn read_returns_initial_data() {
        let s = Shared::new("hello");
        assert_eq!(*s.read(), "hello");
    }

    #[test]
    fn mutate_updates_data_and_bumps_generation() {
        let s = Shared::new(0);
        s.mutate(|v| *v = 99);
        assert_eq!(*s.read(), 99);
        assert_eq!(s.generation(), 2);
    }

    #[test]
    fn changed_returns_data_on_first_call() {
        let s = Shared::new(42);
        let mut seen = 0u64;
        let guard = s.changed(&mut seen);
        assert!(guard.is_some());
        assert_eq!(*guard.unwrap(), 42);
        assert_eq!(seen, 1);
    }

    #[test]
    fn changed_returns_none_when_unchanged() {
        let s = Shared::new(42);
        let mut seen = 0u64;
        let _ = s.changed(&mut seen); // consume initial
        assert!(s.changed(&mut seen).is_none());
    }

    #[test]
    fn changed_returns_data_after_mutate() {
        let s = Shared::new(0);
        let mut seen = 0u64;
        let _ = s.changed(&mut seen); // consume initial
        s.mutate(|v| *v = 5);
        let guard = s.changed(&mut seen);
        assert!(guard.is_some());
        assert_eq!(*guard.unwrap(), 5);
        assert_eq!(seen, 2);
    }

    #[test]
    fn clone_shares_state() {
        let s1 = Shared::new(0);
        let s2 = s1.clone();
        s1.mutate(|v| *v = 7);
        assert_eq!(*s2.read(), 7);
        assert_eq!(s2.generation(), 2);
    }

    #[test]
    fn multiple_mutates_accumulate_generation() {
        let s = Shared::new(0);
        for i in 1..=5 {
            s.mutate(|v| *v = i);
        }
        assert_eq!(s.generation(), 6); // 1 initial + 5 mutates
        assert_eq!(*s.read(), 5);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p flotilla-tui --lib shared -- --nocapture`
Expected: All 7 tests pass.

- [ ] **Step 3: Add module declaration**

In `crates/flotilla-tui/src/lib.rs`, add `pub mod shared;`.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace --locked`
Expected: All existing tests still pass.

- [ ] **Step 5: Run CI checks**

Run: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: Clean.

- [ ] **Step 6: Commit**

```bash
git add crates/flotilla-tui/src/shared.rs crates/flotilla-tui/src/lib.rs
git commit -m "feat: add Shared<T> newtype with generation-based change detection"
```

---

## Task 2: Create Screen as thin wrapper around BaseView

**Files:**
- Create: `crates/flotilla-tui/src/widgets/screen.rs`
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (add `pub mod screen;`)
- Modify: `crates/flotilla-tui/src/app/mod.rs` (use Screen at stack[0] instead of BaseView)

Screen wraps BaseView and delegates everything. No behavior change.

- [ ] **Step 1: Create Screen widget**

Create `crates/flotilla-tui/src/widgets/screen.rs`:

```rust
use std::any::Any;

use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::{layout::Rect, Frame};

use super::{base_view::BaseView, InteractiveWidget, Outcome, RenderContext, WidgetContext, WidgetStatusData};
use crate::keymap::{Action, ModeId};

/// Root widget that wraps the base layer. Will eventually own Tabs,
/// StatusBar, and the modal stack. For now, delegates to BaseView.
pub struct Screen {
    pub base_view: BaseView,
}

impl Screen {
    pub fn new() -> Self {
        Self { base_view: BaseView::new() }
    }
}

impl InteractiveWidget for Screen {
    fn handle_action(&mut self, action: Action, ctx: &mut WidgetContext) -> Outcome {
        self.base_view.handle_action(action, ctx)
    }

    fn handle_raw_key(&mut self, key: KeyEvent, ctx: &mut WidgetContext) -> Outcome {
        self.base_view.handle_raw_key(key, ctx)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, ctx: &mut WidgetContext) -> Outcome {
        self.base_view.handle_mouse(mouse, ctx)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &mut RenderContext) {
        self.base_view.render(frame, area, ctx)
    }

    fn mode_id(&self) -> ModeId {
        self.base_view.mode_id()
    }

    fn captures_raw_keys(&self) -> bool {
        self.base_view.captures_raw_keys()
    }

    fn status_data(&self) -> WidgetStatusData {
        self.base_view.status_data()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
```

- [ ] **Step 2: Add module declaration**

In `crates/flotilla-tui/src/widgets/mod.rs`, add `pub mod screen;`.

- [ ] **Step 3: Replace BaseView with Screen in App::new()**

In `crates/flotilla-tui/src/app/mod.rs`, change the `widget_stack` initialization in `App::new()`:

```rust
// Before:
widget_stack: vec![Box::new(crate::widgets::base_view::BaseView::new())],
// After:
widget_stack: vec![Box::new(crate::widgets::screen::Screen::new())],
```

- [ ] **Step 4: Update with_base_view() to downcast through Screen**

In `crates/flotilla-tui/src/app/mod.rs`, update `with_base_view()`:

```rust
pub fn with_base_view<R>(&mut self, f: impl FnOnce(&mut crate::widgets::base_view::BaseView) -> R) -> R {
    let mut stack = std::mem::take(&mut self.widget_stack);
    let screen = stack[0]
        .as_any_mut()
        .downcast_mut::<crate::widgets::screen::Screen>()
        .expect("widget_stack[0] is always Screen");
    let result = f(&mut screen.base_view);
    self.widget_stack = stack;
    result
}
```

- [ ] **Step 5: Update tab drag in handle_mouse()**

In `crates/flotilla-tui/src/app/key_handlers.rs`, the drag handling downcasts to BaseView. Update it to go through Screen:

```rust
// Before:
let base = stack[0]
    .as_any_mut()
    .downcast_mut::<crate::widgets::base_view::BaseView>()
    .expect("widget_stack[0] is always BaseView");
// After:
let screen = stack[0]
    .as_any_mut()
    .downcast_mut::<crate::widgets::screen::Screen>()
    .expect("widget_stack[0] is always Screen");
let base = &mut screen.base_view;
```

- [ ] **Step 6: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: All pass — behavior is identical.

- [ ] **Step 7: Commit**

```bash
git commit -am "refactor: introduce Screen as thin wrapper around BaseView"
```

---

## Task 3: Move global action resolution into Screen

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/screen.rs`
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs`
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (add global action variants to AppAction if needed)

Currently `App::handle_global_action()` in `key_handlers.rs:24-65` handles global actions. Move this logic into `Screen::handle_action()`, emitting `AppAction` variants for things that need `App` context (theme cycling, layout persistence, tab switching).

- [ ] **Step 1: Add missing AppAction variants**

In `crates/flotilla-tui/src/widgets/mod.rs`, add variants to `AppAction` for anything Screen needs to request:

```rust
pub enum AppAction {
    // ... existing variants ...
    PrevTab,
    NextTab,
    MoveTabLeft,
    MoveTabRight,
    Refresh,
}
```

- [ ] **Step 2: Implement global action handling in Screen**

In `screen.rs`, update `handle_action` to intercept global actions before delegating to BaseView. **Note:** `CycleLayout` is intentionally NOT here — per spec, it becomes page-scoped in Task 6 (RepoPage owns its layout).

```rust
fn handle_action(&mut self, action: Action, ctx: &mut WidgetContext) -> Outcome {
    // Phase 1: Global actions
    match action {
        Action::PrevTab => { ctx.app_actions.push(AppAction::PrevTab); return Outcome::Consumed; }
        Action::NextTab => { ctx.app_actions.push(AppAction::NextTab); return Outcome::Consumed; }
        Action::MoveTabLeft => { ctx.app_actions.push(AppAction::MoveTabLeft); return Outcome::Consumed; }
        Action::MoveTabRight => { ctx.app_actions.push(AppAction::MoveTabRight); return Outcome::Consumed; }
        Action::CycleTheme => { ctx.app_actions.push(AppAction::CycleTheme); return Outcome::Consumed; }
        Action::CycleHost => { ctx.app_actions.push(AppAction::CycleHost); return Outcome::Consumed; }
        Action::ToggleDebug => { ctx.app_actions.push(AppAction::ToggleDebug); return Outcome::Consumed; }
        Action::ToggleStatusBarKeys => { ctx.app_actions.push(AppAction::ToggleStatusBarKeys); return Outcome::Consumed; }
        Action::Refresh => { ctx.app_actions.push(AppAction::Refresh); return Outcome::Consumed; }
        _ => {}
    }
    // Phase 2: Delegate to BaseView
    self.base_view.handle_action(action, ctx)
}
```

- [ ] **Step 3: Remove global bypass from App::handle_key()**

In `key_handlers.rs`, remove the `action.is_global() && !self.has_modal()` check (lines 144-148). Global actions are now handled by Screen's `handle_action()`, which sits at the bottom of the widget stack. When a modal is on top, the modal gets the event first and traps it — Screen never sees it.

Also remove `App::handle_global_action()` entirely. Its logic is split between `Screen::handle_action()` (dispatch) and `App::process_app_actions()` (execution).

- [ ] **Step 4: Remove the refresh special case in run.rs**

In `run.rs` lines 95-112, there is a special-cased `r` key handler that spawns a refresh directly, bypassing `app.handle_key()`. Remove this — refresh now flows through the normal path: `app.handle_key()` → Screen → `AppAction::Refresh` → `process_app_actions()`. Without removing this, refresh would be double-handled.

- [ ] **Step 5: Add the new AppAction variants to process_app_actions()**

In `app/mod.rs`, handle **all** variants that moved from `handle_global_action()` into `process_app_actions()`. This includes the existing `CycleTheme`/`CycleHost`/`ToggleDebug`/`ToggleStatusBarKeys` (which already have handlers in `process_app_actions` — verify they exist and are correct) plus the new ones:

```rust
AppAction::PrevTab => self.prev_tab(),
AppAction::NextTab => self.next_tab(),
AppAction::MoveTabLeft => {
    if !self.ui.mode.is_config() && self.move_tab(-1) {
        self.config.save_tab_order(&self.persisted_tab_order_paths());
    }
}
AppAction::MoveTabRight => {
    if !self.ui.mode.is_config() && self.move_tab(1) {
        self.config.save_tab_order(&self.persisted_tab_order_paths());
    }
}
AppAction::Refresh => {
    let repo = self.model.active_repo_root().clone();
    self.proto_commands.push(self.command(
        flotilla_protocol::CommandAction::Refresh { repo: Some(flotilla_protocol::RepoSelector::Path(repo)) }
    ));
}
```

- [ ] **Step 6: Verify the global-action-when-modal-open behavior**

Key concern: with the old code, global actions were explicitly blocked when a modal was open (`action.is_global() && !self.has_modal()`). In the new model, Screen sits at `widget_stack[0]`. When a modal is on top, the dispatch loop in `handle_key()` sets `stop_at = 1` (line 160 of key_handlers.rs), so the loop never reaches index 0 (Screen). Global actions are therefore blocked structurally — Screen never sees the event, not because the modal traps it, but because the dispatch loop excludes the base layer when modals are present. Verify this by checking that existing tests covering "global keys while modal is open" still pass.

- [ ] **Step 7: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git commit -am "refactor: move global action resolution from App into Screen widget"
```

---

## Task 4a: Move modal stack and render into Screen

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/screen.rs`
- Modify: `crates/flotilla-tui/src/app/mod.rs`
- Modify: `crates/flotilla-tui/src/run.rs`

Structural change: Screen owns the modal stack and renders it. App owns Screen directly instead of `widget_stack`. No dispatch changes yet.

- [ ] **Step 1: Add modal stack to Screen and change App ownership**

Screen gets a modal stack. App owns Screen directly instead of `widget_stack`:

```rust
// screen.rs
pub struct Screen {
    pub base_view: BaseView,
    pub modal_stack: Vec<Box<dyn InteractiveWidget>>,
}

// app/mod.rs
pub struct App {
    // ... existing fields ...
    pub screen: Screen,  // replaces widget_stack
}
```

In `App::new()`, construct Screen directly:
```rust
screen: Screen::new(),
```

- [ ] **Step 2: Move render stack iteration into Screen**

`Screen::render()` renders the base layer then modals:

```rust
fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &mut RenderContext) {
    self.base_view.render(frame, area, ctx);
    for modal in &mut self.modal_stack {
        modal.render(frame, area, ctx);
    }
}
```

Update `run.rs::render_frame()` to render Screen directly. No more `std::mem::take`:

```rust
fn render_frame(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<()> {
    let active_widget_mode = app.screen.active_mode_id();
    let active_widget_data = app.screen.active_status_data();
    terminal.draw(|f| {
        let area = f.area();
        let mut ctx = RenderContext { /* ... */ };
        app.screen.render(f, area, &mut ctx);
    })?;
    Ok(())
}
```

Add helper methods on Screen: `active_mode_id()` returns the top widget's mode (modal if present, else base_view), `active_status_data()` similarly.

- [ ] **Step 3: Move modal stack helpers to Screen**

Move `apply_outcome()`, `dismiss_modals()`, `has_modal()` from App to Screen:

```rust
impl Screen {
    pub fn has_modal(&self) -> bool { !self.modal_stack.is_empty() }
    pub fn dismiss_modals(&mut self) { self.modal_stack.clear(); }
    pub fn apply_outcome(&mut self, index: usize, outcome: Outcome) {
        match outcome {
            Outcome::Consumed | Outcome::Ignored => {}
            Outcome::Finished => { self.modal_stack.remove(index); }
            Outcome::Push(widget) => { self.modal_stack.push(widget); }
            Outcome::Swap(widget) => {
                self.modal_stack.remove(index);
                self.modal_stack.insert(index, widget);
            }
        }
    }
}
```

Update all call sites in App to use `self.screen.has_modal()`, `self.screen.dismiss_modals()`, etc. `with_base_view()` becomes `self.screen.base_view` direct access.

- [ ] **Step 4: Update needs_animation()**

Check `screen.modal_stack.last()` instead of `widget_stack.last()`.

- [ ] **Step 5: Update tests**

Update `widget_stack` references in tests to `screen.modal_stack` or `screen.base_view`. Update `stub_app()` to construct with `Screen`.

- [ ] **Step 6: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 7: Commit**

```bash
git commit -am "refactor: move modal stack and render into Screen widget"
```

---

## Task 4b: Move key/mouse dispatch into Screen

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/screen.rs`
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs`
- Modify: `crates/flotilla-tui/src/widgets/mod.rs`

Behavioral change: Screen handles modal dispatch internally. App's `handle_key`/`handle_mouse` delegate to Screen.

- [ ] **Step 1: Move key dispatch into Screen**

The current dispatch logic in `App::handle_key()` (key_handlers.rs:99-191) does:
1. Check `captures_raw_keys` on top widget
2. Resolve action via keymap
3. Iterate stack top-down, dispatch action or raw key
4. Apply outcome
5. Fall through to `dispatch_action()` if ignored

Move this into Screen. The key challenge: `WidgetContext` construction. Currently `App::build_widget_context()` borrows `self.model`, `self.keymap`, etc. Screen doesn't own these. Two options:
- Screen's `handle_action()`/`handle_raw_key()` receive `WidgetContext` (already the trait signature), and dispatch to modals then base_view within that call.
- Screen dispatches to its modal stack, then base_view, handling outcomes internally.

The natural approach: Screen's `InteractiveWidget::handle_action()` is called by App with a pre-built `WidgetContext`. Screen dispatches internally:

```rust
fn handle_action(&mut self, action: Action, ctx: &mut WidgetContext) -> Outcome {
    // Global actions (from Task 3)
    match action { /* ... */ }

    // Modal dispatch: try top modal first
    if let Some(modal) = self.modal_stack.last_mut() {
        let outcome = modal.handle_action(action, ctx);
        if !matches!(outcome, Outcome::Ignored) {
            self.apply_modal_outcome(outcome);
            return Outcome::Consumed;
        }
        // Modal is a focus barrier — don't fall through to base
        return Outcome::Ignored;
    }

    // No modal: dispatch to base_view
    self.base_view.handle_action(action, ctx)
}
```

Note: `Outcome::Push` from base_view or modals should push onto `self.modal_stack`. Update `apply_modal_outcome` to handle this.

- [ ] **Step 2: Handle raw keys in Screen**

Same pattern for `handle_raw_key()` — if top modal captures raw keys, dispatch there. Otherwise fall through to base_view if it captures raw keys.

- [ ] **Step 3: Move mouse dispatch into Screen**

Port `App::handle_mouse()` modal dispatch into `Screen::handle_mouse()`. Tab drag handling: add `AppAction::TabDragSwap { column: u16, row: u16 }` to let App perform the repo_order mutation:

```rust
// In widgets/mod.rs
pub enum AppAction {
    // ... existing ...
    TabDragSwap { column: u16, row: u16 },
}
```

In `process_app_actions()`:
```rust
AppAction::TabDragSwap { column, row } => {
    self.screen.base_view.tab_bar.handle_drag(
        column, row,
        &mut self.screen.base_view.drag,
        &mut self.model.repo_order,
        &mut self.model.active_repo,
    );
}
```

- [ ] **Step 4: Simplify App::handle_key() and handle_mouse()**

App's methods become thin wrappers:

```rust
pub fn handle_key(&mut self, key: KeyEvent) {
    let prev_selection = self.active_ui().selected_selectable_idx;

    let action = /* resolve action, same logic as before */;
    let mut ctx = self.build_widget_context();

    if let Some(action) = action {
        self.screen.handle_action(action, &mut ctx);
    } else {
        self.screen.handle_raw_key(key, &mut ctx);
    }

    let app_actions = std::mem::take(&mut ctx.app_actions);
    self.process_app_actions(app_actions);

    if self.active_ui().selected_selectable_idx != prev_selection {
        self.check_infinite_scroll();
    }
}
```

Remove the old stack-based dispatch loop from `handle_key()` and `handle_mouse()`. Remove `dispatch_action()` — its logic moves into Screen or becomes AppAction variants.

- [ ] **Step 5: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git commit -am "refactor: move key/mouse dispatch from App into Screen"
```

---

## Task 5: Create Tabs widget with TabPage

**Files:**
- Create: `crates/flotilla-tui/src/widgets/tabs.rs`
- Modify: `crates/flotilla-tui/src/widgets/screen.rs` (Screen owns Tabs instead of BaseView)
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (add module)
- Modify: `crates/flotilla-tui/src/app/mod.rs` (construct TabPages from repos)
- Modify: `crates/flotilla-tui/src/app/navigation.rs` (move into Tabs or adapt)

At this point, BaseView still exists as the content renderer. Tabs wraps it for now — each TabPage's content is a reference back to BaseView's rendering. The actual page widgets (RepoPage, OverviewPage) come in Tasks 6 and 7.

- [ ] **Step 1: Define TabPage and Tabs**

Create `crates/flotilla-tui/src/widgets/tabs.rs`:

```rust
use std::any::Any;
use crossterm::event::{MouseEvent, MouseEventKind, MouseButton};
use ratatui::{layout::Rect, Frame};
use flotilla_protocol::RepoIdentity;

use super::{InteractiveWidget, Outcome, RenderContext, WidgetContext, AppAction};
use crate::app::ui_state::DragState;
use crate::keymap::{Action, ModeId};

/// Label metadata for a tab.
pub struct TabLabel {
    pub text: String,
    pub has_unseen_changes: bool,
    pub has_change_requests: bool,
}

/// A tab page pairs label metadata with a content widget.
pub struct TabPage {
    pub label: TabLabel,
    pub repo_identity: Option<RepoIdentity>, // None for overview page
    pub content: Box<dyn InteractiveWidget>,
}

/// The tab container: renders the tab bar strip and delegates to the active page.
pub struct Tabs {
    pub pages: Vec<TabPage>,
    pub active: usize,
    pub drag: DragState,
    // Click target areas populated during render
    tab_areas: Vec<(usize, Rect)>,
    add_button_area: Option<Rect>,
}
```

- [ ] **Step 2: Implement tab bar rendering**

Port rendering logic from `crates/flotilla-tui/src/widgets/tab_bar.rs` into `Tabs::render()`. The tab bar is the first row; the remaining area is delegated to the active page's content widget.

- [ ] **Step 3: Implement input routing**

`Tabs::handle_action()` delegates to `self.pages[self.active].content.handle_action()`. Mouse events are hit-tested against `tab_areas` for tab switching, and against the content area for delegation.

- [ ] **Step 4: Move StatusBarWidget from BaseView to Screen**

Currently `StatusBarWidget` is owned by `BaseView`. Move it to `Screen` directly, so Screen renders it after Tabs content and before modals. Update `Screen::render()` to call `self.status_bar.render_bespoke(...)` between the content area and modal rendering.

- [ ] **Step 5: Integrate Tabs into Screen**

Screen now owns `Tabs` instead of (or alongside) `BaseView`. During this transitional step, the overview page and repo pages can still delegate to BaseView's rendering internals. The goal is to get the tab bar and page routing working through Tabs.

```rust
pub struct Screen {
    pub tabs: Tabs,
    pub status_bar: StatusBarWidget,
    pub modal_stack: Vec<Box<dyn InteractiveWidget>>,
}
```

Screen renders: tabs (with tab bar + active page content), then status bar, then modals.

- [ ] **Step 6: Move tab navigation from App into Tabs**

`App::next_tab()`, `prev_tab()`, `switch_tab()`, `move_tab()` in `navigation.rs` become methods on `Tabs`. AppAction variants `PrevTab`/`NextTab`/etc. still go through `process_app_actions` but now call `self.screen.tabs.next_tab()` etc.

- [ ] **Step 7: Construct TabPages in App::new()**

When App is constructed, create a TabPage per repo (with a placeholder content widget that delegates to the old rendering path), plus the overview TabPage.

- [ ] **Step 8: Tab label lifecycle**

Tab labels need to reflect current state (unseen changes, change request presence). Two update paths:

- **On snapshot**: When `App::apply_snapshot()` runs, it updates the `TabPage.label` for the affected repo. Compare new providers against old to detect changes for inactive tabs (existing `has_unseen_changes` logic). Set `has_change_requests` based on provider data.
- **On tab switch**: When a tab becomes active, clear its `has_unseen_changes` flag.

Add a helper `Tabs::update_label_for_repo(&mut self, identity: &RepoIdentity, f: impl FnOnce(&mut TabLabel))` for the snapshot handler to use.

- [ ] **Step 9: Remove TabBar widget**

`crates/flotilla-tui/src/widgets/tab_bar.rs` is now absorbed into `Tabs`. Remove it and update `mod.rs`.

- [ ] **Step 10: Write tests for Tabs**

Add tests in `tabs.rs`:
- `next()`/`prev()` wrap correctly (including overview page at index 0)
- `switch_tab()` clears `has_unseen_changes` on the target tab
- `move_tab()` reorders pages correctly and preserves active index
- Mouse hit-test routes clicks to correct tab

- [ ] **Step 11: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 12: Commit**

```bash
git commit -am "refactor: introduce Tabs widget with TabPage, absorb TabBar"
```

---

## Task 6a: Move selection state into WorkItemTable

**Files:**
- Modify: `crates/flotilla-tui/src/widgets/work_item_table.rs`
- Modify: `crates/flotilla-tui/src/app/ui_state.rs`

This is an isolated preparatory step — WorkItemTable owns its selection, making the later RepoPage migration simpler.

- [ ] **Step 1: Add selection and data fields to WorkItemTable**

```rust
pub struct WorkItemTable {
    pub table_state: TableState,
    pub selected_identity: Option<WorkItemIdentity>,
    pub grouped_items: GroupedWorkItems,
    pub selected_selectable_idx: Option<usize>,
    // existing fields: table_area, gear_area
}
```

- [ ] **Step 2: Add update_items() method**

Port the reconciliation logic from `RepoUiState::update_table_view()` into `WorkItemTable::update_items()`:

```rust
pub fn update_items(&mut self, items: GroupedWorkItems) {
    // Save previous selection by identity
    let prev_identity = /* lookup current selected_selectable_idx in old grouped_items */;
    self.grouped_items = items;
    // Restore selection: find prev_identity in new data, or fall back to first item
    // Update table_state.select() to match
    // (Same logic as RepoUiState::update_table_view())
}
```

- [ ] **Step 3: Write tests for selection preservation**

```rust
#[test]
fn update_items_preserves_selection_by_identity() {
    let mut table = WorkItemTable::new();
    // Set up initial items, select second item
    // Update with reordered items (same identities, different order)
    // Assert selection followed the identity, not the index
}

#[test]
fn update_items_falls_back_to_first_when_selected_removed() {
    // Select an item, update with items that don't include it
    // Assert selection moved to first item
}

#[test]
fn update_items_clears_selection_when_empty() {
    // Select an item, update with empty items
    // Assert selection is None
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p flotilla-tui --lib widgets::work_item_table -- --nocapture`

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor: move selection state and reconciliation into WorkItemTable"
```

---

## Task 6b: Create RepoPage with Shared<RepoData>

**Files:**
- Create: `crates/flotilla-tui/src/widgets/repo_page.rs`
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (add module)

Create the RepoPage widget. At this step it is created and tested in isolation — wiring into App/Tabs comes in 6c.

- [ ] **Step 1: Define RepoData**

```rust
// In a new file or in repo_page.rs
pub struct RepoData {
    pub path: PathBuf,
    pub providers: Arc<ProviderData>,
    pub labels: RepoLabels,
    pub provider_health: HashMap<String, HashMap<String, bool>>,
    pub work_items: Vec<WorkItem>,
    pub issue_has_more: bool,
    pub issue_total: Option<usize>,
    pub loading: bool,
}
```

- [ ] **Step 2: Create RepoPage struct**

```rust
pub struct RepoPage {
    pub repo_identity: RepoIdentity,
    repo_data: Shared<RepoData>,
    pub table: WorkItemTable,
    pub preview: PreviewPanel,
    pub multi_selected: HashSet<WorkItemIdentity>,
    pub pending_actions: HashMap<WorkItemIdentity, PendingAction>,
    pub layout: RepoViewLayout,
    pub show_providers: bool,
    last_seen_generation: u64,
    double_click: DoubleClickState,
}
```

- [ ] **Step 3: Implement reconciliation**

```rust
impl RepoPage {
    fn reconcile_if_changed(&mut self) {
        if let Some(data) = self.repo_data.changed(&mut self.last_seen_generation) {
            let section_labels = SectionLabels::from(&data.labels);
            let grouped = data::group_work_items(&data.work_items, &data.providers, &section_labels, &data.path);
            self.table.update_items(grouped);
            // Prune stale multi_selected
            let current: HashSet<_> = /* collect identities from new items */;
            self.multi_selected.retain(|id| current.contains(id));
        }
    }
}
```

- [ ] **Step 4: Implement InteractiveWidget for RepoPage**

Port from `BaseView`:
- `render()`: the table + preview layout from `render_content()` (including `resolve_preview_position()`, layout constants). **CycleLayout** is handled here as a page-scoped action — RepoPage owns its `layout` field and cycles it directly.
- `handle_action()`: Normal-mode actions (SelectNext/Prev, ToggleMultiSelect, Dismiss cascade, etc.)
- `handle_mouse()`: table row clicks, double-click detection, preview area mouse

- [ ] **Step 5: Write tests for RepoPage reconciliation**

```rust
#[test]
fn reconcile_rebuilds_table_on_data_change() {
    let data = Shared::new(RepoData { /* one work item */ });
    let mut page = RepoPage::new(identity, data.clone(), RepoViewLayout::Auto);
    page.reconcile_if_changed(); // initial
    assert_eq!(page.table.grouped_items.table_entries.len(), /* expected */);

    data.mutate(|d| d.work_items.push(/* another item */));
    page.reconcile_if_changed();
    assert!(page.table.grouped_items.table_entries.len() > /* previous */);
}

#[test]
fn reconcile_prunes_stale_multi_select() {
    // Select items, remove one from data, reconcile
    // Assert removed item no longer in multi_selected
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p flotilla-tui --lib widgets::repo_page -- --nocapture`

- [ ] **Step 7: Commit**

```bash
git commit -am "feat: create RepoPage widget with Shared<RepoData> and reconciliation"
```

---

## Task 6c: Wire RepoPage into App and Tabs

**Files:**
- Modify: `crates/flotilla-tui/src/app/mod.rs`
- Modify: `crates/flotilla-tui/src/app/ui_state.rs`

- [ ] **Step 1: Create Shared<RepoData> handles in App**

Add a `repo_data_handles: HashMap<RepoIdentity, Shared<RepoData>>` to App (or a dedicated `AppData` struct). In `App::new()`, create a `Shared<RepoData>` per repo and pass to `RepoPage` constructors.

- [ ] **Step 2: Update apply_snapshot to mutate Shared<RepoData>**

Instead of writing to `TuiRepoModel` fields and `rui.update_table_view()`, call:

```rust
if let Some(handle) = self.repo_data_handles.get(&repo_identity) {
    handle.mutate(|d| {
        d.providers = Arc::new(snap.providers);
        d.work_items = snap.work_items;
        d.issue_has_more = snap.issue_has_more;
        // ...
    });
}
```

- [ ] **Step 3: Construct RepoPages as TabPage content**

In `App::new()`:
```rust
let repo_data = Shared::new(RepoData::default());
self.repo_data_handles.insert(identity.clone(), repo_data.clone());
let page = RepoPage::new(identity.clone(), repo_data, layout);
tabs.pages.push(TabPage { label, repo_identity: Some(identity), content: Box::new(page) });
```

- [ ] **Step 4: Update handle_repo_added / handle_repo_removed**

When added: create `Shared<RepoData>`, construct `RepoPage`, push `TabPage`.
When removed: remove from `repo_data_handles`, find and remove `TabPage` from `Tabs.pages`.

- [ ] **Step 5: Remove migrated fields from RepoUiState**

Remove `table_view`, `table_state`, `selected_selectable_idx`, `multi_selected`, `pending_actions`, `show_providers` from `RepoUiState`. If only `has_unseen_changes` and `active_search_query` remain, keep the struct temporarily — `has_unseen_changes` moves to `TabLabel` (already done in Task 5), `active_search_query` moves to RepoPage. Then delete `RepoUiState` if empty.

- [ ] **Step 6: Update tests**

Many tests reference `RepoUiState` fields. Update to access RepoPage state instead. This is mechanical but extensive.

- [ ] **Step 7: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git commit -am "refactor: wire RepoPage into App/Tabs, remove migrated RepoUiState fields"
```

---

## Task 7: Create OverviewPage

**Files:**
- Create: `crates/flotilla-tui/src/widgets/overview_page.rs`
- Create: `crates/flotilla-tui/src/widgets/providers_widget.rs`
- Create: `crates/flotilla-tui/src/widgets/hosts_widget.rs`
- Modify: `crates/flotilla-tui/src/widgets/event_log.rs` (slim down)
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (add modules)

- [ ] **Step 1: Extract ProvidersWidget**

Port the provider status table rendering from `EventLogWidget::render()` (the left-top section that shows provider health across repos) into a standalone `ProvidersWidget`. It reads from `Shared<ProviderStatuses>`.

- [ ] **Step 2: Extract HostsWidget**

Port the hosts status table rendering from `EventLogWidget::render()` (the left-bottom section) into a standalone `HostsWidget`. It reads from `Shared<HostsData>`.

- [ ] **Step 3: Create OverviewPage**

```rust
pub struct OverviewPage {
    provider_statuses: Shared<ProviderStatuses>,
    hosts: Shared<HostsData>,
    providers_widget: ProvidersWidget,
    hosts_widget: HostsWidget,
    event_log: EventLogWidget,
    last_seen_providers_gen: u64,
    last_seen_hosts_gen: u64,
}
```

Layout: two-column — left column has providers (top) and hosts (bottom), right column has event log. Port the layout logic from the existing `EventLogWidget::render()`.

Implement `InteractiveWidget` — handle action for event log navigation/filter cycling, render the composed layout.

- [ ] **Step 4: Slim down EventLogWidget**

Remove the provider/host rendering from `EventLogWidget`. It now only renders the event log list with level filtering. It keeps its `selected`, `filter`, `filter_area` state.

- [ ] **Step 5: Wire OverviewPage as the Flotilla tab content**

Create `Shared<ProviderStatuses>` and `Shared<HostsData>` in App, pass to OverviewPage. The Flotilla tab's `TabPage.content` is `Box::new(OverviewPage::new(...))`.

Update `App::apply_snapshot()` to mutate the shared provider statuses and host data.

- [ ] **Step 6: Remove UiMode::Config**

The overview is now just the active tab page — no mode switching needed. Remove `UiMode::Config` and all `is_config()` checks. Also check `UiMode::IssueSearch` — the `IssueSearchWidget` already exists as a modal `InteractiveWidget`. If `UiMode::IssueSearch` is dead code (no remaining references), remove it. If there are still references, migrate them to use the modal widget instead. If `UiMode` becomes empty after removing Config and IssueSearch, delete the enum entirely.

- [ ] **Step 7: Write tests for OverviewPage**

```rust
#[test]
fn overview_page_renders_without_panic_when_empty() {
    let statuses = Shared::new(ProviderStatuses::default());
    let hosts = Shared::new(HostsData::default());
    let mut page = OverviewPage::new(statuses, hosts);
    // Render into a test terminal buffer — should not panic
}

#[test]
fn overview_page_reconciles_on_provider_data_change() {
    let statuses = Shared::new(ProviderStatuses::default());
    let hosts = Shared::new(HostsData::default());
    let mut page = OverviewPage::new(statuses.clone(), hosts);
    statuses.mutate(|s| { /* add a provider status */ });
    // Verify page picks up the change on next render
}
```

- [ ] **Step 8: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 9: Commit**

```bash
git commit -am "refactor: create OverviewPage with ProvidersWidget, HostsWidget, EventLogWidget"
```

---

## Task 8: Delete BaseView

**Files:**
- Delete: `crates/flotilla-tui/src/widgets/base_view.rs`
- Modify: `crates/flotilla-tui/src/widgets/screen.rs` (Screen no longer wraps BaseView)
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (remove `pub mod base_view;`)
- Modify: any remaining references

- [ ] **Step 1: Verify BaseView is no longer referenced**

Search for all references to `base_view` and `BaseView` in the codebase. At this point, Screen should have absorbed all of BaseView's responsibilities:
- Tab bar rendering → Tabs
- Content rendering → RepoPage / OverviewPage
- Status bar → Screen owns it directly
- Double-click detection → RepoPage
- Drag state → Tabs
- Two-phase dispatch → Screen (global) + page widgets (local)

- [ ] **Step 2: Remove BaseView and update Screen**

Delete `base_view.rs`. Remove `pub mod base_view;` from `mod.rs`. Screen's struct should now look like:

```rust
pub struct Screen {
    pub tabs: Tabs,
    pub status_bar: StatusBarWidget,
    pub modal_stack: Vec<Box<dyn InteractiveWidget>>,
}
```

- [ ] **Step 3: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 4: Commit**

```bash
git commit -am "refactor: delete BaseView — replaced by Screen, Tabs, RepoPage, OverviewPage"
```

---

## Task 9: Clean up UiState and RepoUiState

**Files:**
- Modify: `crates/flotilla-tui/src/app/ui_state.rs`
- Modify: `crates/flotilla-tui/src/app/mod.rs`
- Modify: `crates/flotilla-tui/src/widgets/mod.rs` (slim WidgetContext and RenderContext)

- [ ] **Step 1: Remove RepoUiState if empty**

If all per-repo fields have moved into RepoPage/WorkItemTable, delete `RepoUiState` entirely. Remove `repo_ui: HashMap<RepoIdentity, RepoUiState>` from `UiState`.

If some fields remain (e.g. `active_search_query`), move them into RepoPage and then delete.

- [ ] **Step 2: Remove UiMode if eliminated**

If `UiMode` is gone (Normal/Config replaced by tab structure, IssueSearch replaced by modal), remove the enum and all references.

- [ ] **Step 3: Slim down UiState**

Remove fields that moved elsewhere:
- `view_layout` → per-RepoPage (or stays global on Screen if we kept it global)
- `mode` → gone
- `repo_ui` → gone
- `target_host` → stays (multi-host routing is app-level)
- `status_bar` → may move to StatusBarWidget
- `show_debug`, `help_scroll` → may move to relevant widgets

Whatever remains is the new `UiState`. It may be small enough to fold into `App` directly.

- [ ] **Step 4: Slim down WidgetContext**

Remove fields that widgets no longer need:
- `model` → widgets read `Shared<T>` handles
- `repo_ui` → gone
- `mode` → gone
- `active_repo`, `repo_order` → available via Tabs

New `WidgetContext` keeps `config`, `in_flight`, and `target_host` beyond the spec's minimal version — widgets need `config` for action resolution, `in_flight` for pending state display, and `target_host` for command construction:

```rust
pub struct WidgetContext<'a> {
    pub commands: &'a mut CommandQueue,
    pub app_actions: Vec<AppAction>,
    pub keymap: &'a Keymap,
    pub config: &'a ConfigStore,
    pub in_flight: &'a HashMap<u64, InFlightCommand>,
    pub target_host: Option<&'a HostName>,
}
```

- [ ] **Step 4a: Decompose LayoutAreas**

`UiState` currently owns `LayoutAreas` (table_area, tab_areas, status_bar area, etc.) as a centralized hit-test cache. In the new design, each widget stores its own click-target areas — `WorkItemTable` already has `table_area` and `gear_area`, `Tabs` will have `tab_areas`, `StatusBarWidget` will have its layout. Remove `LayoutAreas` from `UiState` and ensure each widget stores its own areas (populated during render, used during mouse handling).

- [ ] **Step 5: Slim down RenderContext**

```rust
pub struct RenderContext<'a> {
    pub theme: &'a Theme,
    pub keymap: &'a Keymap,
    pub in_flight: &'a HashMap<u64, InFlightCommand>,
    pub active_widget_mode: Option<ModeId>,
    pub active_widget_data: WidgetStatusData,
}
```

- [ ] **Step 6: Update all widget signatures**

All widgets that use the removed `WidgetContext`/`RenderContext` fields need updating. This is mechanical — find and fix all compiler errors.

- [ ] **Step 7: Run full test suite and CI checks**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git commit -am "refactor: remove RepoUiState, UiMode, slim down contexts"
```

---

## Notes for implementers

- **Read the spec** at `docs/superpowers/specs/2026-03-20-widget-tree-restructure-design.md` before starting. It has the full rationale.
- **Run CI after every task**: `cargo +nightly-2026-03-12 fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
- **Snapshot tests**: If any insta snapshots fail, investigate whether the change is expected from the restructure. Don't blindly accept.
- **Task dependencies**: Task 1 is independent. Tasks 2→3→4a→4b are sequential. Task 5 depends on 4b. Task 6a is independent (can start after Task 5 or even earlier). Tasks 6b→6c depend on 6a and Task 5. Task 7 depends on 6c (builds on the same patterns). Tasks 8 and 9 depend on 6c+7.
- **The hardest tasks are 4b and 6c.** Task 4b (moving dispatch) touches the most code paths. Task 6c (wiring RepoPage into App) is where the state ownership model actually changes. Take extra care with these.
- **Keep the app working after each task.** If something isn't compiling, don't move on — fix it first.
- **Existing modals (CommandPalette, Help, FilePicker, etc.)** require no changes to their InteractiveWidget implementations — they continue to work as modal stack entries on Screen. Only their host (widget_stack on App → modal_stack on Screen) changes.
- **CycleLayout is page-scoped**, not global. Each RepoPage owns its layout and handles CycleLayout directly. The persist-to-config action can go through AppAction if needed.
