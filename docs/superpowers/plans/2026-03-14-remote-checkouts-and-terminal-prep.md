# Remote Checkouts And Terminal Prep Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add target-host-driven remote checkout creation and phase-one remote terminal preparation while keeping presentation-local workspace creation on the TUI host.

**Architecture:** Extend the TUI with an app-level target host selector, apply that selector consistently when stamping `Command.host`, reuse existing remote command routing for checkout creation, and add a terminal-preparation contract that returns attach information from the target host back to the local presentation host. Workspace managers remain local-only; remote hosts only execute and prepare.

**Tech Stack:** Rust, Tokio, serde protocol types, existing `DaemonHandle` / peer routing, ratatui app state and status bar, current multi-host daemon tests.

---

## File Structure

- Modify: `crates/flotilla-tui/src/app/ui_state.rs`
  - Add target-host UI state and defaults.
- Modify: `crates/flotilla-tui/src/app/mod.rs`
  - Thread target-host state through app construction, command helpers, and status-bar interaction plumbing.
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs`
  - Add keyboard host-target cycling.
- Modify: `crates/flotilla-tui/src/app/executor.rs`
  - Apply universal host-targeting rules and preserve local-only action behavior.
- Modify: `crates/flotilla-tui/src/status_bar.rs`
  - Render target-host status affordance and click region.
- Modify: `crates/flotilla-tui/src/app/intent.rs`
  - Reconcile which intents are selector-targeted vs item-fixed vs always local.
- Modify: `crates/flotilla-protocol/src/commands.rs`
  - Add terminal-preparation command/result types if needed.
- Modify: `crates/flotilla-protocol/src/peer.rs`
  - Add any peer-routable terminal-preparation result/event types if needed by remote command transport.
- Modify: `crates/flotilla-core/src/executor.rs`
  - Execute remote terminal preparation on the target host and return attach info.
- Modify: `crates/flotilla-core/src/providers/terminal/mod.rs`
  - Add provider contract for preparing a remotely attachable terminal command.
- Modify: `crates/flotilla-core/src/providers/terminal/passthrough.rs`
  - Implement phase-one passthrough terminal preparation.
- Modify: `crates/flotilla-core/src/providers/workspace/*`
  - Accept prepared attach commands when creating local workspaces.
- Modify: `crates/flotilla-daemon/src/server.rs`
  - Ensure remote checkout and terminal-preparation commands route/complete correctly.
- Modify: `crates/flotilla-daemon/tests/multi_host.rs`
  - Add integration coverage for remote checkout replication and terminal preparation.
- Modify: `crates/flotilla-tui/src/app/*tests*`, `crates/flotilla-core/src/providers/terminal/*tests*`, `crates/flotilla-protocol/src/*tests*`
  - Add focused unit coverage.

## Chunk 1: Target Host State And Remote Checkout Routing

### Task 1: Add failing TUI tests for target-host state

**Files:**
- Modify: `crates/flotilla-tui/src/app/ui_state.rs`
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs`
- Test: `crates/flotilla-tui/src/app/ui_state.rs`
- Test: `crates/flotilla-tui/src/app/key_handlers.rs`

- [ ] **Step 1: Write failing tests for target-host defaults and cycling**

Add tests covering:
- default target host is local
- cycling host target with `h`
- cycling ignores empty host lists cleanly

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-tui --locked target_host -- --nocapture`
Expected: FAIL because target-host state and handler logic do not exist yet.

- [ ] **Step 3: Implement minimal UI state and key handler support**

Add:
- a target-host field in app/UI state
- helper to cycle through known hosts
- `h` key binding to advance target host

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-tui --locked target_host -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-tui/src/app/ui_state.rs crates/flotilla-tui/src/app/key_handlers.rs
git commit -m "feat: add target host app state"
```

### Task 2: Add failing executor tests for universal host targeting

**Files:**
- Modify: `crates/flotilla-tui/src/app/executor.rs`
- Modify: `crates/flotilla-tui/src/app/intent.rs`
- Test: `crates/flotilla-tui/src/app/executor.rs`

- [ ] **Step 1: Write failing tests for command host stamping**

Cover:
- selector-targeted checkout commands pick `Command.host = Some(remote)`
- presentation-local actions stay local
- item-fixed remote actions preserve item host over selector

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-tui --locked executor::tests::remote -- --nocapture`
Expected: FAIL because executor currently does not apply the new targeting rules.

- [ ] **Step 3: Implement minimal executor routing changes**

Update command construction/execution so:
- checkout and other host-executed commands use target host by default
- local-only actions remain unstamped or local
- item-derived host wins when appropriate

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-tui --locked executor::tests::remote -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-tui/src/app/executor.rs crates/flotilla-tui/src/app/intent.rs
git commit -m "feat: route host-executed commands via target host"
```

### Task 3: Add failing daemon/integration tests for remote checkout creation

**Files:**
- Modify: `crates/flotilla-daemon/tests/multi_host.rs`
- Modify: `crates/flotilla-daemon/src/server.rs`
- Possibly modify: `crates/flotilla-core/src/executor.rs`

- [ ] **Step 1: Write failing tests for remote checkout creation and replication**

Add tests proving:
- a checkout command addressed to a remote host is executed remotely
- the resulting checkout replicates back and is attributed to the target host

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-daemon --locked remote_checkout -- --nocapture`
Expected: FAIL because current coverage does not implement or prove the full flow.

- [ ] **Step 3: Implement minimal checkout routing support**

Adjust daemon/core behavior as needed so remote `CommandAction::Checkout`:
- routes through the existing remote command channel
- executes on the target daemon
- yields replication-backed visibility on the requester

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-daemon --locked remote_checkout -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-daemon/tests/multi_host.rs crates/flotilla-daemon/src/server.rs crates/flotilla-core/src/executor.rs
git commit -m "feat: support remote checkout creation"
```

## Chunk 2: Passthrough Remote Terminal Preparation

### Task 4: Add failing protocol/core tests for terminal preparation contract

**Files:**
- Modify: `crates/flotilla-protocol/src/commands.rs`
- Modify: `crates/flotilla-core/src/providers/terminal/mod.rs`
- Test: `crates/flotilla-protocol/src/commands.rs`
- Test: `crates/flotilla-core/src/providers/terminal/mod.rs`

- [ ] **Step 1: Write failing tests for terminal-preparation command/result shape**

Cover:
- protocol roundtrip of terminal-preparation request/result
- provider-facing attach payload structure
- passthrough semantic marker vs durable-session marker if represented

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-protocol --locked terminal_prepare -- --nocapture`
Expected: FAIL because types/roundtrips do not exist yet.

- [ ] **Step 3: Implement minimal protocol and provider contract**

Add:
- command/result types for terminal preparation
- any supporting structs required to carry attach information
- terminal provider trait method for preparing attachable terminal execution

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-protocol --locked terminal_prepare -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-protocol/src/commands.rs crates/flotilla-core/src/providers/terminal/mod.rs
git commit -m "feat: add terminal preparation contract"
```

### Task 5: Add failing passthrough provider tests

**Files:**
- Modify: `crates/flotilla-core/src/providers/terminal/passthrough.rs`
- Test: `crates/flotilla-core/src/providers/terminal/passthrough.rs`

- [ ] **Step 1: Write failing tests for passthrough terminal preparation**

Cover:
- passthrough provider returns attach info without creating durable remote state
- returned command is target-host relative, not synthesized from local assumptions

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-core --locked passthrough_prepare -- --nocapture`
Expected: FAIL because passthrough preparation is not implemented.

- [ ] **Step 3: Implement minimal passthrough preparation**

Add provider logic that:
- accepts pane/checkout context
- returns attach information suitable for later local workspace execution

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-core --locked passthrough_prepare -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/providers/terminal/passthrough.rs
git commit -m "feat: prepare passthrough remote terminals"
```

### Task 6: Add failing core/daemon tests for remote terminal preparation routing

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`
- Modify: `crates/flotilla-daemon/src/server.rs`
- Test: `crates/flotilla-daemon/src/server.rs`

- [ ] **Step 1: Write failing tests for remote terminal prepare command execution**

Cover:
- terminal-prepare command routes to remote host
- remote completion returns attach info to requester
- command lifecycle events still flow correctly

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-daemon --locked terminal_prepare -- --nocapture`
Expected: FAIL because end-to-end remote prepare is not wired.

- [ ] **Step 3: Implement minimal remote prepare execution path**

Update executor/server routing so the new terminal-preparation command:
- executes on the remote daemon
- returns attach payloads through normal remote command completion

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-daemon --locked terminal_prepare -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/executor.rs crates/flotilla-daemon/src/server.rs
git commit -m "feat: route remote terminal preparation"
```

## Chunk 3: Local Workspace Creation Consumes Remote Attach Commands

### Task 7: Add failing workspace-manager integration tests

**Files:**
- Modify: `crates/flotilla-core/src/executor.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/cmux.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/zellij.rs`
- Modify: `crates/flotilla-core/src/providers/workspace/tmux.rs`
- Test: relevant workspace provider tests

- [ ] **Step 1: Write failing tests for create-workspace orchestration with remote pane preparation**

Cover:
- local workspace creation requests remote terminal preparation per pane
- returned attach commands are passed into the local workspace manager
- no remote workspace ownership is introduced

- [ ] **Step 2: Run targeted tests to verify failure**

Run: `cargo test -p flotilla-core --locked create_workspace remote_attach -- --nocapture`
Expected: FAIL because `CreateWorkspaceForCheckout` does not yet resolve remote pane commands.

- [ ] **Step 3: Implement minimal orchestration changes**

Update workspace creation flow so:
- it remains local presentation logic
- it resolves remote pane commands before invoking workspace providers
- it preserves existing local-only behavior for purely local workspaces

- [ ] **Step 4: Run targeted tests to verify pass**

Run: `cargo test -p flotilla-core --locked create_workspace remote_attach -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/executor.rs crates/flotilla-core/src/providers/workspace
git commit -m "feat: create local workspaces from remote attach commands"
```

### Task 8: Add end-to-end integration coverage and finalize docs

**Files:**
- Modify: `crates/flotilla-daemon/tests/multi_host.rs`
- Modify: `AGENTS.md` if command guidance changes
- Modify: `CLAUDE.md` if developer guidance changes
- Modify: `docs/superpowers/plans/2026-03-14-remote-checkouts-and-terminal-prep.md`

- [ ] **Step 1: Add final integration tests**

Add coverage for:
- remote checkout creation from selected target host
- remote passthrough terminal preparation feeding local workspace creation

- [ ] **Step 2: Run the focused integration suite**

Run: `cargo test -p flotilla-daemon --locked --test multi_host -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Run branch-level verification**

Run:
- `cargo +nightly fmt --check`
- `cargo clippy --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`

Expected: all PASS.

- [ ] **Step 4: Mark the plan complete**

Update this plan file to reflect completed work and any scope cuts (for example, if shpool remains deferred by design).

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-daemon/tests/multi_host.rs AGENTS.md CLAUDE.md docs/superpowers/plans/2026-03-14-remote-checkouts-and-terminal-prep.md
git commit -m "chore: finalize remote checkout and terminal prep batch"
```
