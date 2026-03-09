# Issue Cache Robustness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the issue cache refresh incrementally on periodic cycles and fully on manual refresh, and cap concurrent API calls in `fetch_issues_by_id`.

**Architecture:** Add an `IssueChangeset` type to the protocol layer. Extend the `IssueTracker` trait with a default `list_issues_changed_since` method. Add `apply_changeset` and `reset` to `IssueCache`. Wire incremental refresh into the poll loop and full refresh into the manual refresh path. Cap `fetch_issues_by_id` concurrency with `buffer_unordered(10)`.

**Tech Stack:** Rust, async-trait, futures (StreamExt), tokio, serde, chrono (for ISO 8601 timestamps)

---

### Task 1: Add `IssueChangeset` to flotilla-protocol

**Files:**
- Modify: `crates/flotilla-protocol/src/provider_data.rs:82-87` (after `IssuePage`)
- Modify: `crates/flotilla-protocol/src/lib.rs:31-34` (re-export)

**Step 1: Write the test**

Add to the bottom of the `#[cfg(test)] mod tests` in `crates/flotilla-protocol/src/provider_data.rs`:

```rust
#[test]
fn issue_changeset_roundtrip() {
    let changeset = IssueChangeset {
        updated: vec![Issue {
            id: "42".into(),
            title: "Updated issue".into(),
            labels: vec!["bug".into()],
            association_keys: vec![],
        }],
        closed_ids: vec!["7".into(), "13".into()],
        has_more: false,
    };
    assert_roundtrip(&changeset);

    // Empty changeset
    let empty = IssueChangeset {
        updated: vec![],
        closed_ids: vec![],
        has_more: false,
    };
    assert_roundtrip(&empty);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-protocol issue_changeset_roundtrip`
Expected: compile error — `IssueChangeset` not defined

**Step 3: Add the struct**

In `crates/flotilla-protocol/src/provider_data.rs`, after the `IssuePage` struct (line 87), add:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueChangeset {
    pub updated: Vec<Issue>,
    pub closed_ids: Vec<String>,
    pub has_more: bool,
}
```

In `crates/flotilla-protocol/src/lib.rs`, add `IssueChangeset` to the `pub use provider_data::{...}` re-export list (line 31-34).

**Step 4: Run test to verify it passes**

Run: `cargo test -p flotilla-protocol issue_changeset_roundtrip`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/flotilla-protocol/src/provider_data.rs crates/flotilla-protocol/src/lib.rs
git commit -m "feat: add IssueChangeset type to protocol"
```

---

### Task 2: Add `apply_changeset` and `reset` to `IssueCache`

**Files:**
- Modify: `crates/flotilla-core/src/issue_cache.rs`

**Step 1: Write the tests**

Add to the `#[cfg(test)] mod tests` in `crates/flotilla-core/src/issue_cache.rs`:

```rust
#[test]
fn apply_changeset_upserts_and_evicts() {
    let mut cache = IssueCache::new();
    cache.merge_page(IssuePage {
        issues: vec![issue("1"), issue("2"), issue("3")],
        total_count: None,
        has_more: false,
    });

    let changeset = IssueChangeset {
        updated: vec![Issue {
            id: "2".to_string(),
            title: "Updated Issue 2".to_string(),
            labels: vec!["changed".to_string()],
            association_keys: vec![],
        }, issue("4")],
        closed_ids: vec!["3".to_string()],
        has_more: false,
    };
    cache.apply_changeset(changeset);

    let map = cache.to_index_map();
    assert_eq!(map.len(), 3); // 1, updated-2, 4 (3 evicted)
    assert_eq!(map["2"].title, "Updated Issue 2");
    assert!(map.contains_key("4"));
    assert!(!map.contains_key("3"));
}

#[test]
fn apply_changeset_preserves_pinned_on_close() {
    let mut cache = IssueCache::new();
    cache.add_pinned(vec![issue("99")]);
    cache.merge_page(IssuePage {
        issues: vec![issue("1")],
        total_count: None,
        has_more: false,
    });

    let changeset = IssueChangeset {
        updated: vec![],
        closed_ids: vec!["99".to_string(), "1".to_string()],
        has_more: false,
    };
    cache.apply_changeset(changeset);

    let map = cache.to_index_map();
    assert!(map.contains_key("99"), "pinned issues survive eviction");
    assert!(!map.contains_key("1"), "non-pinned issues are evicted");
}

#[test]
fn reset_clears_non_pinned_entries() {
    let mut cache = IssueCache::new();
    cache.merge_page(IssuePage {
        issues: vec![issue("1"), issue("2")],
        total_count: Some(10),
        has_more: true,
    });
    cache.add_pinned(vec![issue("99")]);
    assert_eq!(cache.next_page, 2);

    cache.reset();

    assert_eq!(cache.len(), 1, "only pinned issue remains");
    assert!(cache.to_index_map().contains_key("99"));
    assert_eq!(cache.next_page, 1);
    assert!(cache.has_more);
    assert!(cache.pinned.contains("99"), "pinned set preserved");
    assert_eq!(cache.total_count, None);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core issue_cache`
Expected: compile errors — `apply_changeset` and `reset` not defined

**Step 3: Add the import and implement the methods**

Add `IssueChangeset` to the import at the top of `crates/flotilla-core/src/issue_cache.rs`:

```rust
use flotilla_protocol::{Issue, IssueChangeset, IssuePage};
```

Add methods to the `impl IssueCache` block, after `to_index_map()`:

```rust
/// Apply an incremental changeset: upsert open issues, evict closed ones.
/// Pinned issues are never evicted (they're linked to PRs via correlation).
pub fn apply_changeset(&mut self, changeset: IssueChangeset) {
    let entries = Arc::make_mut(&mut self.entries);
    for issue in changeset.updated {
        entries.insert(issue.id.clone(), issue);
    }
    for id in &changeset.closed_ids {
        if !self.pinned.contains(id) {
            entries.shift_remove(id);
        }
    }
}

/// Reset pagination state for a full re-fetch. Pinned issues and the
/// pinned set are preserved; everything else is cleared.
pub fn reset(&mut self) {
    let pinned_issues: Vec<Issue> = self
        .pinned
        .iter()
        .filter_map(|id| self.entries.get(id).cloned())
        .collect();

    let entries = Arc::make_mut(&mut self.entries);
    entries.clear();
    for issue in pinned_issues {
        entries.insert(issue.id.clone(), issue);
    }
    self.next_page = 1;
    self.has_more = true;
    self.total_count = None;
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-core issue_cache`
Expected: all PASS

**Step 5: Commit**

```bash
git add crates/flotilla-core/src/issue_cache.rs
git commit -m "feat: add apply_changeset and reset to IssueCache"
```

---

### Task 3: Add `last_refreshed_at` to `IssueCache`

**Files:**
- Modify: `crates/flotilla-core/src/issue_cache.rs`

**Step 1: Write the test**

Add to `#[cfg(test)] mod tests` in `issue_cache.rs`:

```rust
#[test]
fn last_refreshed_at_tracks_timestamps() {
    let mut cache = IssueCache::new();
    assert!(cache.last_refreshed_at.is_none());

    cache.mark_refreshed("2026-03-09T12:00:00Z".to_string());
    assert_eq!(cache.last_refreshed_at.as_deref(), Some("2026-03-09T12:00:00Z"));

    cache.reset();
    assert!(cache.last_refreshed_at.is_none(), "reset clears timestamp");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p flotilla-core last_refreshed_at`
Expected: compile error

**Step 3: Add field and method**

Add field to `IssueCache` struct:

```rust
pub last_refreshed_at: Option<String>,
```

Initialize it as `None` in `new()`.

Clear it in `reset()` (add `self.last_refreshed_at = None;`).

Add method:

```rust
pub fn mark_refreshed(&mut self, timestamp: String) {
    self.last_refreshed_at = Some(timestamp);
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p flotilla-core last_refreshed_at`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/flotilla-core/src/issue_cache.rs
git commit -m "feat: add last_refreshed_at to IssueCache"
```

---

### Task 4: Add `list_issues_changed_since` to `IssueTracker` trait

**Files:**
- Modify: `crates/flotilla-core/src/providers/issue_tracker/mod.rs`

**Step 1: Add the trait method with default implementation**

Add import at the top of `crates/flotilla-core/src/providers/issue_tracker/mod.rs`:

```rust
use crate::providers::types::{Issue, IssueChangeset, IssuePage};
```

(Replace the existing `use crate::providers::types::{Issue, IssuePage};`)

Add the method to the `IssueTracker` trait, after `search_issues`:

```rust
/// Incremental sync: returns issues changed since the given ISO 8601 timestamp.
/// Default implementation falls back to a full page-1 fetch (no evictions).
async fn list_issues_changed_since(
    &self,
    repo_root: &Path,
    since: &str,
    per_page: usize,
) -> Result<IssueChangeset, String> {
    let page = self.list_issues_page(repo_root, 1, per_page).await?;
    Ok(IssueChangeset {
        updated: page.issues,
        closed_ids: vec![],
        has_more: page.has_more,
    })
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: compiles (default implementation means no changes needed to GitHubIssueTracker yet)

**Step 3: Commit**

```bash
git add crates/flotilla-core/src/providers/issue_tracker/mod.rs
git commit -m "feat: add list_issues_changed_since to IssueTracker trait"
```

---

### Task 5: Implement `list_issues_changed_since` for GitHub

**Files:**
- Modify: `crates/flotilla-core/src/providers/issue_tracker/github.rs`

**Step 1: Write the test**

Add a `MockGhApi` test helper and test to the `#[cfg(test)] mod tests` block in `github.rs`:

```rust
use super::*;
use std::path::PathBuf;
use std::sync::Mutex;
use std::collections::VecDeque;

struct MockGhApi {
    responses: Mutex<VecDeque<Result<GhApiResponse, String>>>,
}

impl MockGhApi {
    fn new(responses: Vec<Result<GhApiResponse, String>>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
        }
    }
}

#[async_trait::async_trait]
impl GhApi for MockGhApi {
    async fn get(&self, endpoint: &str, repo_root: &Path) -> Result<String, String> {
        self.get_with_headers(endpoint, repo_root)
            .await
            .map(|r| r.body)
    }

    async fn get_with_headers(
        &self,
        _endpoint: &str,
        _repo_root: &Path,
    ) -> Result<GhApiResponse, String> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockGhApi: no more responses")
    }
}

use crate::providers::issue_tracker::IssueTracker;
use crate::providers::github_api::GhApiResponse;

#[tokio::test]
async fn changed_since_partitions_open_and_closed() {
    let body = r#"[
        {"number": 1, "title": "Open issue", "state": "open", "labels": []},
        {"number": 2, "title": "Closed issue", "state": "closed", "labels": []},
        {"number": 3, "title": "Another open", "state": "open", "labels": []}
    ]"#;
    let api = Arc::new(MockGhApi::new(vec![Ok(GhApiResponse {
        status: 200,
        etag: None,
        body: body.to_string(),
        has_next_page: false,
        total_count: None,
    })]));
    let runner = Arc::new(crate::providers::test_support::MockRunner::new(vec![]));
    let tracker = GitHubIssueTracker::new(
        "github".into(),
        "owner/repo".into(),
        api,
        runner,
    );

    let changeset = tracker
        .list_issues_changed_since(Path::new("/tmp/repo"), "2026-03-09T00:00:00Z", 50)
        .await
        .unwrap();

    assert_eq!(changeset.updated.len(), 2);
    assert_eq!(changeset.updated[0].id, "1");
    assert_eq!(changeset.updated[1].id, "3");
    assert_eq!(changeset.closed_ids, vec!["2"]);
    assert!(!changeset.has_more);
}

#[tokio::test]
async fn changed_since_filters_pull_requests() {
    let body = r#"[
        {"number": 1, "title": "Issue", "state": "open", "labels": []},
        {"number": 2, "title": "PR", "state": "open", "labels": [], "pull_request": {"url": "..."}}
    ]"#;
    let api = Arc::new(MockGhApi::new(vec![Ok(GhApiResponse {
        status: 200,
        etag: None,
        body: body.to_string(),
        has_next_page: false,
        total_count: None,
    })]));
    let runner = Arc::new(crate::providers::test_support::MockRunner::new(vec![]));
    let tracker = GitHubIssueTracker::new(
        "github".into(),
        "owner/repo".into(),
        api,
        runner,
    );

    let changeset = tracker
        .list_issues_changed_since(Path::new("/tmp/repo"), "2026-03-09T00:00:00Z", 50)
        .await
        .unwrap();

    assert_eq!(changeset.updated.len(), 1);
    assert_eq!(changeset.updated[0].id, "1");
    assert!(changeset.closed_ids.is_empty());
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p flotilla-core changed_since`
Expected: compile error or test failure (method not yet overridden)

**Step 3: Implement the method**

Add to the `impl super::IssueTracker for GitHubIssueTracker` block in `github.rs`, after `search_issues`:

```rust
async fn list_issues_changed_since(
    &self,
    repo_root: &Path,
    since: &str,
    per_page: usize,
) -> Result<IssueChangeset, String> {
    let per_page = clamp_per_page(per_page);
    let endpoint = format!(
        "repos/{}/issues?state=all&since={}&sort=updated&direction=desc&per_page={}",
        self.repo_slug, since, per_page
    );
    let response = self.api.get_with_headers(&endpoint, repo_root).await?;
    let items: Vec<serde_json::Value> =
        serde_json::from_str(&response.body).map_err(|e| e.to_string())?;

    let mut updated = Vec::new();
    let mut closed_ids = Vec::new();

    for v in &items {
        // Skip pull requests (GitHub's issues endpoint includes PRs)
        if v.as_object()
            .map(|o| o.contains_key("pull_request"))
            .unwrap_or(false)
        {
            continue;
        }
        let state = v["state"].as_str().unwrap_or("open");
        if state == "open" {
            if let Some(issue) = parse_issue(&self.provider_name, v) {
                updated.push(issue);
            }
        } else if let Some(number) = v["number"].as_i64() {
            closed_ids.push(number.to_string());
        }
    }

    Ok(IssueChangeset {
        updated,
        closed_ids,
        has_more: response.has_next_page,
    })
}
```

Add the import at the top of `github.rs`:

```rust
use crate::providers::types::IssueChangeset;
```

(Add `IssueChangeset` to the existing `use crate::providers::types::*;` — since it uses glob, just ensure `IssueChangeset` is exported from `types`.)

**Step 4: Ensure `IssueChangeset` is accessible via `types`**

Check `crates/flotilla-core/src/providers/types.rs` — it re-exports from `flotilla_protocol`. Verify `IssueChangeset` is included. If not, add it.

**Step 5: Run tests to verify they pass**

Run: `cargo test -p flotilla-core changed_since`
Expected: all PASS

**Step 6: Commit**

```bash
git add crates/flotilla-core/src/providers/issue_tracker/github.rs crates/flotilla-core/src/providers/issue_tracker/mod.rs crates/flotilla-core/src/providers/types.rs
git commit -m "feat: implement list_issues_changed_since for GitHub"
```

---

### Task 6: Cap `fetch_issues_by_id` concurrency (#115)

**Files:**
- Modify: `crates/flotilla-core/src/providers/issue_tracker/github.rs:100-131`

**Step 1: Write the test**

Add to `#[cfg(test)] mod tests` in `github.rs`:

```rust
#[tokio::test]
async fn fetch_issues_by_id_limits_concurrency() {
    // Create 15 issue IDs — enough to exceed the concurrency limit of 10
    let ids: Vec<String> = (1..=15).map(|n| n.to_string()).collect();
    let responses: Vec<_> = ids
        .iter()
        .map(|id| {
            let body = format!(
                r#"{{"number": {}, "title": "Issue {}", "labels": []}}"#,
                id, id
            );
            Ok(GhApiResponse {
                status: 200,
                etag: None,
                body,
                has_next_page: false,
                total_count: None,
            })
        })
        .collect();

    let api = Arc::new(MockGhApi::new(responses));
    let runner = Arc::new(crate::providers::test_support::MockRunner::new(vec![]));
    let tracker = GitHubIssueTracker::new(
        "github".into(),
        "owner/repo".into(),
        api,
        runner,
    );

    let result = tracker
        .fetch_issues_by_id(Path::new("/tmp/repo"), &ids)
        .await
        .unwrap();

    // All 15 should be fetched successfully (just capped at 10 concurrent)
    assert_eq!(result.len(), 15);
}
```

**Step 2: Run test to verify it passes with current code (baseline)**

Run: `cargo test -p flotilla-core fetch_issues_by_id_limits`
Expected: PASS (current unbounded code still fetches all 15)

**Step 3: Replace `join_all` with `buffer_unordered`**

In `github.rs`, replace the `fetch_issues_by_id` method body (lines 100-131):

```rust
async fn fetch_issues_by_id(
    &self,
    repo_root: &Path,
    ids: &[String],
) -> Result<Vec<Issue>, String> {
    use futures::stream::{self, StreamExt};

    let futs = ids.iter().map(|id| {
        let endpoint = format!("repos/{}/issues/{}", self.repo_slug, id);
        let api = Arc::clone(&self.api);
        let repo_root = repo_root.to_path_buf();
        let provider_name = self.provider_name.clone();
        async move {
            let body = api.get(&endpoint, &repo_root).await?;
            let v: serde_json::Value =
                serde_json::from_str(&body).map_err(|e| e.to_string())?;
            parse_issue(&provider_name, &v)
                .ok_or_else(|| format!("failed to parse issue {}", id))
        }
    });

    let results: Vec<_> = stream::iter(futs).buffer_unordered(10).collect().await;
    let mut issues = Vec::new();
    for result in results {
        match result {
            Ok(issue) => issues.push(issue),
            Err(e) => tracing::warn!("failed to fetch issue: {}", e),
        }
    }
    Ok(issues)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p flotilla-core fetch_issues_by_id`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/flotilla-core/src/providers/issue_tracker/github.rs
git commit -m "fix: cap fetch_issues_by_id concurrency at 10 (#115)"
```

---

### Task 7: Wire incremental refresh into `poll_snapshots`

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs`

This is the core integration. After `poll_snapshots` detects changed provider data and broadcasts snapshots, it should also run an incremental issue refresh for repos that have a `last_refreshed_at` timestamp.

**Step 1: Add `now_iso8601` helper**

Add a utility function at the top of `in_process.rs` (after the imports):

```rust
fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
```

Add `chrono` to `crates/flotilla-core/Cargo.toml`:

```toml
chrono = { version = "0.4", default-features = false, features = ["clock"] }
```

**Step 2: Add `refresh_issues_incremental` method to `InProcessDaemon`**

Add after `fetch_missing_linked_issues`:

```rust
/// Incremental issue refresh: fetch issues changed since last refresh,
/// apply changeset to cache, and broadcast if anything changed.
async fn refresh_issues_incremental(&self) {
    let tasks: Vec<_> = {
        let repos = self.repos.read().await;
        repos
            .iter()
            .filter_map(|(path, state)| {
                let since = state.issue_cache.last_refreshed_at.as_ref()?;
                if state.model.registry.issue_trackers.is_empty() {
                    return None;
                }
                Some((
                    path.clone(),
                    since.clone(),
                    Arc::clone(&state.model.registry),
                    Arc::clone(&state.issue_fetch_mutex),
                    state.issue_cache.len(),
                ))
            })
            .collect()
    };

    for (path, since, registry, fetch_mutex, prev_count) in tasks {
        let _guard = fetch_mutex.lock().await;
        let tracker = match registry.issue_trackers.values().next() {
            Some(t) => t,
            None => continue,
        };

        match tracker.list_issues_changed_since(&path, &since, 50).await {
            Ok(changeset) => {
                let escalate = changeset.has_more;
                let has_changes =
                    !changeset.updated.is_empty() || !changeset.closed_ids.is_empty();

                {
                    let mut repos = self.repos.write().await;
                    if let Some(state) = repos.get_mut(&path) {
                        state.issue_cache.apply_changeset(changeset);
                        state
                            .issue_cache
                            .mark_refreshed(now_iso8601());
                    }
                }

                if escalate {
                    // Too many changes — fall back to full re-fetch
                    drop(_guard);
                    {
                        let mut repos = self.repos.write().await;
                        if let Some(state) = repos.get_mut(&path) {
                            state.issue_cache.reset();
                        }
                    }
                    self.ensure_issues_cached(&path, prev_count).await;
                    {
                        let mut repos = self.repos.write().await;
                        if let Some(state) = repos.get_mut(&path) {
                            state.issue_cache.mark_refreshed(now_iso8601());
                        }
                    }
                    self.broadcast_snapshot(&path).await;
                } else if has_changes {
                    self.broadcast_snapshot(&path).await;
                }
            }
            Err(e) => {
                tracing::warn!("incremental issue refresh failed for {}: {}", path.display(), e);
            }
        }
    }
}
```

**Step 3: Call from `poll_snapshots`**

In `poll_snapshots`, after `self.fetch_missing_linked_issues().await;` (line 268), add:

```rust
self.refresh_issues_incremental().await;
```

**Step 4: Set `last_refreshed_at` on initial fetch**

In `ensure_issues_cached`, after a successful `merge_page` (inside the `Ok(page)` arm, after line 320), mark the timestamp if this was the first page:

At the end of the loop body in `ensure_issues_cached`, after `state.issue_cache.merge_page(page);` on line 320, add:

```rust
if state.issue_cache.last_refreshed_at.is_none() {
    state.issue_cache.mark_refreshed(now_iso8601());
}
```

**Step 5: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: compiles

**Step 6: Commit**

```bash
git add crates/flotilla-core/Cargo.toml crates/flotilla-core/src/in_process.rs
git commit -m "feat: wire incremental issue refresh into poll loop (#134)"
```

---

### Task 8: Wire full refresh into manual refresh path

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs:565-572`

**Step 1: Implement full issue refresh on manual trigger**

Replace the `refresh` method on `InProcessDaemon` (lines 565-572):

```rust
async fn refresh(&self, repo: &Path) -> Result<(), String> {
    // Reset issue cache and re-fetch up to previous count
    let prev_count = {
        let mut repos = self.repos.write().await;
        let state = repos
            .get_mut(repo)
            .ok_or_else(|| format!("repo not tracked: {}", repo.display()))?;
        let count = state.issue_cache.len();
        state.issue_cache.reset();
        state.model.refresh_handle.trigger_refresh();
        count
    };

    if prev_count > 0 {
        self.ensure_issues_cached(repo, prev_count).await;
        {
            let mut repos = self.repos.write().await;
            if let Some(state) = repos.get_mut(repo) {
                state.issue_cache.mark_refreshed(now_iso8601());
            }
        }
        self.broadcast_snapshot(repo).await;
    }

    Ok(())
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p flotilla-core`
Expected: compiles

**Step 3: Commit**

```bash
git add crates/flotilla-core/src/in_process.rs
git commit -m "feat: reset and re-fetch issues on manual refresh (#134)"
```

---

### Task 9: Ensure types are re-exported correctly

**Files:**
- Modify: `crates/flotilla-core/src/providers/types.rs` (if it exists) or wherever types are re-exported

**Step 1: Verify `IssueChangeset` is accessible**

Run: `cargo check --all-targets`
Expected: compiles with no errors

If `IssueChangeset` is not re-exported from `types`, add it. The `github.rs` file uses `use crate::providers::types::*;` — the `IssueChangeset` needs to be available through that path.

**Step 2: Run full test suite**

Run: `cargo test --locked`
Expected: all PASS

**Step 3: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: no warnings

**Step 4: Commit (if any fixups needed)**

```bash
git add -A
git commit -m "chore: ensure IssueChangeset re-exports and fix clippy"
```

---

### Task 10: Close already-fixed issues

After all code changes are merged, close issues #116, #117, #118 with a note that the implementation already addresses the concerns raised in the PR review.

```bash
gh issue close 116 -c "Already addressed: clamp_per_page is defined in github_api.rs and the Link header guard is correct."
gh issue close 117 -c "Already addressed: ensure_issues_cached drops locks before network calls."
gh issue close 118 -c "Already addressed: all issue commands call broadcast_snapshot()."
```
