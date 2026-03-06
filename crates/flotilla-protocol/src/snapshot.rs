use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Repo info for list_repos response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub path: PathBuf,
    pub name: String,
    pub provider_health: HashMap<String, bool>,
    pub loading: bool,
}

/// A complete snapshot for one repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub seq: u64,
    pub repo: PathBuf,
    pub work_items: Vec<ProtoWorkItem>,
    pub provider_health: HashMap<String, bool>,
    pub errors: Vec<ProtoError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoError {
    pub category: String,
    pub message: String,
}

/// Serializable work item — flattened from the core WorkItem enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoWorkItem {
    pub kind: ProtoWorkItemKind,
    pub identity: ProtoWorkItemIdentity,
    pub branch: Option<String>,
    pub description: String,
    pub checkout: Option<ProtoCheckoutRef>,
    pub pr_key: Option<String>,
    pub session_key: Option<String>,
    pub issue_keys: Vec<String>,
    pub workspace_refs: Vec<String>,
    pub is_main_worktree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProtoWorkItemKind {
    Checkout,
    Session,
    Pr,
    RemoteBranch,
    Issue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProtoWorkItemIdentity {
    Checkout(PathBuf),
    ChangeRequest(String),
    Session(String),
    Issue(String),
    RemoteBranch(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtoCheckoutRef {
    pub key: PathBuf,
    pub is_main_worktree: bool,
}
