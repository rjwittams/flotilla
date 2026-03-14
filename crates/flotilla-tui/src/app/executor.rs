use flotilla_protocol::{Command, CommandAction, CommandResult};
use tracing::info;

use super::{ui_state::UiMode, App};

/// Dispatch a single protocol command through the daemon.
///
/// Most commands go through the shared `execute(command)` path and return a
/// command ID immediately. Issue fetch/search commands are spawned in the
/// background because they may do network I/O inline before returning.
pub async fn dispatch(cmd: Command, app: &mut App) {
    app.model.status_message = None;

    let background_issue_command = matches!(
        cmd.action,
        CommandAction::SetIssueViewport { .. }
            | CommandAction::FetchMoreIssues { .. }
            | CommandAction::SearchIssues { .. }
            | CommandAction::ClearIssueSearch { .. }
    );

    if background_issue_command {
        let daemon = app.daemon.clone();
        tokio::spawn(async move {
            let _ = daemon.execute(cmd).await;
        });
        return;
    }

    match app.daemon.execute(cmd).await {
        Ok(_command_id) => {}
        Err(e) => app.model.status_message = Some(e),
    }
}

/// Interpret a CommandResult into UI state changes.
///
/// Called when a `CommandFinished` event arrives from the daemon.
pub fn handle_result(result: CommandResult, app: &mut App) {
    match result {
        CommandResult::Ok => {}
        CommandResult::RepoAdded { path } => {
            info!(path = %path.display(), "added repo");
        }
        CommandResult::RepoRemoved { path } => {
            info!(path = %path.display(), "removed repo");
        }
        CommandResult::Refreshed { repos } => {
            info!(count = repos.len(), "refresh completed");
        }
        CommandResult::CheckoutCreated { branch, .. } => {
            info!(%branch, "created checkout");
        }
        CommandResult::CheckoutRemoved { branch } => {
            info!(%branch, "removed checkout");
        }
        CommandResult::TerminalPrepared { target_host, branch, checkout_path, commands } => {
            app.proto_commands.push(app.repo_command(CommandAction::CreateWorkspaceFromPreparedTerminal {
                target_host,
                branch,
                checkout_path,
                commands,
            }));
        }
        CommandResult::BranchNameGenerated { name, issue_ids } => {
            app.prefill_branch_input(&name, issue_ids);
        }
        CommandResult::CheckoutStatus(info) => {
            let terminal_keys = match &app.ui.mode {
                UiMode::DeleteConfirm { terminal_keys, .. } => terminal_keys.clone(),
                _ => vec![],
            };
            app.ui.mode = UiMode::DeleteConfirm { info: Some(info), loading: false, terminal_keys };
        }
        CommandResult::Error { message } => {
            app.model.status_message = Some(message);
        }
        CommandResult::Cancelled => {
            app.model.status_message = Some("Command cancelled".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flotilla_protocol::{CommandAction, HostName, PreparedTerminalCommand};

    use super::*;
    use crate::app::test_support::stub_app;

    #[test]
    fn terminal_prepared_queues_local_workspace_creation() {
        let mut app = stub_app();

        handle_result(
            CommandResult::TerminalPrepared {
                target_host: HostName::new("remote-a"),
                branch: "feat-x".into(),
                checkout_path: PathBuf::from("/remote/feat-x"),
                commands: vec![PreparedTerminalCommand { role: "main".into(), command: "bash -l".into() }],
            },
            &mut app,
        );

        let cmd = app.proto_commands.take_next().expect("queued workspace creation");
        match cmd.action {
            CommandAction::CreateWorkspaceFromPreparedTerminal { target_host, branch, checkout_path, commands } => {
                assert_eq!(cmd.host, None);
                assert_eq!(target_host, HostName::new("remote-a"));
                assert_eq!(branch, "feat-x");
                assert_eq!(checkout_path, PathBuf::from("/remote/feat-x"));
                assert_eq!(commands, vec![PreparedTerminalCommand { role: "main".into(), command: "bash -l".into() }]);
            }
            other => panic!("expected CreateWorkspaceFromPreparedTerminal, got {other:?}"),
        }
    }
}
