# Issue Fetching: Pagination, Search, and Caching

## Problem

Issues are fetched with a hardcoded limit of 20 per refresh. Repos with hundreds or thousands of open issues show only a small slice. Issues linked from PRs/checkouts may fall outside that window and be missing. There is no search, no pagination, and no way for multiple TUI clients to have independent views into the issue list.

## Design

### IssueCache (daemon layer)

Each repo gets an `IssueCache` owned by the daemon:

```rust
struct IssueCache {
    entries: IndexMap<String, Issue>,
    next_page: Option<u32>,
    has_more: bool,
    pinned: HashSet<String>,       // linked issues, never evicted
    total_count: Option<u32>,      // from API response headers
}
```

- On initial connect, the TUI reports its viewport size. The daemon fetches enough issues to fill the viewport plus a buffer (roughly 2x visible rows).
- After correlation, the daemon identifies linked issue IDs not in the cache and batch-fetches them via `fetch_issues_by_id`. These are pinned.
- When the TUI scrolls past the last issue, it requests more. The daemon fetches the next page and appends.
- Eviction: non-pinned entries can be evicted when the cache exceeds a threshold (e.g. 500). Not critical for initial implementation.

### Protocol changes

New commands (TUI → Daemon):

```rust
Command::SetIssueViewport { repo: PathBuf, visible_count: usize }
Command::FetchMoreIssues { repo: PathBuf, desired_count: usize }
Command::SearchIssues { repo: PathBuf, query: String }
Command::ClearIssueSearch { repo: PathBuf }
```

Snapshot additions:

```rust
pub issue_total: Option<u32>,
pub issue_has_more: bool,
pub issue_search_results: Option<Vec<Issue>>,
```

No new event types — responses flow through the existing `DaemonEvent::Snapshot` broadcast.

### Provider trait changes

`IssueTracker` gains three methods:

```rust
async fn list_issues_page(
    &self, repo_root: &Path, page: u32, per_page: usize,
) -> Result<IssuePage, String>;

async fn fetch_issues_by_id(
    &self, repo_root: &Path, ids: &[String],
) -> Result<Vec<Issue>, String>;

async fn search_issues(
    &self, repo_root: &Path, query: &str, limit: usize,
) -> Result<Vec<Issue>, String>;
```

```rust
pub struct IssuePage {
    pub issues: Vec<Issue>,
    pub total_count: Option<u32>,
    pub has_more: bool,
}
```

The existing `list_issues` can be reimplemented in terms of `list_issues_page(1, limit)`.

GitHub implementation:
- `list_issues_page` → `GET /repos/:owner/:repo/issues?state=open&page=N&per_page=M`, parse `Link` header for `has_more`
- `fetch_issues_by_id` → parallel `GET /repos/:owner/:repo/issues/:number` calls
- `search_issues` → `GET /search/issues?q=repo:owner/repo+is:issue+is:open+{query}`

### Daemon lifecycle

**Initial load:**
1. TUI sends `SetIssueViewport { visible_count }`.
2. Daemon fetches page 1 with `per_page = visible_count * 2`.
3. Correlation identifies linked issue IDs missing from cache.
4. Daemon batch-fetches missing linked issues, pins them.
5. Broadcasts snapshot.

**Refresh cycle:**
- Periodic refresh no longer fetches issues — it refreshes checkouts, PRs, sessions, workspaces, branches.
- After correlation, any newly-linked issue IDs missing from cache are batch-fetched and pinned.
- Cached issues are included in snapshots as-is.

**Scroll:**
1. TUI sends `FetchMoreIssues { desired_count }`.
2. Daemon compares against cache size, fetches next page(s) if needed.
3. Merges into cache, re-correlates, broadcasts.

**Search:**
1. TUI sends `SearchIssues { query }`.
2. Daemon hits GitHub search API.
3. Broadcasts snapshot with `issue_search_results` populated.
4. `ClearIssueSearch` clears the overlay.

### Multi-client support

- The `IssueCache` is shared per-repo. If one client scrolls deep, all clients benefit.
- `SetIssueViewport` acts as a high-water mark — the daemon ensures at least that many issues are cached.
- Search state is per-client (`HashMap<ClientId, IssueSearchState>`). Each client's snapshot includes the shared cache but their own search overlay.
- In the current in-process daemon (single client), per-client state is trivial.

### TUI changes

- Standalone issues section supports infinite scroll: navigating past the last issue triggers `FetchMoreIssues`.
- A "loading..." indicator appears when `issue_has_more` is true and the user is near the bottom.
- `/` key enters search mode for issues, sends `SearchIssues`, results replace the issue list until `Esc` sends `ClearIssueSearch`.
- Status line shows "N of M issues" using `issue_total`.

## What this does NOT cover

- Client-side filtering (filter cached results locally) — future enhancement
- Closed issue browsing — only open issues for now
- Issue creation/editing from the TUI
- Non-GitHub issue trackers (design is provider-agnostic, but only GitHub is implemented)
