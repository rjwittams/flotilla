# Stateless Issue Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the cursor-based issue query system with stateless paged queries, eliminating server-side cursor lifecycle management and fixing cursor ID collision bugs across multiple tracked repos.

**Architecture:** Each query request carries the full `(repo, params, page, count)` tuple — the server needs no state between requests. The `IssueQueryService` trait drops from 6 methods to 3. All cursor infrastructure (cursor maps, session tracking, expiry, disconnect cleanup) is deleted from core, daemon, client, and TUI.

**Tech Stack:** Rust, async-trait, flotilla-protocol serde types, tokio

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/flotilla-protocol/src/issue_query.rs` | Modify | Remove `CursorId`, add `page` awareness |
| `crates/flotilla-protocol/src/commands.rs` | Modify | Replace 3 cursor commands with 1 `QueryIssues` |
| `crates/flotilla-core/src/providers/issue_query/mod.rs` | Modify | Simplify trait to 3 methods |
| `crates/flotilla-core/src/providers/issue_query/github.rs` | Modify | Remove cursor state, implement stateless `query` |
| `crates/flotilla-core/src/in_process.rs` | Modify | Remove `cursor_repo_map`, simplify `execute_query` |
| `crates/flotilla-core/src/providers/discovery/factories/github_issue_query.rs` | Check | May need signature update |
| `crates/flotilla-daemon/src/server/remote_commands.rs` | Modify | Remove `remote_cursors`, `disconnect_session_cursors` |
| `crates/flotilla-daemon/src/server/client_connection.rs` | Modify | Remove cursor cleanup from `finish_session` |
| `crates/flotilla-daemon/src/server/request_dispatch.rs` | No change | Already generic over query commands |
| `crates/flotilla-daemon/tests/request_session_pair.rs` | Modify | Replace cursor lifecycle test with stateless query test |
| `crates/flotilla-tui/src/app/issue_view.rs` | Modify | Replace `IssueCursorState` with `IssuePagingState` (tracks page number) |
| `crates/flotilla-tui/src/app/mod.rs` | Modify | Remove `spawn_close_cursor`, simplify update handlers |
| `crates/flotilla-tui/src/app/executor.rs` | Modify | Simplify query dispatch |
| `crates/flotilla-tui/src/app/key_handlers.rs` | Modify | Update `check_infinite_scroll` |

---

### Task 1: Protocol types — remove cursors, add stateless command

**Files:**
- Modify: `crates/flotilla-protocol/src/issue_query.rs`
- Modify: `crates/flotilla-protocol/src/commands.rs`

- [ ] **Step 1: Update `issue_query.rs` — remove `CursorId`, keep `IssueQuery` and `IssueResultPage`**

Delete the `CursorId` struct and its tests. `IssueQuery` and `IssueResultPage` stay unchanged.

```rust
// issue_query.rs — full file after edit
use serde::{Deserialize, Serialize};

use crate::provider_data::Issue;

/// Parameters for an issue query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueQuery {
    pub search: Option<String>,
}

/// A single page of query results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueResultPage {
    pub items: Vec<(String, Issue)>,
    pub total: Option<u32>,
    pub has_more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_query_default_has_no_search() {
        let q = IssueQuery::default();
        assert!(q.search.is_none());
    }

    #[test]
    fn issue_result_page_serde_roundtrip() {
        let page = IssueResultPage {
            items: vec![("1".into(), Issue {
                title: "Bug".into(),
                labels: vec![],
                association_keys: vec![],
                provider_name: "github".into(),
                provider_display_name: "GitHub".into(),
            })],
            total: Some(42),
            has_more: true,
        };
        let json = serde_json::to_string(&page).expect("serialize");
        let decoded: IssueResultPage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.items.len(), 1);
        assert_eq!(decoded.total, Some(42));
        assert!(decoded.has_more);
    }
}
```

- [ ] **Step 2: Update `commands.rs` — replace cursor commands with `QueryIssues`**

Replace three `CommandAction` variants:

```rust
// REMOVE these three:
QueryIssueOpen { repo: RepoSelector, params: IssueQuery },
QueryIssueFetchPage { cursor: CursorId, count: usize },
QueryIssueClose { cursor: CursorId },

// REPLACE with one:
QueryIssues { repo: RepoSelector, params: IssueQuery, page: u32, count: usize },
```

Update `is_query()` — replace the three cursor entries with `CommandAction::QueryIssues { .. }`.

Update `description()` — add `CommandAction::QueryIssues { .. } => "query issues"`.

Replace `CommandValue::IssueQueryOpened { cursor: CursorId }` and `CommandValue::IssueQueryClosed` with nothing — `IssuePage(IssueResultPage)` is the only result variant needed. Delete both variants.

Remove the `use crate::issue_query::CursorId` import (keep `IssueQuery`, `IssueResultPage`).

Fix all serde roundtrip tests and `is_query` assertion tests in the same file to use `QueryIssues` instead of the three old variants, and remove test cases for the deleted `CommandValue` variants.

- [ ] **Step 3: Verify protocol crate compiles**

Run: `cargo check -p flotilla-protocol`

This will have downstream errors in other crates — that's expected. The protocol crate itself must compile.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-protocol/
git commit -m "refactor: replace cursor-based issue query commands with stateless QueryIssues"
```

---

### Task 2: Simplify `IssueQueryService` trait and GitHub implementation

**Files:**
- Modify: `crates/flotilla-core/src/providers/issue_query/mod.rs`
- Modify: `crates/flotilla-core/src/providers/issue_query/github.rs`

- [ ] **Step 1: Rewrite the trait in `mod.rs`**

```rust
use std::path::Path;

use async_trait::async_trait;
pub use flotilla_protocol::issue_query::{IssueQuery, IssueResultPage};
use flotilla_protocol::provider_data::Issue;

/// Stateless paged query interface for issue listing and search.
#[async_trait]
pub trait IssueQueryService: Send + Sync {
    /// Fetch a page of issues. `params.search` of `None` lists open issues.
    async fn query(&self, repo: &Path, params: &IssueQuery, page: u32, count: usize) -> Result<IssueResultPage, String>;

    /// Fetch specific issues by ID (for linked/pinned issue resolution).
    async fn fetch_by_ids(&self, repo: &Path, ids: &[String]) -> Result<Vec<(String, Issue)>, String>;

    /// Open an issue in the browser.
    async fn open_in_browser(&self, repo: &Path, id: &str) -> Result<(), String>;
}
```

- [ ] **Step 2: Rewrite `github.rs` — delete all cursor machinery, implement stateless `query`**

The struct drops to three fields:

```rust
pub struct GitHubIssueQueryService {
    repo_slug: String,
    api: Arc<dyn GhApi>,
    runner: Arc<dyn CommandRunner>,
}
```

Delete: `CursorState`, `expire_stale_cursors`, `CURSOR_EXPIRY_SECS`, all `Mutex` fields, `AtomicU64`.

The `query` method takes `(repo, params, page, count)` and does what the old `fetch_page` did for a single page — build the endpoint URL using the `page` and `count` arguments directly, make the API call, parse the response. No lock, no state.

```rust
#[async_trait]
impl IssueQueryService for GitHubIssueQueryService {
    async fn query(&self, repo: &Path, params: &IssueQuery, page: u32, count: usize) -> Result<IssueResultPage, String> {
        let per_page = clamp_per_page(count);
        let (items, has_more, total) = match &params.search {
            None => {
                let endpoint = format!("repos/{}/issues?state=open&per_page={}&page={}", self.repo_slug, per_page, page);
                let response = gh_api_get_with_headers!(self.api, &endpoint, repo)?;
                let raw_items: Vec<serde_json::Value> = serde_json::from_str(&response.body).map_err(|e| e.to_string())?;
                let issues: Vec<(String, Issue)> = raw_items
                    .into_iter()
                    .filter(|v| !v.as_object().map(|o| o.contains_key("pull_request")).unwrap_or(false))
                    .filter_map(|v| parse_issue(PROVIDER_NAME, &v))
                    .collect();
                (issues, response.has_next_page, None)
            }
            Some(search_term) => {
                let raw_query = format!("repo:{} is:issue is:open {}", self.repo_slug, search_term);
                let encoded_query = urlencoding::encode(&raw_query);
                let endpoint = format!("search/issues?q={}&per_page={}&page={}", encoded_query, per_page, page);
                let response = gh_api_get_with_headers!(self.api, &endpoint, repo)?;
                let parsed: serde_json::Value = serde_json::from_str(&response.body).map_err(|e| e.to_string())?;
                let total_count = parsed["total_count"].as_u64().map(|n| n as u32);
                let items_array = parsed["items"].as_array().ok_or("no items array in search response")?;
                let issues: Vec<(String, Issue)> = items_array.iter().filter_map(|v| parse_issue(PROVIDER_NAME, v)).collect();
                (issues, response.has_next_page, total_count)
            }
        };
        Ok(IssueResultPage { items, total, has_more })
    }

    // fetch_by_ids and open_in_browser — unchanged from current code
}
```

- [ ] **Step 3: Rewrite the tests in `github.rs`**

Delete all cursor-lifecycle tests (`open_query_returns_valid_cursor_id`, `close_query_removes_cursor`, `fetch_from_unknown_cursor_returns_error`, `cursor_expiry_removes_stale_cursors`, `disconnect_session_removes_all_session_cursors`, `close_query_removes_from_session_cursors`, `multiple_cursors_are_independent`, `fetch_page_when_exhausted_returns_empty`).

Keep and adapt the data-fetching tests. Each now calls `service.query(path, &params, page, count)` directly:

```rust
fn mock_service(responses: Vec<Result<GhApiResponse, String>>) -> GitHubIssueQueryService {
    let api = Arc::new(MockGhApi::new(responses));
    let runner = Arc::new(MockRunner::new(vec![]));
    GitHubIssueQueryService::new("owner/repo".into(), api, runner)
}

#[tokio::test]
async fn query_returns_issues_from_list_endpoint() {
    let body = make_issues_json(3);
    let service = mock_service(vec![ok_response(&body, false)]);
    let page = service.query(Path::new("/repo"), &IssueQuery::default(), 1, 10).await.unwrap();
    assert_eq!(page.items.len(), 3);
    assert!(!page.has_more);
    assert_eq!(page.items[0].0, "1");
    assert_eq!(page.items[0].1.title, "Issue 1");
}

#[tokio::test]
async fn query_with_search_uses_search_endpoint() {
    let body = make_search_json(2, 5);
    let service = mock_service(vec![ok_response(&body, true)]);
    let page = service.query(Path::new("/repo"), &IssueQuery { search: Some("bug".into()) }, 1, 10).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.has_more);
    assert_eq!(page.total, Some(5));
}

#[tokio::test]
async fn query_filters_pull_requests() {
    let body = r#"[
        {"number": 1, "title": "Real issue", "labels": []},
        {"number": 2, "title": "A PR", "labels": [], "pull_request": {"url": "..."}},
        {"number": 3, "title": "Another issue", "labels": []}
    ]"#;
    let service = mock_service(vec![ok_response(body, false)]);
    let page = service.query(Path::new("/repo"), &IssueQuery::default(), 1, 10).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].0, "1");
    assert_eq!(page.items[1].0, "3");
}

#[tokio::test]
async fn query_pagination_uses_page_param() {
    // Two sequential calls with page 1 and page 2 — each is independent
    let body1 = make_issues_json(2);
    let body2 = make_issues_json(1);
    let service = mock_service(vec![ok_response(&body1, true), ok_response(&body2, false)]);
    let page1 = service.query(Path::new("/repo"), &IssueQuery::default(), 1, 2).await.unwrap();
    assert_eq!(page1.items.len(), 2);
    assert!(page1.has_more);
    let page2 = service.query(Path::new("/repo"), &IssueQuery::default(), 2, 2).await.unwrap();
    assert_eq!(page2.items.len(), 1);
    assert!(!page2.has_more);
}
```

Keep `fetch_by_ids_returns_matching_issues` and `open_in_browser_calls_gh_cli` unchanged.

- [ ] **Step 4: Check the factory compiles**

Read `crates/flotilla-core/src/providers/discovery/factories/github_issue_query.rs` and update if it references removed types. The factory constructs `GitHubIssueQueryService::new(slug, api, runner)` — the constructor signature is unchanged, so this should just work.

Run: `cargo check -p flotilla-core --lib`

- [ ] **Step 5: Run core tests**

Run: `cargo test -p flotilla-core --lib -- issue_query`

- [ ] **Step 6: Commit**

```bash
git add crates/flotilla-core/src/providers/issue_query/
git commit -m "refactor: stateless IssueQueryService trait, remove cursor machinery from GitHub impl"
```

---

### Task 3: Remove cursor state from `InProcessDaemon`

**Files:**
- Modify: `crates/flotilla-core/src/in_process.rs`

- [ ] **Step 1: Delete cursor infrastructure**

Remove from the `InProcessDaemon` struct:
- Field: `cursor_repo_map: RwLock<HashMap<CursorId, (RepoIdentity, uuid::Uuid)>>`
- Method: `get_issue_query_service_for_cursor`
- Method: `cursors_for_session`
- Method: `disconnect_client_session`

Remove the `CursorId` import.

- [ ] **Step 2: Rewrite the `execute_query` match arms**

Replace the three cursor arms:

```rust
// OLD: QueryIssueOpen, QueryIssueFetchPage, QueryIssueClose
// NEW: single arm
CommandAction::QueryIssues { repo, params, page, count } => {
    let repo_path = self.resolve_repo_selector(repo).await?;
    let service = self.get_issue_query_service(&repo_path).await?;
    let page = service.query(&repo_path, params, *page, *count).await?;
    Ok(CommandValue::IssuePage(page))
}
```

The `get_issue_query_service` method (which takes a repo path) stays as-is — it's already stateless.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p flotilla-core`

Expect downstream errors in flotilla-daemon (it calls `disconnect_client_session` and `cursors_for_session`). That's Task 4.

- [ ] **Step 4: Commit**

```bash
git add crates/flotilla-core/src/in_process.rs
git commit -m "refactor: remove cursor_repo_map and session tracking from InProcessDaemon"
```

---

### Task 4: Remove cursor tracking from daemon server

**Files:**
- Modify: `crates/flotilla-daemon/src/server/remote_commands.rs`
- Modify: `crates/flotilla-daemon/src/server/client_connection.rs`
- Modify: `crates/flotilla-daemon/tests/request_session_pair.rs`

- [ ] **Step 1: Clean up `remote_commands.rs`**

Delete:
- `RemoteCursorEntry` struct
- `RemoteCursorMap` type alias
- `remote_cursors` field from `RemoteCommandRouter`
- Cursor tracking block inside `dispatch_query` (the `match value { IssueQueryOpened .. IssueQueryClosed .. }` block after the result, lines ~250-263)
- `disconnect_session_cursors` method entirely

In `dispatch_query`, the method becomes: route to local or remote, return result. No post-processing.

In `RemoteCommandRouter::new`, remove the `remote_cursors` field initialization.

- [ ] **Step 2: Clean up `client_connection.rs`**

In `finish_session`, remove the line:
```rust
self.remote_command_router.disconnect_session_cursors(session_id).await;
```
And remove the line:
```rust
self.daemon.disconnect_client_session(session_id).await;
```

The method becomes: abort event task, decrement client count, log.

- [ ] **Step 3: Rewrite the daemon integration test**

In `request_session_pair.rs`:

Replace `TrackingIssueQueryService` with a simpler mock that implements the new 3-method trait:

```rust
struct MockIssueQueryService;

#[async_trait]
impl IssueQueryService for MockIssueQueryService {
    async fn query(&self, _repo: &Path, _params: &IssueQuery, _page: u32, _count: usize) -> Result<IssueResultPage, String> {
        Ok(IssueResultPage {
            items: vec![("1".into(), Issue {
                title: "Test issue".into(),
                labels: vec![],
                association_keys: vec![],
                provider_name: "github".into(),
                provider_display_name: "GitHub".into(),
            })],
            total: Some(1),
            has_more: false,
        })
    }

    async fn fetch_by_ids(&self, _repo: &Path, _ids: &[String]) -> Result<Vec<(String, Issue)>, String> {
        Ok(vec![])
    }

    async fn open_in_browser(&self, _repo: &Path, _id: &str) -> Result<(), String> {
        Ok(())
    }
}
```

Replace `remote_cursor_cleaned_up_on_client_disconnect` with a test that verifies a stateless remote query works end-to-end:

```rust
#[tokio::test]
async fn remote_issue_query_returns_results() {
    // Set up follower with tracked repo and mock issue query service
    // ...
    let result = topology.client.execute_query(
        Command {
            host: Some(HostName::new("follower")),
            provisioning_target: None,
            context_repo: None,
            action: CommandAction::QueryIssues {
                repo: RepoSelector::Path(follower_repo.clone()),
                params: IssueQuery::default(),
                page: 1,
                count: 10,
            },
        },
        uuid::Uuid::nil(),
    ).await.expect("remote issue query");

    match result {
        CommandValue::IssuePage(page) => {
            assert_eq!(page.items.len(), 1);
            assert_eq!(page.items[0].1.title, "Test issue");
        }
        other => panic!("expected IssuePage, got {other:?}"),
    }
}
```

- [ ] **Step 4: Verify daemon compiles and tests pass**

Run: `cargo test -p flotilla-daemon --locked`

- [ ] **Step 5: Commit**

```bash
git add crates/flotilla-daemon/
git commit -m "refactor: remove cursor lifecycle tracking from daemon server"
```

---

### Task 5: Update TUI — stateless paging state

**Files:**
- Modify: `crates/flotilla-tui/src/app/issue_view.rs`
- Modify: `crates/flotilla-tui/src/app/mod.rs`
- Modify: `crates/flotilla-tui/src/app/executor.rs`
- Modify: `crates/flotilla-tui/src/app/key_handlers.rs`

- [ ] **Step 1: Rewrite `issue_view.rs`**

Replace `IssueCursorState` with paging state that tracks the client-side page number:

```rust
use flotilla_protocol::{
    issue_query::{IssueQuery, IssueResultPage},
    provider_data::Issue,
};

use crate::widgets::section_table::IssueRow;

/// State for a single paginated query — tracks accumulated items and next page.
pub struct IssuePagingState {
    pub params: IssueQuery,
    pub items: Vec<(String, Issue)>,
    pub next_page: u32,
    pub total: Option<u32>,
    pub has_more: bool,
    pub fetch_pending: bool,
}

impl IssuePagingState {
    pub fn new(params: IssueQuery) -> Self {
        Self { params, items: Vec::new(), next_page: 1, total: None, has_more: true, fetch_pending: false }
    }

    pub fn append_page(&mut self, page: IssueResultPage) {
        self.total = page.total;
        self.has_more = page.has_more;
        self.fetch_pending = false;
        self.next_page += 1;
        self.items.extend(page.items);
    }

    pub fn to_issue_rows(&self) -> Vec<IssueRow> {
        self.items.iter().map(|(id, issue)| IssueRow { id: id.clone(), issue: issue.clone() }).collect()
    }
}

/// Per-repo issue view state, managing default and search listings.
#[derive(Default)]
pub struct IssueViewState {
    pub default: Option<IssuePagingState>,
    pub search: Option<IssuePagingState>,
    pub search_query: Option<String>,
}

impl IssueViewState {
    pub fn new() -> Self {
        Self { default: None, search: None, search_query: None }
    }

    pub fn active(&self) -> Option<&IssuePagingState> {
        self.search.as_ref().or(self.default.as_ref())
    }

    pub fn active_mut(&mut self) -> Option<&mut IssuePagingState> {
        if self.search.is_some() { self.search.as_mut() } else { self.default.as_mut() }
    }

    pub fn active_issue_rows(&self) -> Vec<IssueRow> {
        self.active().map(|c| c.to_issue_rows()).unwrap_or_default()
    }
}

/// Background update messages from spawned query tasks.
pub enum IssueQueryUpdate {
    /// A page of results arrived.
    PageFetched { repo: flotilla_protocol::RepoIdentity, params: IssueQuery, page: IssueResultPage },
    /// A query request failed.
    QueryFailed { repo: flotilla_protocol::RepoIdentity, message: String, is_search: bool },
}
```

The `IssueQueryUpdate` variants simplify: no cursor IDs, no separate Open/Close messages. `PageFetched` carries the `params` so the handler can match it to default or search. The separate `DefaultCursorOpened` / `SearchCursorOpened` / `PageFetchFailed` variants are gone — opening and fetching the first page is a single operation now.

Update the tests in this file to use `IssuePagingState` instead of `IssueCursorState`.

- [ ] **Step 2: Rewrite query dispatch in `mod.rs`**

Delete `spawn_close_cursor` function entirely.

Replace `spawn_fetch_page` with `spawn_query_page`:

```rust
fn spawn_query_page(&self, repo: RepoIdentity, params: IssueQuery, page: u32, count: usize) {
    let daemon = self.daemon.clone();
    let tx = self.issue_update_tx.clone();
    let session_id = self.session_id;
    let params_clone = params.clone();
    tokio::spawn(async move {
        let cmd = Command {
            host: None,
            provisioning_target: None,
            context_repo: None,
            action: CommandAction::QueryIssues {
                repo: RepoSelector::Identity(repo.clone()),
                params: params_clone.clone(),
                page,
                count,
            },
        };
        match daemon.execute_query(cmd, session_id).await {
            Ok(CommandValue::IssuePage(page)) => {
                let _ = tx.send(IssueQueryUpdate::PageFetched { repo, params: params_clone, page });
            }
            Ok(other) => {
                let _ = tx.send(IssueQueryUpdate::QueryFailed {
                    repo,
                    message: format!("unexpected query result: {other:?}"),
                    is_search: params_clone.search.is_some(),
                });
            }
            Err(e) => {
                let _ = tx.send(IssueQueryUpdate::QueryFailed {
                    repo,
                    message: e,
                    is_search: params_clone.search.is_some(),
                });
            }
        }
    });
}
```

- [ ] **Step 3: Rewrite `drain_background_updates` in `mod.rs`**

Replace all the cursor-aware handlers with two simple cases:

```rust
pub(crate) fn drain_background_updates(&mut self) {
    use issue_view::{IssuePagingState, IssueQueryUpdate};

    while let Ok(update) = self.issue_update_rx.try_recv() {
        match update {
            IssueQueryUpdate::PageFetched { repo, params, page } => {
                let is_search = params.search.is_some();
                let view = self.issue_views.entry(repo.clone()).or_default();
                // Route to the right paging state based on whether this is a search.
                let target = if is_search {
                    // Initialize search state if this is the first page.
                    if view.search.is_none() {
                        view.search = Some(IssuePagingState::new(params.clone()));
                        view.search_query = params.search.clone();
                    }
                    view.search.as_mut()
                } else {
                    if view.default.is_none() {
                        view.default = Some(IssuePagingState::new(params.clone()));
                    }
                    view.default.as_mut()
                };
                if let Some(state) = target {
                    state.append_page(page);
                }
                self.push_issue_items_to_repo_data(&repo);
            }
            IssueQueryUpdate::QueryFailed { repo, message, is_search } => {
                tracing::warn!(%message, %is_search, "issue query failed");
                self.set_status_message(Some(message));
                if is_search {
                    if let Some(view) = self.issue_views.get_mut(&repo) {
                        view.search = None;
                        view.search_query = None;
                    }
                    self.push_issue_items_to_repo_data(&repo);
                } else {
                    self.issue_views.remove(&repo);
                }
            }
        }
    }
}
```

- [ ] **Step 4: Rewrite `maybe_open_default_issue_cursor` → `maybe_fetch_default_issues`**

Same guard logic (skip if already fetched, skip remote-only repos). Instead of opening a cursor then fetching, just call `spawn_query_page` with page 1:

```rust
fn maybe_fetch_default_issues(&self, repo_identity: &RepoIdentity) {
    if self.issue_views.get(repo_identity).and_then(|v| v.default.as_ref()).is_some() {
        return;
    }
    if self.model.repos.get(repo_identity).is_some_and(|r| r.path.starts_with("<remote>")) {
        return;
    }
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    self.spawn_query_page(repo_identity.clone(), IssueQuery::default(), 1, 50);
}
```

Update the call site in `apply_snapshot` to use the new name.

- [ ] **Step 5: Update `ClearSearchQuery` handler in `mod.rs`**

Remove the `spawn_close_cursor` call — there's no server-side cursor to close. Just clear the local state:

```rust
AppAction::ClearSearchQuery { repo } => {
    if let Some(page) = self.screen.repo_pages.get_mut(&repo) {
        page.active_search_query = None;
    }
    if let Some(view) = self.issue_views.get_mut(&repo) {
        view.search = None;
        view.search_query = None;
    }
    self.push_issue_items_to_repo_data(&repo);
}
```

- [ ] **Step 6: Update `executor.rs`**

The `dispatch` function currently intercepts `QueryIssueOpen` to route through the background channel. With the new design, it should intercept `QueryIssues` instead. But actually, since `spawn_query_page` in `mod.rs` now handles all issue query dispatch directly (it builds the command and spawns the task itself), the `executor.rs` dispatch function no longer needs to intercept issue query commands at all. Remove the `QueryIssueOpen` interception block.

- [ ] **Step 7: Update `check_infinite_scroll` in `key_handlers.rs`**

Replace the cursor-based scroll trigger with the paging state:

```rust
fn check_infinite_scroll(&mut self) {
    if self.model.repo_order.is_empty() {
        return;
    }
    let repo_identity = self.model.repo_order[self.model.active_repo].clone();
    let Some(page) = self.screen.repo_pages.get(&repo_identity) else { return };
    let Some(view) = self.issue_views.get(&repo_identity) else { return };
    let Some(active) = view.active() else { return };
    if !active.has_more || active.fetch_pending {
        return;
    }
    let issue_count = active.items.len();
    if issue_count == 0 {
        return;
    }
    let total_items = page.table.total_item_count();
    let Some(flat_idx) = page.table.selected_flat_index() else { return };
    if flat_idx + 5 >= total_items {
        let params = active.params.clone();
        let next_page = active.next_page;
        if let Some(view) = self.issue_views.get_mut(&repo_identity) {
            if let Some(c) = view.active_mut() {
                c.fetch_pending = true;
            }
        }
        self.spawn_query_page(repo_identity, params, next_page, 50);
    }
}
```

- [ ] **Step 8: Verify TUI compiles**

Run: `cargo check -p flotilla-tui`

- [ ] **Step 9: Commit**

```bash
git add crates/flotilla-tui/
git commit -m "refactor: stateless issue paging in TUI, remove cursor lifecycle management"
```

---

### Task 6: Update discovery test support and remaining references

**Files:**
- Modify: `crates/flotilla-core/src/providers/discovery/test_support.rs`
- Any remaining compile errors

- [ ] **Step 1: Update `FakeDiscoveryProviders` and test support**

The `with_issue_query_service` helper in test support takes an `Arc<dyn IssueQueryService>`. The trait signature changed, so any fake/mock implementations in test support need updating to the new 3-method trait. Search for all implementations of `IssueQueryService` and update them.

Run: `cargo check --workspace --all-targets --locked`

Fix any remaining compile errors — these will be mechanical (removed types, renamed methods).

- [ ] **Step 2: Run the full test suite**

Run: `cargo test --workspace --locked`

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`

- [ ] **Step 4: Run format check**

Run: `cargo +nightly-2026-03-12 fmt --check`

- [ ] **Step 5: Commit any remaining fixes**

```bash
git add -A
git commit -m "chore: fix remaining references to removed cursor types"
```

---

### Task 7: Verify snapshot tests

- [ ] **Step 1: Check for snapshot changes**

Run: `cargo test --workspace --locked 2>&1 | grep -i "snapshot"` or check for any insta snapshot failures.

If any snapshots changed (e.g. command serialization snapshots), investigate why — the `CommandAction` serde format changed, so serialization snapshots for `QueryIssueOpen`/`QueryIssueFetchPage`/`QueryIssueClose` need updating to `QueryIssues`. Accept only after verifying the change is the expected consequence of this refactor.

- [ ] **Step 2: Final full CI check**

Run all three CI gates:
```bash
cargo +nightly-2026-03-12 fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

- [ ] **Step 3: Commit if needed**

```bash
git add -A
git commit -m "chore: update snapshots for stateless issue query commands"
```
