# Issue Cache Robustness Design

Addresses: #115, #134 (and closes already-fixed #116, #117, #118)

## Problem

The issue cache populates once and never refreshes. Manual and automatic refresh
cycles re-fetch PRs, branches, sessions, and workspaces — but not issues. Once
cached, issues go stale: new issues don't appear, closed issues linger, metadata
changes are invisible.

Separately, `fetch_issues_by_id` fires unbounded concurrent API calls — a repo
with 50 PRs each linking 3 issues means 150 simultaneous requests.

## Change Scenarios

| Change | Effect on cache | Detection |
|--------|----------------|-----------|
| New issue filed | Missing from cache | `since` returns it |
| Issue closed | Should disappear from open view | `state=all&since=T` returns it as closed |
| Issue reopened | Should reappear | `state=all&since=T` returns it as open |
| Metadata change | Stale content | `since=T` returns updated version |

All four cases are captured by a single `state=all&since=last_refresh` query
against GitHub's list issues endpoint.

## Two-Tier Refresh Model

**Periodic (background, every refresh cycle):** lightweight incremental sync.
One API call per cycle using the `since` parameter. Merge open issues into cache,
evict closed ones. If more than one page of changes, escalate to a full re-fetch.

**Client-initiated (manual `r`):** guaranteed-fresh. Reset cache and re-fetch up
to the previous cached count. ETag caching at the API layer makes unchanged pages
return 304 (free — doesn't count against rate limit).

## Design

### New Protocol Type: `IssueChangeset`

```rust
pub struct IssueChangeset {
    pub updated: Vec<Issue>,      // new or changed open issues
    pub closed_ids: Vec<String>,  // now closed — evict from cache
    pub has_more: bool,           // too many changes → escalate to full refresh
}
```

### IssueTracker Trait Addition

One new method with a default fallback so future providers (Jira, Linear, GitLab)
aren't forced to implement it immediately:

```rust
async fn list_issues_changed_since(
    &self, repo: &Path, since: &str, per_page: usize
) -> Result<IssueChangeset, String> {
    // Default: full page 1 fetch, treat all as updated, no evictions
    let page = self.list_issues_page(repo, 1, per_page).await?;
    Ok(IssueChangeset {
        updated: page.issues,
        closed_ids: vec![],
        has_more: page.has_more,
    })
}
```

GitHub implementation uses `state=all&since={since}&sort=updated&direction=desc&per_page=50`.
Partitions response into open (→ `updated`) and closed (→ `closed_ids`).

### IssueCache Changes

New field:

- `last_refreshed_at: Option<String>` — ISO 8601 timestamp of last successful refresh

New methods:

- `apply_changeset(changeset)` — upsert updated issues, remove closed_ids
  (unless pinned — pinned issues survive because they're linked to PRs via
  correlation).
- `reset()` — clear non-pinned entries, reset `next_page` to 1, set `has_more`
  to true. Pinned issues and the pinned set itself are preserved.

### Refresh Integration

**Periodic** (in `poll_snapshots`, after provider data arrives):
1. If `last_refreshed_at` is set, call `list_issues_changed_since(since, 50)`
2. Apply changeset. If `has_more`, escalate: `reset()` + `ensure_issues_cached(previous_count)`
3. Update `last_refreshed_at` to now
4. Broadcast snapshot

**Manual refresh** (`RefreshAll` / `RefreshRepo` command):
1. Note current cache size
2. `reset()` + `ensure_issues_cached(previous_count)`
3. Broadcast snapshot

### Rate Limit Protection for `fetch_issues_by_id` (#115)

Replace `futures::future::join_all` with
`futures::stream::iter(...).buffer_unordered(10)` to cap concurrent API calls
at 10.

## What We're Not Doing

- Not touching search architecture (#114 — Bundle B)
- Not adding GraphQL — REST `since` parameter is sufficient
- Not using the Events API — one extra endpoint for marginal gain
- Not changing the default sort order (created desc is stable)

## Already Fixed

Review of the codebase shows three issues from PR #107 review were already
addressed in the implementation:

- **#117** — `ensure_issues_cached` already drops locks before network calls
- **#116** — `clamp_per_page` exists; Link header guard is correct
- **#118** — All issue commands already call `broadcast_snapshot()`

These can be closed with a note.
