use async_trait::async_trait;
use reqwest;
use serde::Deserialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use crate::providers::types::*;
use crate::providers::CommandRunner;
use tracing::{debug, info, warn};

pub struct ClaudeCodingAgent {
    provider_name: String,
    runner: Arc<dyn CommandRunner>,
    sessions_cache: Mutex<SessionsCache>,
}

impl ClaudeCodingAgent {
    pub fn new(provider_name: String, runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            provider_name,
            runner,
            sessions_cache: Mutex::new(SessionsCache {
                sessions: Vec::new(),
                fetched_at: None,
                known_ids: std::collections::HashSet::new(),
            }),
        }
    }
}

// ---------- internal auth types ----------

#[derive(Deserialize)]
struct OAuthCredentials {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthToken,
}

#[derive(Deserialize, Clone)]
struct OAuthToken {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: i64,
}

struct AuthCache {
    token: Option<OAuthToken>,
}

static AUTH_CACHE: LazyLock<Mutex<AuthCache>> =
    LazyLock::new(|| Mutex::new(AuthCache { token: None }));

/// Guard so the "sessions unavailable" warning is emitted only once per process.
static AUTH_WARNED: AtomicBool = AtomicBool::new(false);

// ---------- API deserialization types ----------

#[derive(Deserialize)]
struct SessionsResponse {
    data: Vec<WebSession>,
}

#[derive(Debug, Clone, Deserialize)]
struct WebSession {
    id: String,
    title: String,
    session_status: String,
    #[serde(default)]
    #[allow(dead_code)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    session_context: SessionContext,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SessionContext {
    #[serde(default)]
    model: String,
    #[serde(default)]
    outcomes: Vec<SessionOutcome>,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionOutcome {
    #[serde(default)]
    git_info: Option<SessionGitInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct SessionGitInfo {
    #[serde(default)]
    branches: Vec<String>,
    /// "owner/repo" slug (e.g. "changedirection/reticulate")
    #[serde(default)]
    repo: Option<String>,
}

impl WebSession {
    fn branch(&self) -> Option<&str> {
        self.session_context
            .outcomes
            .first()
            .and_then(|o| o.git_info.as_ref())
            .and_then(|gi| gi.branches.first())
            .map(|s| s.as_str())
    }

    fn repo_slug(&self) -> Option<&str> {
        self.session_context
            .outcomes
            .first()
            .and_then(|o| o.git_info.as_ref())
            .and_then(|gi| gi.repo.as_deref())
    }
}

// ---------- sessions cache ----------

struct SessionsCache {
    sessions: Vec<WebSession>,
    fetched_at: Option<Instant>,
    known_ids: std::collections::HashSet<String>,
}

const SESSIONS_CACHE_TTL_SECS: u64 = 30;
const CLAUDE_API_BASE_URL: &str = "https://api.anthropic.com";

fn sessions_url_for(base_url: &str) -> String {
    format!("{}/v1/sessions", base_url.trim_end_matches('/'))
}

fn session_url_for(base_url: &str, session_id: &str) -> String {
    format!(
        "{}/v1/sessions/{session_id}",
        base_url.trim_end_matches('/')
    )
}

// ---------- auth helpers ----------

async fn read_oauth_token_from_keychain(runner: &dyn CommandRunner) -> Result<OAuthToken, String> {
    let output = runner
        .run(
            "security",
            &[
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ],
            Path::new("."),
        )
        .await
        .map_err(|_| "No Claude Code credentials in keychain".to_string())?;
    let json = output.trim();
    let creds: OAuthCredentials = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(creds.claude_ai_oauth)
}

async fn get_oauth_token(runner: &dyn CommandRunner) -> Result<OAuthToken, String> {
    {
        let cache = AUTH_CACHE.lock().unwrap();
        if let Some(ref token) = cache.token {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            if token.expires_at > now + 60 {
                return Ok(token.clone());
            }
        }
    }
    // Token missing or expiring soon — re-read from keychain
    let token = read_oauth_token_from_keychain(runner).await?;
    let mut cache = AUTH_CACHE.lock().unwrap();
    cache.token = Some(token.clone());
    Ok(token)
}

fn invalidate_auth_cache() {
    let mut cache = AUTH_CACHE.lock().unwrap();
    cache.token = None;
}

impl ClaudeCodingAgent {
    /// Fetch all non-archived sessions from the API, sorted by updated_at descending.
    /// Returns empty list on auth errors (insufficient scopes, expired token) to
    /// degrade gracefully instead of spamming errors on every refresh cycle.
    async fn fetch_sessions(runner: &dyn CommandRunner) -> Result<Vec<WebSession>, String> {
        Self::fetch_sessions_with_base(runner, CLAUDE_API_BASE_URL).await
    }

    async fn fetch_sessions_with_base(
        runner: &dyn CommandRunner,
        base_url: &str,
    ) -> Result<Vec<WebSession>, String> {
        match Self::fetch_sessions_inner_with_base(runner, base_url).await {
            Ok(sessions) => Ok(sessions),
            Err(e) if e.contains("authentication") || e.contains("missing field `data`") => {
                debug!("session fetch failed, clearing auth cache and retrying: {e}");
                invalidate_auth_cache();
                match Self::fetch_sessions_inner_with_base(runner, base_url).await {
                    Ok(sessions) => Ok(sessions),
                    Err(e) if e.contains("authentication") => {
                        if !AUTH_WARNED.swap(true, Ordering::Relaxed) {
                            warn!("Claude sessions unavailable: insufficient OAuth scopes");
                        }
                        debug!("Claude auth error detail: {e}");
                        Ok(vec![])
                    }
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn fetch_sessions_inner_with_base(
        runner: &dyn CommandRunner,
        base_url: &str,
    ) -> Result<Vec<WebSession>, String> {
        let token = get_oauth_token(runner).await?;
        let access_token = token.access_token;

        let client = reqwest::Client::new();
        let resp = client
            .get(sessions_url_for(base_url))
            .bearer_auth(&access_token)
            .header("anthropic-beta", "ccr-byoc-2025-07-29")
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        if status == 401 || status == 403 {
            return Err(format!("authentication error (HTTP {status})"));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("session fetch failed (HTTP {status}): {body}"));
        }

        let resp: SessionsResponse = resp
            .json()
            .await
            .map_err(|e| format!("session parse error: {e}"))?;

        let mut sessions: Vec<WebSession> = resp
            .data
            .into_iter()
            .filter(|s| s.session_status != "archived")
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    async fn archive_session_with_base(
        &self,
        session_id: &str,
        base_url: &str,
    ) -> Result<(), String> {
        info!("archiving session {session_id}");
        let token = get_oauth_token(&*self.runner).await?;
        let access_token = token.access_token;

        let url = session_url_for(base_url, session_id);
        let client = reqwest::Client::new();
        let resp = client
            .patch(&url)
            .bearer_auth(&access_token)
            .header("anthropic-beta", "ccr-byoc-2025-07-29")
            .header("anthropic-version", "2023-06-01")
            .json(&serde_json::json!({"session_status": "archived"}))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(format!("archive session failed (HTTP {status}): {body}"))
        }
    }
}

// ---------- trait implementation ----------

#[async_trait]
impl super::CodingAgent for ClaudeCodingAgent {
    fn display_name(&self) -> &str {
        "Claude Sessions"
    }

    async fn list_sessions(
        &self,
        criteria: &RepoCriteria,
    ) -> Result<Vec<(String, CloudAgentSession)>, String> {
        // Check instance cache
        let cached = {
            let cache = self.sessions_cache.lock().unwrap();
            if let Some(fetched_at) = cache.fetched_at {
                if fetched_at.elapsed().as_secs() < SESSIONS_CACHE_TTL_SECS {
                    Some(cache.sessions.clone())
                } else {
                    None
                }
            } else {
                None
            }
        };

        let sessions = if let Some(sessions) = cached {
            debug!("Claude sessions: cache hit");
            sessions
        } else {
            let fetched = Self::fetch_sessions(&*self.runner).await?;
            debug!("Claude sessions: fetched {} from API", fetched.len());

            // Diff against known IDs and log additions/removals at INFO
            let mut cache = self.sessions_cache.lock().unwrap();
            let new_ids: std::collections::HashSet<String> =
                fetched.iter().map(|s| s.id.clone()).collect();
            if !cache.known_ids.is_empty() {
                for s in &fetched {
                    if !cache.known_ids.contains(&s.id) {
                        info!("session appeared: {} ({})", s.title, s.id);
                    }
                }
                for old_id in &cache.known_ids {
                    if !new_ids.contains(old_id) {
                        info!("session gone: {}", old_id);
                    }
                }
            }
            cache.known_ids = new_ids;
            cache.sessions = fetched.clone();
            cache.fetched_at = Some(Instant::now());
            fetched
        };

        // No remote slug means no cloud sessions can match this repo
        let Some(ref slug) = criteria.repo_slug else {
            return Ok(vec![]);
        };

        // Sessions with no repo info still match (backward compat with older sessions)
        let filtered: Vec<WebSession> = sessions
            .into_iter()
            .filter(|s| s.repo_slug().is_none_or(|r| r == slug))
            .collect();

        let provider_name = &self.provider_name;
        Ok(filtered
            .into_iter()
            .map(|s| {
                let status = match s.session_status.as_str() {
                    "running" => SessionStatus::Running,
                    "archived" => SessionStatus::Archived,
                    _ => SessionStatus::Idle,
                };

                let model = if s.session_context.model.is_empty() {
                    None
                } else {
                    Some(s.session_context.model.clone())
                };

                let id = s.id.clone();
                let mut correlation_keys = vec![CorrelationKey::SessionRef(
                    provider_name.clone(),
                    id.clone(),
                )];

                // Add branch correlation key if available
                if let Some(branch) = s.branch() {
                    let clean = branch
                        .strip_prefix("refs/heads/")
                        .unwrap_or(branch)
                        .to_string();
                    correlation_keys.push(CorrelationKey::Branch(clean));
                }

                (
                    id,
                    CloudAgentSession {
                        title: s.title,
                        status,
                        model,
                        updated_at: Some(s.updated_at.clone()),
                        correlation_keys,
                    },
                )
            })
            .collect())
    }

    async fn archive_session(&self, session_id: &str) -> Result<(), String> {
        self.archive_session_with_base(session_id, CLAUDE_API_BASE_URL)
            .await
    }

    async fn attach_command(&self, session_id: &str) -> Result<String, String> {
        Ok(format!("claude --teleport {session_id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::coding_agent::CodingAgent;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as AsyncMutex;

    static TEST_LOCK: LazyLock<AsyncMutex<()>> = LazyLock::new(|| AsyncMutex::new(()));

    #[derive(Debug, Clone)]
    struct TestResponse {
        status: u16,
        body: String,
    }

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: HashMap<String, String>,
        body: String,
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> CapturedRequest {
        let mut buf = Vec::new();
        let mut read_buf = [0u8; 1024];
        let header_end = loop {
            let n = stream.read(&mut read_buf).await.expect("read request");
            assert!(n > 0, "unexpected EOF before headers");
            buf.extend_from_slice(&read_buf[..n]);
            if let Some(pos) = find_bytes(&buf, b"\r\n\r\n") {
                break pos + 4;
            }
        };

        let header_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut lines = header_text.split("\r\n").filter(|line| !line.is_empty());
        let request_line = lines.next().expect("missing request line");
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

        let mut headers = HashMap::new();
        let mut content_length = 0usize;
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                let value = v.trim().to_string();
                if key == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.insert(key, value);
            }
        }

        while buf.len() < header_end + content_length {
            let n = stream.read(&mut read_buf).await.expect("read request body");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&read_buf[..n]);
        }

        let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
        CapturedRequest {
            method,
            path,
            headers,
            body,
        }
    }

    fn status_text(status: u16) -> &'static str {
        match status {
            200 => "OK",
            201 => "Created",
            401 => "Unauthorized",
            403 => "Forbidden",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }

    async fn spawn_test_server(
        responses: Vec<TestResponse>,
    ) -> (
        String,
        Arc<Mutex<Vec<CapturedRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let queued = Arc::new(Mutex::new(VecDeque::from(responses)));
        let captured = Arc::new(Mutex::new(Vec::new()));
        let queued_clone = Arc::clone(&queued);
        let captured_clone = Arc::clone(&captured);

        let task = tokio::spawn(async move {
            loop {
                let response = {
                    let mut guard = queued_clone.lock().unwrap();
                    guard.pop_front()
                };
                let Some(response) = response else {
                    break;
                };
                let (mut stream, _) = listener.accept().await.expect("accept");
                let request = read_http_request(&mut stream).await;
                captured_clone.lock().unwrap().push(request);

                let status = response.status;
                let body = response.body;
                let wire = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    status,
                    status_text(status),
                    body.len(),
                    body
                );
                stream
                    .write_all(wire.as_bytes())
                    .await
                    .expect("write response");
            }
        });

        (base_url, captured, task)
    }

    fn now_epoch_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn token_json(access_token: &str, expires_at: i64) -> String {
        serde_json::json!({
            "claudeAiOauth": {
                "accessToken": access_token,
                "expiresAt": expires_at
            }
        })
        .to_string()
    }

    fn reset_auth_state() {
        invalidate_auth_cache();
        AUTH_WARNED.store(false, Ordering::Relaxed);
    }

    fn mock_runner(
        responses: Vec<Result<String, String>>,
    ) -> crate::providers::testing::MockRunner {
        crate::providers::testing::MockRunner::new(responses)
    }

    fn mock_runner_arc(responses: Vec<Result<String, String>>) -> Arc<dyn CommandRunner> {
        Arc::new(mock_runner(responses))
    }

    #[tokio::test]
    async fn oauth_token_is_cached_until_near_expiry() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner(vec![Ok(token_json("token-1", now_epoch_secs() + 3600))]);
        let token1 = get_oauth_token(&runner).await.expect("first token");
        let token2 = get_oauth_token(&runner).await.expect("cached token");
        assert_eq!(token1.access_token, "token-1");
        assert_eq!(token2.access_token, "token-1");
    }

    #[tokio::test]
    async fn oauth_token_refreshes_when_expiring_soon() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner(vec![
            Ok(token_json("old-token", now_epoch_secs() + 10)),
            Ok(token_json("new-token", now_epoch_secs() + 3600)),
        ]);
        let first = get_oauth_token(&runner).await.expect("first token");
        let second = get_oauth_token(&runner).await.expect("refreshed token");
        assert_eq!(first.access_token, "old-token");
        assert_eq!(second.access_token, "new-token");
    }

    #[tokio::test]
    async fn fetch_sessions_inner_filters_archived_sorts_and_sends_auth_header() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner(vec![Ok(token_json("abc123", now_epoch_secs() + 3600))]);
        let body = serde_json::json!({
            "data": [
                {
                    "id": "old",
                    "title": "Older",
                    "session_status": "running",
                    "updated_at": "2026-03-01T00:00:00Z",
                    "session_context": {"model": "opus", "outcomes": []}
                },
                {
                    "id": "skip",
                    "title": "Archived",
                    "session_status": "archived",
                    "updated_at": "2026-03-03T00:00:00Z",
                    "session_context": {"model": "opus", "outcomes": []}
                },
                {
                    "id": "new",
                    "title": "Newer",
                    "session_status": "idle",
                    "updated_at": "2026-03-02T00:00:00Z",
                    "session_context": {"model": "sonnet", "outcomes": []}
                }
            ]
        })
        .to_string();
        let (base_url, captured, server_task) =
            spawn_test_server(vec![TestResponse { status: 200, body }]).await;

        let sessions = ClaudeCodingAgent::fetch_sessions_inner_with_base(&runner, &base_url)
            .await
            .expect("fetch sessions");
        server_task.await.expect("server task");

        assert_eq!(sessions.len(), 2, "archived sessions should be filtered");
        assert_eq!(sessions[0].id, "new", "sessions should be sorted desc");
        assert_eq!(sessions[1].id, "old");

        let captured = captured.lock().unwrap();
        let req = captured.first().expect("captured request");
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/v1/sessions");
        assert_eq!(
            req.headers.get("authorization").map(String::as_str),
            Some("Bearer abc123")
        );
    }

    #[tokio::test]
    async fn fetch_sessions_retries_after_auth_error() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner(vec![
            Ok(token_json("expired", now_epoch_secs() + 3600)),
            Ok(token_json("fresh", now_epoch_secs() + 3600)),
        ]);
        let (base_url, captured, server_task) = spawn_test_server(vec![
            TestResponse {
                status: 401,
                body: "{}".to_string(),
            },
            TestResponse {
                status: 200,
                body: serde_json::json!({"data":[{"id":"s1","title":"Recovered","session_status":"running","updated_at":"2026-03-02T00:00:00Z","session_context":{"model":"","outcomes":[]}}]}).to_string(),
            },
        ])
        .await;

        let sessions = ClaudeCodingAgent::fetch_sessions_with_base(&runner, &base_url)
            .await
            .expect("retry should succeed");
        server_task.await.expect("server task");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[0].headers.get("authorization").map(String::as_str),
            Some("Bearer expired")
        );
        assert_eq!(
            captured[1].headers.get("authorization").map(String::as_str),
            Some("Bearer fresh")
        );
    }

    #[tokio::test]
    async fn fetch_sessions_returns_empty_after_second_auth_error() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner(vec![
            Ok(token_json("bad-1", now_epoch_secs() + 3600)),
            Ok(token_json("bad-2", now_epoch_secs() + 3600)),
        ]);
        let (base_url, captured, server_task) = spawn_test_server(vec![
            TestResponse {
                status: 403,
                body: "{}".to_string(),
            },
            TestResponse {
                status: 401,
                body: "{}".to_string(),
            },
        ])
        .await;

        let sessions = ClaudeCodingAgent::fetch_sessions_with_base(&runner, &base_url)
            .await
            .expect("auth failures should degrade gracefully");
        server_task.await.expect("server task");

        assert!(sessions.is_empty());
        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(
            captured[0].headers.get("authorization").map(String::as_str),
            Some("Bearer bad-1")
        );
        assert_eq!(
            captured[1].headers.get("authorization").map(String::as_str),
            Some("Bearer bad-2")
        );
    }

    #[tokio::test]
    async fn list_sessions_uses_cache_and_maps_fields() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner_arc(vec![]);
        let agent = ClaudeCodingAgent::new("claude".into(), runner);
        {
            let mut cache = agent.sessions_cache.lock().unwrap();
            cache.sessions = vec![
                WebSession {
                    id: "one".into(),
                    title: "One".into(),
                    session_status: "running".into(),
                    created_at: String::new(),
                    updated_at: "2026-03-05T00:00:00Z".into(),
                    session_context: SessionContext {
                        model: "sonnet".into(),
                        outcomes: vec![SessionOutcome {
                            git_info: Some(SessionGitInfo {
                                branches: vec!["refs/heads/feat-a".into()],
                                repo: Some("owner/repo".into()),
                            }),
                        }],
                    },
                },
                WebSession {
                    id: "two".into(),
                    title: "Two".into(),
                    session_status: "something-else".into(),
                    created_at: String::new(),
                    updated_at: "2026-03-04T00:00:00Z".into(),
                    session_context: SessionContext {
                        model: String::new(),
                        outcomes: vec![SessionOutcome { git_info: None }],
                    },
                },
                WebSession {
                    id: "skip".into(),
                    title: "Skip".into(),
                    session_status: "running".into(),
                    created_at: String::new(),
                    updated_at: "2026-03-03T00:00:00Z".into(),
                    session_context: SessionContext {
                        model: "opus".into(),
                        outcomes: vec![SessionOutcome {
                            git_info: Some(SessionGitInfo {
                                branches: vec!["refs/heads/feat-b".into()],
                                repo: Some("other/repo".into()),
                            }),
                        }],
                    },
                },
            ];
            cache.fetched_at = Some(Instant::now());
        }

        let sessions = agent
            .list_sessions(&RepoCriteria {
                repo_slug: Some("owner/repo".into()),
            })
            .await
            .expect("list sessions");

        assert_eq!(sessions.len(), 2);
        let one = sessions
            .iter()
            .find(|(id, _)| id == "one")
            .expect("one session");
        assert_eq!(one.1.status, SessionStatus::Running);
        assert_eq!(one.1.model.as_deref(), Some("sonnet"));
        assert!(one
            .1
            .correlation_keys
            .contains(&CorrelationKey::Branch("feat-a".into())));

        let two = sessions
            .iter()
            .find(|(id, _)| id == "two")
            .expect("two session");
        assert_eq!(two.1.status, SessionStatus::Idle);
        assert!(two.1.model.is_none());
    }

    #[tokio::test]
    async fn archive_session_sends_patch_and_returns_error_on_failure() {
        let _test_lock = TEST_LOCK.lock().await;
        reset_auth_state();
        let runner = mock_runner_arc(vec![
            Ok(token_json("archive-token", now_epoch_secs() + 3600)),
            Ok(token_json("archive-token", now_epoch_secs() + 3600)),
        ]);
        let agent = ClaudeCodingAgent::new("claude".into(), runner);
        let (base_url, captured, server_task) = spawn_test_server(vec![
            TestResponse {
                status: 200,
                body: "{}".to_string(),
            },
            TestResponse {
                status: 500,
                body: "boom".to_string(),
            },
        ])
        .await;

        agent
            .archive_session_with_base("s-ok", &base_url)
            .await
            .expect("first archive should succeed");
        let err = agent
            .archive_session_with_base("s-fail", &base_url)
            .await
            .expect_err("second archive should fail");
        server_task.await.expect("server task");

        assert!(err.contains("HTTP 500"));
        assert!(err.contains("boom"));

        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].method, "PATCH");
        assert_eq!(captured[0].path, "/v1/sessions/s-ok");
        assert!(
            captured[0].body.contains("\"session_status\":\"archived\""),
            "archive payload should include archived status"
        );
    }

    #[tokio::test]
    async fn attach_command_formats_teleport_command() {
        let _test_lock = TEST_LOCK.lock().await;
        let runner = mock_runner_arc(vec![]);
        let agent = ClaudeCodingAgent::new("claude".into(), runner);
        let cmd = agent
            .attach_command("abc123")
            .await
            .expect("attach command");
        assert_eq!(cmd, "claude --teleport abc123");
    }
}
