# Close PR Action

Add a "Close PR" action to the TUI action menu, with a confirmation dialog.

## Flow

Follows the existing Intent -> Command -> Executor -> Provider pattern.

### 1. Intent & Menu

Add `Intent::CloseChangeRequest` to the enum in `intent.rs`.

- **Availability:** work item has a change request ID and its status is `Open`
- **Menu:** included in `all_in_menu_order()`, no keyboard shortcut
- **Label:** "Close PR" (or provider-configured equivalent via `labels.code_review.item`)

### 2. Confirmation Mode

Add `UiMode::CloseConfirm { id: String, title: String }` to `ui_state.rs`.

- Shows PR number and title
- `y` or `Enter` confirms (dispatches `Command::CloseChangeRequest`)
- `n` or `Esc` cancels (returns to `Normal` mode)
- No async loading needed — all info available at intent resolution time

### 3. Command & Protocol

Add `Command::CloseChangeRequest { id: String }` to `commands.rs`.

- Description: `"Closing PR..."`

### 4. Provider Trait

Add to `CodeReview` trait in `code_review/mod.rs`:

```rust
async fn close_change_request(&self, repo_root: &Path, id: &str) -> Result<(), String>;
```

GitHub implementation in `code_review/github.rs` runs `gh pr close <id>`.

### 5. Executor

Handle `Command::CloseChangeRequest` in `executor.rs`:

- Call `cr.close_change_request(repo_root, &id).await`
- Return `CommandResult::Ok`

### 6. Tests

- `is_available`: available for open PRs, unavailable for merged/closed/no-PR items
- `resolve`: produces `Command::CloseChangeRequest` with correct ID
- Confirmation flow: intent opens `CloseConfirm` mode, `y` dispatches command, `Esc` cancels

## Files Changed

| File | Change |
|------|--------|
| `crates/flotilla-tui/src/app/intent.rs` | Add variant, availability, resolve, label, menu order |
| `crates/flotilla-tui/src/app/ui_state.rs` | Add `CloseConfirm` mode |
| `crates/flotilla-tui/src/app/key_handlers.rs` | Handle confirm keys, wire intent to confirm mode |
| `crates/flotilla-tui/src/ui.rs` | Render confirmation dialog |
| `crates/flotilla-protocol/src/commands.rs` | Add command variant |
| `crates/flotilla-core/src/providers/code_review/mod.rs` | Add trait method |
| `crates/flotilla-core/src/providers/code_review/github.rs` | Implement with `gh pr close` |
| `crates/flotilla-core/src/executor.rs` | Handle new command |
