//! GitHub implementation of the IssueQueryService.

use std::{collections::HashMap, path::Path, sync::Arc};

use async_trait::async_trait;
use flotilla_protocol::provider_data::Issue;
use tokio::sync::Mutex;

use super::{CursorId, IssueQuery, IssueQueryService, IssueResultPage};
use crate::providers::{github_api::GhApiClient, CommandRunner};

struct CursorState {
    #[allow(dead_code)]
    query: IssueQuery,
    #[allow(dead_code)]
    repo_slug: String,
    #[allow(dead_code)]
    next_page: u32,
    #[allow(dead_code)]
    has_more: bool,
    total: Option<u32>,
}

pub struct GitHubIssueQueryService {
    repo_slug: String,
    #[allow(dead_code)]
    api: Arc<GhApiClient>,
    #[allow(dead_code)]
    runner: Arc<dyn CommandRunner>,
    cursors: Mutex<HashMap<CursorId, CursorState>>,
    next_cursor_id: std::sync::atomic::AtomicU64,
}

impl GitHubIssueQueryService {
    pub fn new(repo_slug: String, api: Arc<GhApiClient>, runner: Arc<dyn CommandRunner>) -> Self {
        Self { repo_slug, api, runner, cursors: Mutex::new(HashMap::new()), next_cursor_id: std::sync::atomic::AtomicU64::new(1) }
    }
}

#[async_trait]
impl IssueQueryService for GitHubIssueQueryService {
    async fn open_query(&self, _repo: &Path, params: IssueQuery) -> Result<CursorId, String> {
        let id = self.next_cursor_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let cursor_id = CursorId::new(format!("gh-{id}"));
        let state = CursorState { query: params, repo_slug: self.repo_slug.clone(), next_page: 1, has_more: true, total: None };
        self.cursors.lock().await.insert(cursor_id.clone(), state);
        Ok(cursor_id)
    }

    async fn fetch_page(&self, cursor: &CursorId, _count: usize) -> Result<IssueResultPage, String> {
        let cursors = self.cursors.lock().await;
        let state = cursors.get(cursor).ok_or_else(|| format!("unknown cursor: {:?}", cursor.0))?;
        Ok(IssueResultPage { items: vec![], total: state.total, has_more: false })
    }

    async fn close_query(&self, cursor: &CursorId) {
        self.cursors.lock().await.remove(cursor);
    }

    async fn fetch_by_ids(&self, _repo: &Path, _ids: &[String]) -> Result<Vec<(String, Issue)>, String> {
        Ok(vec![])
    }

    async fn open_in_browser(&self, _repo: &Path, _id: &str) -> Result<(), String> {
        Ok(())
    }
}
