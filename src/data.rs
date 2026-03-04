use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use crate::providers::types::{
    ChangeRequest, CloudAgentSession, CorrelationKey, Issue, Workspace,
};
use crate::providers::registry::ProviderRegistry;

#[derive(Debug, Clone, Deserialize)]
pub struct Worktree {
    pub branch: String,
    pub path: PathBuf,
    #[serde(default)]
    pub is_main: bool,
    #[serde(default)]
    #[allow(dead_code)]
    pub is_current: bool,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub name: Option<String>,
    #[allow(dead_code)]
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkItemKind {
    Worktree,
    Session,
    Pr,
    RemoteBranch,
    Issue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionHeader {
    Worktrees,
    Sessions,
    PullRequests,
    RemoteBranches,
    Issues,
}

impl fmt::Display for SectionHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SectionHeader::Worktrees => write!(f, "Worktrees"),
            SectionHeader::Sessions => write!(f, "Sessions"),
            SectionHeader::PullRequests => write!(f, "Pull Requests"),
            SectionHeader::RemoteBranches => write!(f, "Remote Branches"),
            SectionHeader::Issues => write!(f, "Issues"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TableEntry {
    Header(SectionHeader),
    Item(WorkItem),
}

#[derive(Debug, Clone)]
pub struct WorkItem {
    pub kind: WorkItemKind,
    pub branch: Option<String>,
    pub description: String,
    pub worktree_idx: Option<usize>,
    pub is_main_worktree: bool,
    pub pr_idx: Option<usize>,
    pub session_idx: Option<usize>,
    pub issue_idxs: Vec<usize>,
    pub workspace_refs: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct DataStore {
    pub worktrees: Vec<Worktree>,
    pub change_requests: Vec<ChangeRequest>,
    pub issues: Vec<Issue>,
    pub workspaces: Vec<Workspace>,
    pub sessions: Vec<CloudAgentSession>,
    pub remote_branches: Vec<String>,
    pub merged_branches: Vec<String>,
    pub table_entries: Vec<TableEntry>,
    pub selectable_indices: Vec<usize>,
    pub loading: bool,
}

impl DataStore {
    pub async fn refresh(&mut self, repo_root: &PathBuf, registry: &ProviderRegistry) -> Vec<String> {
        self.loading = true;
        let mut errors = Vec::new();

        // Worktrees - still direct (for rich wt data)
        let wt_fut = fetch_worktrees(repo_root);

        // Change requests through registry
        let cr_fut = async {
            if let Some(cr) = registry.code_review.values().next() {
                cr.list_change_requests(repo_root.as_path(), 20).await
            } else {
                Ok(vec![])
            }
        };

        // Issues through registry
        let issues_fut = async {
            if let Some(it) = registry.issue_trackers.values().next() {
                it.list_issues(repo_root.as_path(), 20).await
            } else {
                Ok(vec![])
            }
        };

        // Sessions through registry
        let sessions_fut = async {
            if let Some(ca) = registry.coding_agents.values().next() {
                ca.list_sessions().await
            } else {
                Ok(vec![])
            }
        };

        // Remote branches through registry
        let branches_fut = async {
            if let Some(vcs) = registry.vcs.values().next() {
                vcs.list_remote_branches(repo_root.as_path()).await
            } else {
                Ok(vec![])
            }
        };

        // Merged branches - still direct (not in trait)
        let merged_fut = fetch_merged_pr_branches(repo_root);

        // Workspaces through registry
        let ws_fut = async {
            if let Some((_, ws_mgr)) = &registry.workspace_manager {
                ws_mgr.list_workspaces().await
            } else {
                Ok(vec![])
            }
        };

        let (wt, crs, issues, sessions, branches, merged, workspaces) = tokio::join!(
            wt_fut, cr_fut, issues_fut, sessions_fut, branches_fut, merged_fut, ws_fut
        );

        self.worktrees = wt.unwrap_or_else(|e| { errors.push(format!("worktrees: {e}")); Vec::new() });
        self.change_requests = crs.unwrap_or_else(|e| { errors.push(format!("PRs: {e}")); Vec::new() });
        self.issues = issues.unwrap_or_else(|e| { errors.push(format!("issues: {e}")); Vec::new() });
        self.workspaces = workspaces.unwrap_or_else(|e| { errors.push(format!("workspaces: {e}")); Vec::new() });
        self.sessions = sessions.unwrap_or_else(|e| { errors.push(format!("sessions: {e}")); Vec::new() });
        self.remote_branches = branches.unwrap_or_else(|e| { errors.push(format!("branches: {e}")); Vec::new() });
        self.merged_branches = merged.unwrap_or_else(|e| { errors.push(format!("merged: {e}")); Vec::new() });
        self.correlate();
        self.loading = false;
        errors
    }

    fn find_workspaces_for_worktree(&self, wt: &Worktree) -> Vec<String> {
        self.workspaces.iter().filter(|ws| {
            ws.directories.iter().any(|dir| dir == &wt.path)
        }).map(|ws| ws.ws_ref.clone()).collect()
    }

    fn correlate(&mut self) {
        let mut items_by_branch: HashMap<String, WorkItem> = HashMap::new();
        let mut branchless_sessions: Vec<WorkItem> = Vec::new();

        // 1. Insert worktrees (primary items)
        for (i, wt) in self.worktrees.iter().enumerate() {
            let branch = wt.branch.clone();
            let ws_refs = self.find_workspaces_for_worktree(wt);
            let item = WorkItem {
                kind: WorkItemKind::Worktree,
                branch: Some(branch.clone()),
                description: branch.clone(),
                worktree_idx: Some(i),
                is_main_worktree: wt.is_main,
                pr_idx: None,
                session_idx: None,
                issue_idxs: Vec::new(),
                workspace_refs: ws_refs,
            };
            items_by_branch.insert(branch, item);
        }

        // 2. Augment with change requests (or create new items)
        for (i, cr) in self.change_requests.iter().enumerate() {
            let branch = cr.branch.clone();
            if let Some(item) = items_by_branch.get_mut(&branch) {
                item.pr_idx = Some(i);
            } else {
                let item = WorkItem {
                    kind: WorkItemKind::Pr,
                    branch: Some(branch.clone()),
                    description: cr.title.clone(),
                    worktree_idx: None,
                    is_main_worktree: false,
                    pr_idx: Some(i),
                    session_idx: None,
                    issue_idxs: Vec::new(),
                    workspace_refs: Vec::new(),
                };
                items_by_branch.insert(branch, item);
            }
        }

        // 3. Augment with sessions
        for (i, session) in self.sessions.iter().enumerate() {
            if let Some(branch) = session_branch(session) {
                if let Some(item) = items_by_branch.get_mut(&branch) {
                    item.session_idx = Some(i);
                } else {
                    let item = WorkItem {
                        kind: WorkItemKind::Session,
                        branch: Some(branch.clone()),
                        description: session.title.clone(),
                        worktree_idx: None,
                        is_main_worktree: false,
                        pr_idx: None,
                        session_idx: Some(i),
                        issue_idxs: Vec::new(),
                        workspace_refs: Vec::new(),
                    };
                    items_by_branch.insert(branch, item);
                }
            } else {
                branchless_sessions.push(WorkItem {
                    kind: WorkItemKind::Session,
                    branch: None,
                    description: session.title.clone(),
                    worktree_idx: None,
                    is_main_worktree: false,
                    pr_idx: None,
                    session_idx: Some(i),
                    issue_idxs: Vec::new(),
                    workspace_refs: Vec::new(),
                });
            }
        }

        // 4. Link issues to change requests using correlation keys
        let mut linked_issues: std::collections::HashSet<String> = std::collections::HashSet::new();
        for cr in &self.change_requests {
            for key in &cr.correlation_keys {
                if let CorrelationKey::IssueRef(_, issue_id) = key {
                    linked_issues.insert(issue_id.clone());
                    // Find which branch this CR belongs to and link the issue
                    if let Some(item) = items_by_branch.get_mut(&cr.branch) {
                        if let Some(issue_idx) = self.issues.iter().position(|i| i.id == *issue_id) {
                            if !item.issue_idxs.contains(&issue_idx) {
                                item.issue_idxs.push(issue_idx);
                            }
                        }
                    }
                }
            }
        }

        // 5. Build table entries in section order
        let mut entries: Vec<TableEntry> = Vec::new();
        let mut selectable: Vec<usize> = Vec::new();

        // Worktrees section -- sorted by branch name
        let mut wt_items: Vec<WorkItem> = items_by_branch.values()
            .filter(|item| item.kind == WorkItemKind::Worktree)
            .cloned()
            .collect();
        wt_items.sort_by(|a, b| a.branch.cmp(&b.branch));
        if !wt_items.is_empty() {
            entries.push(TableEntry::Header(SectionHeader::Worktrees));
            for item in wt_items {
                selectable.push(entries.len());
                entries.push(TableEntry::Item(item));
            }
        }

        // Sessions section -- sorted by updated_at descending (most recent first)
        let mut session_items: Vec<WorkItem> = items_by_branch.values()
            .filter(|item| item.kind == WorkItemKind::Session)
            .cloned()
            .chain(branchless_sessions)
            .collect();
        session_items.sort_by(|a, b| {
            let a_time = a.session_idx.and_then(|i| self.sessions.get(i)).and_then(|s| s.updated_at.as_deref());
            let b_time = b.session_idx.and_then(|i| self.sessions.get(i)).and_then(|s| s.updated_at.as_deref());
            b_time.cmp(&a_time) // descending
        });
        if !session_items.is_empty() {
            entries.push(TableEntry::Header(SectionHeader::Sessions));
            for item in session_items {
                selectable.push(entries.len());
                entries.push(TableEntry::Item(item));
            }
        }

        // PRs section -- sorted by id descending (most recent first)
        let mut pr_items: Vec<WorkItem> = items_by_branch.values()
            .filter(|item| item.kind == WorkItemKind::Pr)
            .cloned()
            .collect();
        pr_items.sort_by(|a, b| {
            let a_num = a.pr_idx.and_then(|i| self.change_requests.get(i)).and_then(|cr| cr.id.parse::<i64>().ok());
            let b_num = b.pr_idx.and_then(|i| self.change_requests.get(i)).and_then(|cr| cr.id.parse::<i64>().ok());
            b_num.cmp(&a_num) // descending
        });
        if !pr_items.is_empty() {
            entries.push(TableEntry::Header(SectionHeader::PullRequests));
            for item in pr_items {
                selectable.push(entries.len());
                entries.push(TableEntry::Item(item));
            }
        }

        // Remote branches section -- sorted by branch name
        let known_branches: std::collections::HashSet<&str> = items_by_branch.keys()
            .map(|s| s.as_str())
            .collect();
        let merged_set: std::collections::HashSet<&str> = self.merged_branches.iter()
            .map(|s| s.as_str())
            .collect();
        let mut remote_items: Vec<WorkItem> = self.remote_branches.iter()
            .filter(|b| {
                b.as_str() != "HEAD" && b.as_str() != "main" && b.as_str() != "master"
                    && !known_branches.contains(b.as_str())
                    && !merged_set.contains(b.as_str())
            })
            .map(|b| {
                WorkItem {
                    kind: WorkItemKind::RemoteBranch,
                    branch: Some(b.clone()),
                    description: b.clone(),
                    worktree_idx: None,
                    is_main_worktree: false,
                    pr_idx: None,
                    session_idx: None,
                    issue_idxs: Vec::new(),
                    workspace_refs: Vec::new(),
                }
            })
            .collect();
        remote_items.sort_by(|a, b| a.branch.cmp(&b.branch));
        if !remote_items.is_empty() {
            entries.push(TableEntry::Header(SectionHeader::RemoteBranches));
            for item in remote_items {
                selectable.push(entries.len());
                entries.push(TableEntry::Item(item));
            }
        }

        // Issues section -- sorted by issue id descending (most recent first)
        let mut issue_items: Vec<WorkItem> = self.issues.iter()
            .enumerate()
            .filter(|(_, issue)| !linked_issues.contains(&issue.id))
            .map(|(i, issue)| WorkItem {
                kind: WorkItemKind::Issue,
                branch: None,
                description: issue.title.clone(),
                worktree_idx: None,
                is_main_worktree: false,
                pr_idx: None,
                session_idx: None,
                issue_idxs: vec![i],
                workspace_refs: Vec::new(),
            })
            .collect();
        issue_items.sort_by(|a, b| {
            let a_num = a.issue_idxs.first().and_then(|&i| self.issues.get(i)).and_then(|iss| iss.id.parse::<i64>().ok());
            let b_num = b.issue_idxs.first().and_then(|&i| self.issues.get(i)).and_then(|iss| iss.id.parse::<i64>().ok());
            b_num.cmp(&a_num) // descending
        });
        if !issue_items.is_empty() {
            entries.push(TableEntry::Header(SectionHeader::Issues));
            for item in issue_items {
                selectable.push(entries.len());
                entries.push(TableEntry::Item(item));
            }
        }

        self.table_entries = entries;
        self.selectable_indices = selectable;
    }
}

/// Extract branch from a session's correlation keys.
fn session_branch(session: &CloudAgentSession) -> Option<String> {
    session.correlation_keys.iter().find_map(|key| {
        if let CorrelationKey::Branch(ref b) = key {
            Some(b.clone())
        } else {
            None
        }
    })
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
    // wt may append ANSI escape codes after the JSON; strip them
    let json_end = output.rfind(']').map(|i| i + 1).unwrap_or(output.len());
    serde_json::from_str(&output[..json_end]).map_err(|e| e.to_string())
}

async fn fetch_merged_pr_branches(repo_root: &PathBuf) -> Result<Vec<String>, String> {
    let output = run_command(
        "gh",
        &["pr", "list", "--state", "merged", "--limit", "50", "--json", "headRefName"],
        Some(repo_root),
    ).await?;
    let prs: Vec<serde_json::Value> = serde_json::from_str(&output).map_err(|e| e.to_string())?;
    Ok(prs
        .iter()
        .filter_map(|p| p.get("headRefName").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect())
}

#[derive(Debug, Clone, Default)]
pub struct DeleteConfirmInfo {
    pub branch: String,
    pub pr_status: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub unpushed_commits: Vec<String>,
    pub has_uncommitted: bool,
}

pub async fn fetch_delete_confirm_info(
    branch: &str,
    worktree_path: Option<&PathBuf>,
    pr_number: Option<&str>,
    repo_root: &PathBuf,
) -> DeleteConfirmInfo {
    let branch_owned = branch.to_string();
    let repo = repo_root.clone();
    let wt_path = worktree_path.cloned();
    let pr_num = pr_number.map(|s| s.to_string());
    let repo2 = repo_root.clone();

    let (unpushed, uncommitted, pr_info) = tokio::join!(
        async {
            run_command(
                "git",
                &["log", &format!("origin/main..{}", branch_owned), "--oneline"],
                Some(&repo),
            ).await.unwrap_or_default()
        },
        async {
            if let Some(path) = &wt_path {
                run_command("git", &["status", "--porcelain"], Some(path))
                    .await.unwrap_or_default()
            } else {
                String::new()
            }
        },
        async {
            if let Some(ref num) = pr_num {
                run_command(
                    "gh",
                    &["pr", "view", num, "--json", "state,mergeCommit"],
                    Some(&repo2),
                ).await.ok()
            } else {
                None
            }
        },
    );

    let mut info = DeleteConfirmInfo {
        branch: branch.to_string(),
        ..Default::default()
    };

    info.unpushed_commits = unpushed
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect();

    info.has_uncommitted = !uncommitted.trim().is_empty();

    if let Some(pr_json) = pr_info {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&pr_json) {
            info.pr_status = v.get("state").and_then(|s| s.as_str()).map(|s| s.to_string());
            info.merge_commit_sha = v.get("mergeCommit")
                .and_then(|mc| mc.get("oid"))
                .and_then(|s| s.as_str())
                .map(|s| s[..7.min(s.len())].to_string());
        }
    }

    info
}
