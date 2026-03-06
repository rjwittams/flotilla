use crate::data::{WorkItem, WorkItemKind};
use super::command::Command;
use super::App;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    SwitchToWorkspace,
    CreateWorkspace,
    RemoveWorktree,
    CreateWorktreeAndWorkspace,
    GenerateBranchName,
    OpenPr,
    OpenIssue,
    TeleportSession,
    ArchiveSession,
}

impl Intent {
    pub fn label(&self, labels: &super::model::RepoLabels) -> String {
        match self {
            Intent::SwitchToWorkspace => "Switch to workspace".into(),
            Intent::CreateWorkspace => "Create workspace".into(),
            Intent::RemoveWorktree => format!("Remove {}", labels.checkouts.noun),
            Intent::CreateWorktreeAndWorkspace => format!("Create {} + workspace", labels.checkouts.noun),
            Intent::GenerateBranchName => "Generate branch name".into(),
            Intent::OpenPr => format!("Open {} in browser", labels.code_review.noun),
            Intent::OpenIssue => "Open issue in browser".into(),
            Intent::TeleportSession => "Teleport session".into(),
            Intent::ArchiveSession => "Archive session".into(),
        }
    }

    pub fn is_available(&self, item: &WorkItem) -> bool {
        match self {
            Intent::SwitchToWorkspace => !item.workspace_refs.is_empty(),
            Intent::CreateWorkspace => item.checkout_key.is_some() && item.workspace_refs.is_empty(),
            Intent::RemoveWorktree => item.checkout_key.is_some() && !item.is_main_worktree,
            Intent::CreateWorktreeAndWorkspace => item.checkout_key.is_none() && item.branch.is_some(),
            Intent::GenerateBranchName => item.branch.is_none() && !item.issue_keys.is_empty(),
            Intent::OpenPr => item.pr_key.is_some(),
            Intent::OpenIssue => !item.issue_keys.is_empty(),
            Intent::TeleportSession => item.session_key.is_some(),
            Intent::ArchiveSession => item.session_key.is_some(),
        }
    }

    pub fn shortcut_hint(&self, labels: &super::model::RepoLabels) -> Option<String> {
        match self {
            Intent::RemoveWorktree => Some(format!("d:remove {}", labels.checkouts.noun)),
            Intent::OpenPr => Some(format!("p:show {}", labels.code_review.abbr)),
            _ => None,
        }
    }

    /// Resolve an intent into a concrete command, given the current item and app state.
    /// Returns None if the intent can't be resolved (missing data).
    pub fn resolve(&self, item: &WorkItem, app: &App) -> Option<Command> {
        match self {
            Intent::SwitchToWorkspace => {
                item.workspace_refs.first().map(|ws_ref| Command::SelectWorkspace(ws_ref.clone()))
            }
            Intent::CreateWorkspace => {
                item.checkout_key.clone().map(Command::SwitchWorktree)
            }
            Intent::RemoveWorktree => {
                if item.kind != WorkItemKind::Checkout || item.is_main_worktree {
                    return None;
                }
                app.active_ui().selected_selectable_idx.map(Command::FetchDeleteInfo)
            }
            Intent::CreateWorktreeAndWorkspace => {
                item.branch.as_ref().map(|branch| Command::CreateWorktree {
                    branch: branch.clone(),
                    create_branch: item.kind != WorkItemKind::RemoteBranch && item.kind != WorkItemKind::Pr,
                })
            }
            Intent::GenerateBranchName => {
                if !item.issue_keys.is_empty() {
                    Some(Command::GenerateBranchName(item.issue_keys.clone()))
                } else {
                    None
                }
            }
            Intent::OpenPr => {
                item.pr_key.as_ref().map(|k| Command::OpenPr(k.clone()))
            }
            Intent::OpenIssue => {
                item.issue_keys.first().map(|k| Command::OpenIssueBrowser(k.clone()))
            }
            Intent::TeleportSession => {
                item.session_key.as_ref().map(|k| {
                    Command::TeleportSession {
                        session_id: k.clone(),
                        branch: item.branch.clone(),
                        checkout_key: item.checkout_key.clone(),
                    }
                })
            }
            Intent::ArchiveSession => {
                item.session_key.clone().map(Command::ArchiveSession)
            }
        }
    }

    pub fn all_in_menu_order() -> &'static [Intent] {
        &[
            Intent::SwitchToWorkspace,
            Intent::CreateWorkspace,
            Intent::RemoveWorktree,
            Intent::CreateWorktreeAndWorkspace,
            Intent::GenerateBranchName,
            Intent::OpenPr,
            Intent::OpenIssue,
            Intent::TeleportSession,
            Intent::ArchiveSession,
        ]
    }

    pub fn enter_priority() -> &'static [Intent] {
        &[
            Intent::SwitchToWorkspace,
            Intent::TeleportSession,
            Intent::CreateWorkspace,
            Intent::CreateWorktreeAndWorkspace,
            Intent::GenerateBranchName,
        ]
    }
}
