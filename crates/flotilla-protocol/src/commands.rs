use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Commands the client can send to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    CreateWorkspaceForCheckout {
        checkout_path: PathBuf,
    },
    SelectWorkspace {
        ws_ref: String,
    },
    CreateCheckout {
        branch: String,
        create_branch: bool,
        issue_ids: Vec<(String, String)>,
    },
    RemoveCheckout {
        branch: String,
    },
    FetchCheckoutStatus {
        branch: String,
        checkout_path: Option<PathBuf>,
        change_request_id: Option<String>,
    },
    OpenChangeRequest {
        id: String,
    },
    OpenIssue {
        id: String,
    },
    LinkIssuesToChangeRequest {
        change_request_id: String,
        issue_ids: Vec<String>,
    },
    ArchiveSession {
        session_id: String,
    },
    GenerateBranchName {
        issue_keys: Vec<String>,
    },
    TeleportSession {
        session_id: String,
        branch: Option<String>,
        checkout_key: Option<PathBuf>,
    },
    AddRepo {
        path: PathBuf,
    },
    RemoveRepo {
        path: PathBuf,
    },
    Refresh,
}

/// Result returned from command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CommandResult {
    Ok,
    CheckoutCreated {
        branch: String,
    },
    BranchNameGenerated {
        name: String,
        issue_ids: Vec<(String, String)>,
    },
    CheckoutStatus(CheckoutStatus),
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CheckoutStatus {
    pub branch: String,
    pub change_request_status: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub unpushed_commits: Vec<String>,
    pub has_uncommitted: bool,
    pub base_detection_warning: Option<String>,
}
