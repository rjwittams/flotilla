use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Worktree {
    pub branch: String,
    pub path: PathBuf,
    #[serde(default)]
    pub is_main: bool,
    #[serde(default)]
    pub is_current: bool,
    pub main_state: Option<String>,
    pub main: Option<AheadBehind>,
    pub remote: Option<RemoteStatus>,
    pub working_tree: Option<WorkingTree>,
    pub commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AheadBehind {
    pub ahead: i64,
    pub behind: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteStatus {
    pub name: Option<String>,
    pub branch: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkingTree {
    #[serde(default)]
    pub staged: bool,
    #[serde(default)]
    pub modified: bool,
    #[serde(default)]
    pub untracked: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitInfo {
    pub short_sha: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubPr {
    pub number: i64,
    pub title: String,
    #[serde(rename = "headRefName")]
    pub head_ref_name: String,
    pub state: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubIssue {
    pub number: i64,
    pub title: String,
    pub labels: Vec<Label>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Default, Clone)]
pub struct DataStore {
    pub worktrees: Vec<Worktree>,
    pub prs: Vec<GithubPr>,
    pub issues: Vec<GithubIssue>,
    pub cmux_workspaces: Vec<String>,
    pub loading: bool,
}

impl DataStore {
    pub async fn refresh(&mut self, repo_root: &PathBuf) {
        self.loading = true;
        let (wt, prs, issues, ws) = tokio::join!(
            fetch_worktrees(repo_root),
            fetch_prs(repo_root),
            fetch_issues(repo_root),
            fetch_cmux_workspaces(),
        );
        self.worktrees = wt.unwrap_or_default();
        self.prs = prs.unwrap_or_default();
        self.issues = issues.unwrap_or_default();
        self.cmux_workspaces = ws.unwrap_or_default();
        self.loading = false;
    }
}

async fn run_command(cmd: &str, args: &[&str], cwd: Option<&PathBuf>) -> Result<String, String> {
    let mut command = tokio::process::Command::new(cmd);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command.output().await.map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

async fn fetch_worktrees(repo_root: &PathBuf) -> Result<Vec<Worktree>, String> {
    let output = run_command("wt", &["list", "--format=json"], Some(repo_root)).await?;
    serde_json::from_str(&output).map_err(|e| e.to_string())
}

async fn fetch_prs(repo_root: &PathBuf) -> Result<Vec<GithubPr>, String> {
    let output = run_command(
        "gh",
        &["pr", "list", "--json", "number,title,headRefName,state,updatedAt", "--limit", "20"],
        Some(repo_root),
    ).await?;
    serde_json::from_str(&output).map_err(|e| e.to_string())
}

async fn fetch_issues(repo_root: &PathBuf) -> Result<Vec<GithubIssue>, String> {
    let output = run_command(
        "gh",
        &["issue", "list", "--json", "number,title,labels,updatedAt", "--limit", "20", "--state", "open"],
        Some(repo_root),
    ).await?;
    serde_json::from_str(&output).map_err(|e| e.to_string())
}

async fn fetch_cmux_workspaces() -> Result<Vec<String>, String> {
    let output = run_command(
        "/Applications/cmux.app/Contents/Resources/bin/cmux",
        &["list-workspaces"],
        None,
    ).await?;
    // Parse text format: "* workspace:14  scratch  [selected]"
    Ok(output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_start_matches('*').trim();
            // Skip the workspace:N ref, get the name
            let parts: Vec<&str> = trimmed.splitn(2, "  ").collect();
            parts.get(1).map(|s| s.trim().trim_end_matches("[selected]").trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .collect())
}
