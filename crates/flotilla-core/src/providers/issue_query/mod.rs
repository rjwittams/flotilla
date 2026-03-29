use std::path::Path;

use async_trait::async_trait;
pub use flotilla_protocol::issue_query::{CursorId, IssueQuery, IssueResultPage};
use flotilla_protocol::provider_data::Issue;

/// Cursor-based query interface for issue listing and search.
#[async_trait]
pub trait IssueQueryService: Send + Sync {
    /// Open a query cursor. The default listing uses `IssueQuery { search: None }`.
    async fn open_query(&self, repo: &Path, params: IssueQuery) -> Result<CursorId, String>;

    /// Fetch the next page for a cursor.
    async fn fetch_page(&self, cursor: &CursorId, count: usize) -> Result<IssueResultPage, String>;

    /// Close a cursor. Cursors also expire after a period of inactivity.
    async fn close_query(&self, cursor: &CursorId);

    /// Fetch specific issues by ID (for linked/pinned issue resolution).
    async fn fetch_by_ids(&self, repo: &Path, ids: &[String]) -> Result<Vec<(String, Issue)>, String>;

    /// Open an issue in the browser.
    async fn open_in_browser(&self, repo: &Path, id: &str) -> Result<(), String>;
}
