# Duplication Extraction Batch 1 — Quick Wins

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract four independent duplication hotspots identified in issue #225: ahead/behind parser (A6), popup utility (B9), transport framing (C2), and spawn lock RAII guard (C4).

**Architecture:** Each task is a self-contained extraction that moves duplicated code into a shared helper/type. Tasks 1, 3 are fully independent. Tasks 2 and 4 both touch `flotilla-client/src/lib.rs` — run them sequentially (Task 2 first, then Task 4) or in separate worktrees.

**Tech Stack:** Rust, tokio (async I/O for C2), ratatui (TUI widgets for B9), serde_json (serialization for C2)

---

## Chunk 1: A6 + C4 (core crate + client crate)

### Task 1: Extract `parse_ahead_behind` into VCS module (A6)

**Context:** The ahead/behind parsing logic is duplicated between `git.rs` (lines 109-113) and `git_worktree.rs` (lines 147-153). Both parse `git rev-list --count --left-right` output by trimming, splitting on tab, and parsing two i64 values. The `git_worktree.rs` version is already a standalone function; `git.rs` has it inline. Move the `git_worktree.rs` version into `vcs/mod.rs` and call it from both sites.

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/mod.rs`
- Modify: `crates/flotilla-core/src/providers/vcs/git.rs:106-114`
- Modify: `crates/flotilla-core/src/providers/vcs/git_worktree.rs:147-153`

- [ ] **Step 1: Write test for the shared parser in `vcs/mod.rs`**

Add to the existing `#[cfg(test)] mod tests` block in `crates/flotilla-core/src/providers/vcs/mod.rs`:

```rust
#[test]
fn parse_ahead_behind_normal() {
    let ab = parse_ahead_behind("3\t5\n").expect("should parse");
    assert_eq!(ab.ahead, 3);
    assert_eq!(ab.behind, 5);
}

#[test]
fn parse_ahead_behind_zeros() {
    let ab = parse_ahead_behind("0\t0\n").expect("should parse");
    assert_eq!(ab.ahead, 0);
    assert_eq!(ab.behind, 0);
}

#[test]
fn parse_ahead_behind_empty() {
    assert!(parse_ahead_behind("").is_none());
}

#[test]
fn parse_ahead_behind_malformed() {
    assert!(parse_ahead_behind("notanumber\t5").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core parse_ahead_behind`
Expected: FAIL — `parse_ahead_behind` not found in `vcs/mod.rs`

- [ ] **Step 3: Move the function into `vcs/mod.rs`**

Add this function to `crates/flotilla-core/src/providers/vcs/mod.rs` (after `parse_porcelain_status`, before the `parse_issue_config_output` function):

```rust
/// Parse the output of `git rev-list --count --left-right` into an `AheadBehind`.
///
/// Output format is `<ahead>\t<behind>\n`.
pub(crate) fn parse_ahead_behind(output: &str) -> Option<AheadBehind> {
    let trimmed = output.trim();
    let mut parts = trimmed.split('\t');
    let ahead: i64 = parts.next()?.parse().ok()?;
    let behind: i64 = parts.next()?.parse().ok()?;
    Some(AheadBehind { ahead, behind })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-core parse_ahead_behind`
Expected: PASS — all 4 new tests green

- [ ] **Step 5: Update `git.rs` to call the shared function**

Replace lines 106-114 in `crates/flotilla-core/src/providers/vcs/git.rs`:

```rust
async fn ahead_behind(&self, repo_root: &Path, branch: &str, reference: &str) -> Result<AheadBehind, String> {
    let range = format!("{}...{}", branch, reference);
    let output = run!(self.runner, "git", &["rev-list", "--count", "--left-right", &range], repo_root)?;
    super::parse_ahead_behind(&output).ok_or_else(|| format!("failed to parse ahead/behind from: {output:?}"))
}
```

**Note:** This is a minor semantic change — previously `git.rs` used `unwrap_or(0)` for malformed values (silently defaulting), now it returns `Err`. This is strictly better behaviour.

- [ ] **Step 6: Remove the local `parse_ahead_behind` from `git_worktree.rs`**

Delete the `parse_ahead_behind` function (lines 147-153) from `crates/flotilla-core/src/providers/vcs/git_worktree.rs` and update its call sites to use `super::parse_ahead_behind` instead.

Also delete the now-redundant tests `parse_ahead_behind_valid` and `parse_ahead_behind_empty` from the test module in `git_worktree.rs` (lines 312-324), since equivalent tests are now in `vcs/mod.rs`.

- [ ] **Step 7: Run full VCS test suite**

Run: `cargo test -p flotilla-core -- vcs`
Expected: PASS — all existing tests still green

- [ ] **Step 8: Commit**

```bash
git add crates/flotilla-core/src/providers/vcs/mod.rs crates/flotilla-core/src/providers/vcs/git.rs crates/flotilla-core/src/providers/vcs/git_worktree.rs
git commit -m "refactor: extract shared parse_ahead_behind into vcs module (A6, #225)"
```

---

### Task 2: Extract spawn lock RAII guard (C4)

**Context:** In `crates/flotilla-client/src/lib.rs`, the `connect_or_spawn` function (lines 220-323) has a 3-line lock cleanup pattern repeated 3 times (lines 295-298, 309-312, 316-319): check if lock is held, drop file handle, remove lock file. Replace with a RAII guard that does cleanup on drop.

**Files:**
- Modify: `crates/flotilla-client/src/lib.rs:220-323`
- Modify: `crates/flotilla-client/Cargo.toml` (add `tempfile` dev-dependency)

- [ ] **Step 0: Add `tempfile` dev-dependency**

Add to `crates/flotilla-client/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 1: Write test for the guard type**

Add a test at the bottom of `crates/flotilla-client/src/lib.rs` (in the existing test module, or create one if none exists):

```rust
#[cfg(test)]
mod spawn_lock_tests {
    use super::*;
    use std::fs;

    #[test]
    fn spawn_lock_guard_removes_file_on_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lock_path = dir.path().join("test.lock");
        fs::write(&lock_path, "").expect("create lock file");
        let file = fs::File::open(&lock_path).expect("open lock file");
        {
            let _guard = SpawnLockGuard::new(file, lock_path.clone());
            assert!(lock_path.exists(), "lock file should exist while guard is held");
        }
        assert!(!lock_path.exists(), "lock file should be removed after guard drops");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-client spawn_lock`
Expected: FAIL — `SpawnLockGuard` not found

- [ ] **Step 3: Implement `SpawnLockGuard`**

Add this struct near the top of `crates/flotilla-client/src/lib.rs` (after imports, before `connect_or_spawn`):

```rust
/// RAII guard that removes a lock file when dropped.
///
/// Holds the open file handle (which keeps the OS flock) and removes the
/// lock file on drop.
struct SpawnLockGuard {
    _file: std::fs::File,
    path: PathBuf,
}

impl SpawnLockGuard {
    fn new(file: std::fs::File, path: PathBuf) -> Self {
        Self { _file: file, path }
    }
}

impl Drop for SpawnLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-client spawn_lock`
Expected: PASS

- [ ] **Step 5: Refactor `connect_or_spawn` to use the guard**

In `connect_or_spawn`, replace `let mut lock_file = None;` (line 237) and all subsequent lock management code:

1. Change `lock_file` from `Option<File>` to `Option<SpawnLockGuard>`:
   ```rust
   let mut lock_guard: Option<SpawnLockGuard> = None;
   ```

2. Where the lock is acquired (line 244), wrap in guard:
   ```rust
   Ok(Some(file)) => {
       lock_guard = Some(SpawnLockGuard::new(file, lock_path.clone()));
       break;
   }
   ```
   And similarly at line 267.

3. Delete all three manual cleanup blocks (lines 295-298, 309-312, 316-319). The guard's `Drop` impl handles cleanup automatically. The code at each exit point becomes just the return statement.

- [ ] **Step 6: Run full client test suite + build check**

Run: `cargo test -p flotilla-client && cargo clippy -p flotilla-client`
Expected: PASS, no warnings

- [ ] **Step 7: Commit**

```bash
git add crates/flotilla-client/Cargo.toml crates/flotilla-client/src/lib.rs
git commit -m "refactor: replace spawn lock cleanup with RAII guard (C4, #225)"
```

---

## Chunk 2: B9 + C2 (TUI crate + protocol/client/daemon crates)

### Task 3: Extract popup frame helper (B9)

**Context:** Six popup rendering functions in `crates/flotilla-tui/src/ui.rs` all repeat the same 2-3 line setup: `popup_area(...)`, `Clear`, `Block::bordered().title(...)`. Extract a helper that does the common setup and returns the inner area for content rendering.

The six sites are:
- `render_action_menu` (line 681-683) — stores area in `ui.layout.menu_area`
- `render_input_popup` (line 704-709) — uses `inner_area`
- `render_delete_confirm` (line 732-733)
- `render_close_confirm` (line 825-826)
- `render_help` (line 846-847)
- `render_file_picker` (line 929-935) — stores area in `ui.layout.file_picker_area`

Some popups use `block.inner(area)` for content (input_popup, file_picker), others render directly into `area` (the bordered block wraps content via `.block(Block::bordered().title(...))` on the Paragraph widget). The helper should handle the common `popup_area` + `Clear` sequence and return the area, since the block usage varies by site.

**Files:**
- Modify: `crates/flotilla-tui/src/ui_helpers.rs`
- Modify: `crates/flotilla-tui/src/ui.rs`

- [ ] **Step 1: Write test for the popup frame helper**

Add to the test module in `crates/flotilla-tui/src/ui_helpers.rs`:

```rust
#[test]
fn popup_frame_returns_inner_area() {
    let area = Rect::new(0, 0, 100, 50);
    let (popup, inner) = popup_frame(area, 50, 50, " Test ");
    // Popup should be centered
    assert!(popup.x > 0);
    assert!(popup.y > 0);
    // Inner should be inset by border (1px each side)
    assert_eq!(inner.x, popup.x + 1);
    assert_eq!(inner.y, popup.y + 1);
    assert_eq!(inner.width, popup.width - 2);
    assert_eq!(inner.height, popup.height - 2);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-tui popup_frame`
Expected: FAIL — `popup_frame` not found

- [ ] **Step 3: Implement `popup_frame` in `ui_helpers.rs`**

Add after the existing `popup_area` function in `crates/flotilla-tui/src/ui_helpers.rs`:

```rust
/// Calculate a centered popup area and its bordered inner area.
///
/// Returns `(outer_area, inner_area)` where `inner_area` is the content area
/// inside a `Block::bordered()` with the given title.
pub fn popup_frame(container: Rect, percent_x: u16, percent_y: u16, title: &str) -> (Rect, Rect) {
    let area = popup_area(container, percent_x, percent_y);
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    (area, inner)
}
```

Add `Block` to the ratatui imports at the top of ui_helpers.rs:

```rust
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::Color,
    widgets::Block,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-tui popup_frame`
Expected: PASS

- [ ] **Step 5: Add `render_popup_frame` helper for the rendering step**

The calculation helper above doesn't render — we also need a rendering helper. Add to `ui_helpers.rs`:

```rust
/// Render a popup frame: clear the area and draw a bordered block with title.
/// Returns `(outer_area, inner_area)` for the caller to render content into.
pub fn render_popup_frame(frame: &mut ratatui::Frame, container: Rect, percent_x: u16, percent_y: u16, title: &str) -> (Rect, Rect) {
    let area = popup_area(container, percent_x, percent_y);
    frame.render_widget(ratatui::widgets::Clear, area);
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    (area, inner)
}
```

- [ ] **Step 6: Update popup sites in `ui.rs` to use the helper**

Replace the duplicated setup in each function. For example, in `render_input_popup`:

Before:
```rust
let area = ui_helpers::popup_area(frame.area(), 50, 20);
frame.render_widget(Clear, area);

let inner = Block::bordered().title(" New Branch ");
let inner_area = inner.inner(area);
frame.render_widget(inner, area);
```

After:
```rust
let (_area, inner_area) = ui_helpers::render_popup_frame(frame, frame.area(), 50, 20, " New Branch ");
```

For `render_file_picker`:
```rust
let (area, inner) = ui_helpers::render_popup_frame(frame, frame.area(), 60, 60, " Add Repository ");
ui.layout.file_picker_area = area;
```

For popups that render block via `.block(Block::bordered().title(...))` on the Paragraph (action_menu, delete_confirm, close_confirm, help), use the simpler `popup_area` + `Clear` extraction:
```rust
let area = ui_helpers::popup_area(frame.area(), 40, 40);
ui.layout.menu_area = area;
frame.render_widget(Clear, area);
```
These already use `popup_area`, so just the `Clear` line is the repetition. For these sites, the `render_popup_frame` helper is not the right fit because they pass `Block::bordered()` to the widget's `.block()` method (not rendering it separately). Leave these sites using the existing `popup_area` + `Clear` two-liner — the duplication there is minimal (one line of `Clear`) and the widget's `.block()` API is the idiomatic ratatui pattern.

**Apply `render_popup_frame` only to the two sites that manually split block/inner:**
- `render_input_popup` (lines 704-709)
- `render_file_picker` (lines 929-935)

- [ ] **Step 7: Run full TUI test suite + clippy**

Run: `cargo test -p flotilla-tui && cargo clippy -p flotilla-tui`
Expected: PASS, no warnings

- [ ] **Step 8: Commit**

```bash
git add crates/flotilla-tui/src/ui_helpers.rs crates/flotilla-tui/src/ui.rs
git commit -m "refactor: extract render_popup_frame helper for popup setup (B9, #225)"
```

---

### Task 4: Extract shared transport framing into protocol crate (C2)

**Context:** Three crates independently implement JSON-lines message framing (serialize → write bytes → write newline → flush). The pattern appears in:
- `flotilla-client/src/lib.rs:355-362` — inline in `send_request`, uses `tokio::sync::Mutex<BufWriter<OwnedWriteHalf>>`
- `flotilla-daemon/src/server.rs:812-820` — `write_message` helper, uses `tokio::sync::Mutex<BufWriter<OwnedWriteHalf>>`
- `flotilla-daemon/src/peer/ssh_transport.rs:352-358` — `write_message_line` helper, uses `&mut UnixStream`
- Plus 4 inline instances in server.rs test code (lines 1388-1391, 1419-1422, 1543-1546, 1622-1625)

The protocol crate (`flotilla-protocol`) already owns `Message` and `serde_json` but doesn't have tokio. Since the framing is async I/O, the helper should live in a new module that depends on tokio's `AsyncWrite`. The protocol crate is the natural home since it already defines the wire format.

**Files:**
- Modify: `crates/flotilla-protocol/Cargo.toml`
- Create: `crates/flotilla-protocol/src/framing.rs`
- Modify: `crates/flotilla-protocol/src/lib.rs`
- Modify: `crates/flotilla-client/src/lib.rs`
- Modify: `crates/flotilla-daemon/src/server.rs`
- Modify: `crates/flotilla-daemon/src/peer/ssh_transport.rs`

- [ ] **Step 1: Add tokio dependency to protocol crate (minimal features)**

The workspace tokio dependency uses `features = ["full", "test-util"]` which is too heavy for a lightweight protocol crate. Use a targeted dependency instead.

Add to `crates/flotilla-protocol/Cargo.toml` under `[dependencies]`:

```toml
tokio = { version = "1", features = ["io-util"] }
```

And add under `[dev-dependencies]` (for `#[tokio::test]`):

```toml
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
```

- [ ] **Step 2: Create `framing.rs` with the write helper**

Create `crates/flotilla-protocol/src/framing.rs`:

```rust
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::Message;

/// Write a `Message` as a JSON line (JSON text + newline + flush).
///
/// This is the canonical framing used on all flotilla wire connections
/// (client↔daemon, peer↔peer). The receiver reads with `tokio::io::AsyncBufReadExt::read_line`
/// or equivalent.
pub async fn write_message_line(writer: &mut (impl AsyncWrite + Unpin), msg: &Message) -> Result<(), String> {
    let json = serde_json::to_string(msg).map_err(|e| format!("failed to serialize message: {e}"))?;
    writer.write_all(json.as_bytes()).await.map_err(|e| format!("failed to write message: {e}"))?;
    writer.write_all(b"\n").await.map_err(|e| format!("failed to write newline: {e}"))?;
    writer.flush().await.map_err(|e| format!("failed to flush: {e}"))?;
    Ok(())
}
```

- [ ] **Step 3: Export the module from `lib.rs`**

Add to `crates/flotilla-protocol/src/lib.rs` after the other module declarations:

```rust
pub mod framing;
```

- [ ] **Step 4: Write tests for the framing helper**

Add to the bottom of `crates/flotilla-protocol/src/framing.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::HostName;

    #[tokio::test]
    async fn write_message_line_produces_valid_json_line() {
        let msg = Message::Hello { protocol_version: 1, host_name: HostName::new("test") };
        let mut buf = Vec::new();
        write_message_line(&mut buf, &msg).await.expect("write should succeed");

        let output = String::from_utf8(buf).expect("valid utf-8");
        assert!(output.ends_with('\n'), "should end with newline");
        let trimmed = output.trim_end();
        let parsed: Message = serde_json::from_str(trimmed).expect("should be valid JSON");
        match parsed {
            Message::Hello { protocol_version, host_name } => {
                assert_eq!(protocol_version, 1);
                assert_eq!(host_name, HostName::new("test"));
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn write_message_line_request() {
        let msg = Message::Request { id: 42, method: "subscribe".into(), params: serde_json::json!({}) };
        let mut buf = Vec::new();
        write_message_line(&mut buf, &msg).await.expect("write should succeed");

        let output = String::from_utf8(buf).expect("valid utf-8");
        let parsed: Message = serde_json::from_str(output.trim_end()).expect("valid JSON");
        match parsed {
            Message::Request { id, method, .. } => {
                assert_eq!(id, 42);
                assert_eq!(method, "subscribe");
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p flotilla-protocol framing`
Expected: PASS

- [ ] **Step 6: Update `ssh_transport.rs` to use the shared helper**

In `crates/flotilla-daemon/src/peer/ssh_transport.rs`, replace the `write_message_line` method (lines 352-358):

```rust
async fn write_message_line(stream: &mut UnixStream, msg: &Message) -> Result<(), String> {
    flotilla_protocol::framing::write_message_line(stream, msg).await
}
```

Or remove the wrapper entirely and call `flotilla_protocol::framing::write_message_line` directly at call sites.

- [ ] **Step 7: Update `server.rs` to use the shared helper**

In `crates/flotilla-daemon/src/server.rs`, replace the `write_message` function (lines 812-820). This function takes a `Mutex<BufWriter<...>>` so it locks then writes. Refactor to lock + delegate:

```rust
async fn write_message(writer: &tokio::sync::Mutex<BufWriter<tokio::net::unix::OwnedWriteHalf>>, msg: &Message) -> Result<(), ()> {
    let mut w = writer.lock().await;
    flotilla_protocol::framing::write_message_line(&mut *w, msg).await.map_err(|_| ())
}
```

For the 4 test helper instances (lines 1388-1391, 1419-1422, 1543-1546, 1622-1625), replace each block like:
```rust
let hello_json = serde_json::to_string(&hello).expect("serialize hello");
writer.write_all(hello_json.as_bytes()).await.expect("write");
writer.write_all(b"\n").await.expect("newline");
writer.flush().await.expect("flush");
```
with:
```rust
flotilla_protocol::framing::write_message_line(&mut writer, &hello).await.expect("write hello");
```

- [ ] **Step 8: Update `lib.rs` (client) to use the shared helper**

In `crates/flotilla-client/src/lib.rs`, replace the inline framing in `send_request` (lines 347-362):

```rust
let write_result = async {
    let mut w = writer.lock().await;
    flotilla_protocol::framing::write_message_line(&mut *w, &msg).await
}
.await;
```

This replaces the separate serialize + write_all + newline + flush calls. The `line` variable and its error-handling match block can be removed.

- [ ] **Step 9: Run full test suite + clippy**

Run: `cargo test --workspace && cargo clippy --all-targets`
Expected: PASS, no warnings

- [ ] **Step 10: Commit**

```bash
git add crates/flotilla-protocol/Cargo.toml crates/flotilla-protocol/src/framing.rs crates/flotilla-protocol/src/lib.rs crates/flotilla-client/src/lib.rs crates/flotilla-daemon/src/server.rs crates/flotilla-daemon/src/peer/ssh_transport.rs
git commit -m "refactor: extract shared JSON-lines framing into protocol crate (C2, #225)"
```
