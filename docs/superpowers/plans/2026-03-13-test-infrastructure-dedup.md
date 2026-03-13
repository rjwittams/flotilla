# Test Infrastructure Deduplication Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract three duplicated test infrastructure patterns (A7, A8, A9) into shared modules.

**Architecture:** Add shared test helpers to the existing `#[cfg(test)] pub(crate) mod testing` in `providers/mod.rs` and consolidate VCS `git()` calls to the existing `checkout_test_support`. Each task is independent.

**Tech Stack:** Rust, `#[cfg(test)]` modules, `env!("CARGO_MANIFEST_DIR")`

**Spec:** `docs/superpowers/specs/2026-03-13-test-infrastructure-dedup-design.md`

---

## Task 1: VCS `git()` helper consolidation (A9)

Delete the local `git()` functions from `vcs/git.rs` and `vcs/wt.rs`. Both are identical to `checkout_test_support::git()` in `vcs/mod.rs` (which already exists and is used by `git_worktree.rs`). The shared version uses `.expect()` instead of `.unwrap()`, which is the project convention.

**Files:**
- Modify: `crates/flotilla-core/src/providers/vcs/git.rs:126-129` — remove local `git()`, add import
- Modify: `crates/flotilla-core/src/providers/vcs/wt.rs:170-173` — remove local `git()`, add import

- [ ] **Step 1: Update `vcs/git.rs` — remove local `git()` and use shared version**

Delete the local `git()` function (lines 126-129):

```rust
    // DELETE THIS:
    /// Run a git command in `repo`, panicking on failure.
    fn git(repo: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").args(args).current_dir(repo).stdin(std::process::Stdio::null()).output().unwrap();
        assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
    }
```

Add an import of the shared helper. Near the top of the `#[cfg(test)] mod tests` block (after `use super::*;`), add:

```rust
    use super::checkout_test_support::git;
```

All existing call sites use `git(&repo, &[...])` which matches the shared signature `git(cwd: &Path, args: &[&str])` — no call-site changes needed.

- [ ] **Step 2: Update `vcs/wt.rs` — remove local `git()` and use shared version**

Delete the local `git()` function (lines 170-173):

```rust
    // DELETE THIS:
    /// Run a git command in `repo`, panicking on failure.
    fn git(repo: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").args(args).current_dir(repo).stdin(std::process::Stdio::null()).output().unwrap();
        assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
    }
```

Add an import. Near the top of the `#[cfg(test)] mod tests` block (after the existing imports), add:

```rust
    use super::checkout_test_support::git;
```

Note: `wt.rs` tests also call `git(&base, ...)` and `git(&feature_path, ...)` — all with `&Path` first arg, so they match.

- [ ] **Step 3: Verify**

Run: `cargo test -p flotilla-core --lib -- providers::vcs`

Expected: All VCS tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/providers/vcs/git.rs crates/flotilla-core/src/providers/vcs/wt.rs
git commit -m "refactor: consolidate VCS git() test helper to checkout_test_support (A9)"
```

---

## Task 2: GitHub test helpers (A8)

Create a `github_test_support` module in `providers/mod.rs` containing `repo_root_for_recording()` and `build_api_and_runner()`. These are character-identical between `code_review/github.rs` and `issue_tracker/github.rs`.

**Files:**
- Modify: `crates/flotilla-core/src/providers/mod.rs:391` — add `github_test_support` module after `testing`
- Modify: `crates/flotilla-core/src/providers/code_review/github.rs:185-193` — remove local helpers, import shared
- Modify: `crates/flotilla-core/src/providers/issue_tracker/github.rs:190-198` — remove local helpers, import shared

- [ ] **Step 1: Add `github_test_support` module to `providers/mod.rs`**

Insert after the closing brace of the `testing` module (after line 391) and before the `#[cfg(test)] mod tests` block:

```rust
#[cfg(test)]
pub(crate) mod github_test_support {
    use std::{path::PathBuf, sync::Arc};

    use crate::providers::{github_api::GhApi, replay, CommandRunner};

    pub fn repo_root_for_recording() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
    }

    pub fn build_api_and_runner(session: &replay::Session) -> (Arc<dyn GhApi>, Arc<dyn CommandRunner>) {
        let runner = replay::test_runner(session);
        let api = replay::test_gh_api(session);
        (api, runner)
    }
}
```

- [ ] **Step 2: Update `code_review/github.rs` — remove local helpers, import shared**

Delete `repo_root_for_recording` and `build_api_and_runner` (lines 185-193):

```rust
    // DELETE THESE:
    fn repo_root_for_recording() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
    }

    fn build_api_and_runner(session: &replay::Session) -> (Arc<dyn GhApi>, Arc<dyn CommandRunner>) {
        let runner = replay::test_runner(session);
        let api = replay::test_gh_api(session);
        (api, runner)
    }
```

Add import near the top of the test module:

```rust
    use crate::providers::github_test_support::{build_api_and_runner, repo_root_for_recording};
```

After removing the two helper functions, `Arc`, `GhApi`, and `CommandRunner` are no longer referenced directly in this test module (the test bodies only destructure the return of `build_api_and_runner` via type inference). Remove them from the imports:

```rust
    // BEFORE:
    use std::{path::PathBuf, sync::Arc};
    // AFTER:
    use std::path::PathBuf;

    // BEFORE (in the crate::providers block):
    //     github_api::GhApi,
    //     CommandRunner,
    // AFTER: remove both lines
```

Keep `code_review::CodeReview`, `replay::{Masks, {self}}` — those are still used by test bodies.

- [ ] **Step 3: Update `issue_tracker/github.rs` — remove local helpers, import shared**

Delete `repo_root_for_recording` and `build_api_and_runner` (lines 190-198). Add import:

```rust
    use crate::providers::github_test_support::{build_api_and_runner, repo_root_for_recording};
```

**Import cleanup differs from `code_review/github.rs`:** This file has a `MockGhApi` struct and `mock_tracker` function that actively use `Arc`, `GhApi`, `GhApiResponse`, and `ChannelLabel`. Only `CommandRunner` becomes unused after removing `build_api_and_runner` — remove it from the imports. Keep everything else.

- [ ] **Step 4: Verify**

Run: `cargo test -p flotilla-core --lib -- providers::code_review && cargo test -p flotilla-core --lib -- providers::issue_tracker`

Expected: All GitHub provider tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-core/src/providers/mod.rs crates/flotilla-core/src/providers/code_review/github.rs crates/flotilla-core/src/providers/issue_tracker/github.rs
git commit -m "refactor: extract shared GitHub test helpers to github_test_support (A8)"
```

---

## Task 3: Provider fixture path helper (A7)

Add `fixture_path` to the existing `testing` module, then update all 9 `fixture()` functions to delegate.

**Files:**
- Modify: `crates/flotilla-core/src/providers/mod.rs:354-391` — add `fixture_path` to `testing` module
- Modify (9 files — replace `fixture()` body with delegation):
  - `crates/flotilla-core/src/providers/vcs/git.rs:204-206`
  - `crates/flotilla-core/src/providers/vcs/wt.rs:251-253`
  - `crates/flotilla-core/src/providers/vcs/git_worktree.rs:356-358`
  - `crates/flotilla-core/src/providers/code_review/github.rs:181-183`
  - `crates/flotilla-core/src/providers/issue_tracker/github.rs:186-188`
  - `crates/flotilla-core/src/providers/coding_agent/claude.rs:360-362`
  - `crates/flotilla-core/src/providers/coding_agent/codex.rs:514-516`
  - `crates/flotilla-core/src/providers/workspace/tmux.rs:370-372`
  - `crates/flotilla-core/src/providers/workspace/zellij.rs:385-387`

- [ ] **Step 1: Add `fixture_path` to `testing` module in `providers/mod.rs`**

Inside the existing `#[cfg(test)] pub mod testing` block (at the end, before the closing brace), add:

```rust
    /// Build the path to a provider fixture file.
    ///
    /// `provider_dir` is the subdirectory under `src/providers/` (e.g. `"vcs"`, `"code_review"`).
    pub fn fixture_path(provider_dir: &str, name: &str) -> String {
        format!("{}/src/providers/{}/fixtures/{}", env!("CARGO_MANIFEST_DIR"), provider_dir, name)
    }
```

- [ ] **Step 2: Update all 9 `fixture()` functions to delegate**

In each test module, replace the `fixture()` body with a call to the shared function. Keep the local wrapper for ergonomics.

**`vcs/git.rs`** (line 204-206) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("vcs", name)
    }
```

**`vcs/wt.rs`** (line 251-253) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("vcs", name)
    }
```

**`vcs/git_worktree.rs`** (line 356-358) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("vcs", name)
    }
```

**`code_review/github.rs`** (line 181-183) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("code_review", name)
    }
```

**`issue_tracker/github.rs`** (line 186-188) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("issue_tracker", name)
    }
```

**`coding_agent/claude.rs`** (line 360-362) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("coding_agent", name)
    }
```

**`coding_agent/codex.rs`** (line 514-516) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("coding_agent", name)
    }
```

**`workspace/tmux.rs`** (line 370-372) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("workspace", name)
    }
```

**`workspace/zellij.rs`** (line 385-387) — change to:
```rust
    fn fixture(name: &str) -> String {
        crate::providers::testing::fixture_path("workspace", name)
    }
```

- [ ] **Step 3: Verify**

Run: `cargo test -p flotilla-core --locked`

Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/providers/mod.rs \
  crates/flotilla-core/src/providers/vcs/git.rs \
  crates/flotilla-core/src/providers/vcs/wt.rs \
  crates/flotilla-core/src/providers/vcs/git_worktree.rs \
  crates/flotilla-core/src/providers/code_review/github.rs \
  crates/flotilla-core/src/providers/issue_tracker/github.rs \
  crates/flotilla-core/src/providers/coding_agent/claude.rs \
  crates/flotilla-core/src/providers/coding_agent/codex.rs \
  crates/flotilla-core/src/providers/workspace/tmux.rs \
  crates/flotilla-core/src/providers/workspace/zellij.rs
git commit -m "refactor: extract shared fixture_path helper for provider tests (A7)"
```

---

## Final Verification

- [ ] **Full test suite:** `cargo test --locked`
- [ ] **Lint:** `cargo clippy --all-targets --locked -- -D warnings`
- [ ] **Format:** `cargo +nightly fmt`
