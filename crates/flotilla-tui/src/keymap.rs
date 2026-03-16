// Keymap module: configurable key bindings for the TUI.

use crate::app::intent::Intent;

/// An action that can be triggered by a key binding.
///
/// Most variants correspond to UI-level operations (navigation, mode transitions).
/// `Dispatch(Intent)` wraps an `Intent` for actions that go through the executor pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SelectNext,
    SelectPrev,
    Confirm,
    Dismiss,
    Quit,
    Refresh,
    PrevTab,
    NextTab,
    MoveTabLeft,
    MoveTabRight,
    ToggleHelp,
    ToggleMultiSelect,
    ToggleProviders,
    ToggleDebug,
    ToggleStatusBarKeys,
    CycleHost,
    CycleLayout,
    CycleTheme,
    OpenActionMenu,
    OpenBranchInput,
    OpenIssueSearch,
    OpenFilePicker,
    Dispatch(Intent),
}

impl Action {
    /// Parse an action from its snake_case config string representation.
    ///
    /// Intent-wrapping actions use the intent name directly (e.g. "remove_checkout"
    /// maps to `Action::Dispatch(Intent::RemoveCheckout)`).
    pub fn from_config_str(s: &str) -> Option<Action> {
        let action = match s {
            "select_next" => Action::SelectNext,
            "select_prev" => Action::SelectPrev,
            "confirm" => Action::Confirm,
            "dismiss" => Action::Dismiss,
            "quit" => Action::Quit,
            "refresh" => Action::Refresh,
            "prev_tab" => Action::PrevTab,
            "next_tab" => Action::NextTab,
            "move_tab_left" => Action::MoveTabLeft,
            "move_tab_right" => Action::MoveTabRight,
            "toggle_help" => Action::ToggleHelp,
            "toggle_multi_select" => Action::ToggleMultiSelect,
            "toggle_providers" => Action::ToggleProviders,
            "toggle_debug" => Action::ToggleDebug,
            "toggle_status_bar_keys" => Action::ToggleStatusBarKeys,
            "cycle_host" => Action::CycleHost,
            "cycle_layout" => Action::CycleLayout,
            "cycle_theme" => Action::CycleTheme,
            "open_action_menu" => Action::OpenActionMenu,
            "open_branch_input" => Action::OpenBranchInput,
            "open_issue_search" => Action::OpenIssueSearch,
            "open_file_picker" => Action::OpenFilePicker,
            // Intent-wrapping actions
            "switch_to_workspace" => Action::Dispatch(Intent::SwitchToWorkspace),
            "create_workspace" => Action::Dispatch(Intent::CreateWorkspace),
            "remove_checkout" => Action::Dispatch(Intent::RemoveCheckout),
            "create_checkout" => Action::Dispatch(Intent::CreateCheckout),
            "generate_branch_name" => Action::Dispatch(Intent::GenerateBranchName),
            "open_change_request" => Action::Dispatch(Intent::OpenChangeRequest),
            "open_issue" => Action::Dispatch(Intent::OpenIssue),
            "link_issues_to_change_request" => Action::Dispatch(Intent::LinkIssuesToChangeRequest),
            "teleport_session" => Action::Dispatch(Intent::TeleportSession),
            "archive_session" => Action::Dispatch(Intent::ArchiveSession),
            "close_change_request" => Action::Dispatch(Intent::CloseChangeRequest),
            _ => return None,
        };
        Some(action)
    }

    /// Convert the action to its snake_case config string representation.
    ///
    /// This is the inverse of `from_config_str`.
    pub fn as_config_str(&self) -> &'static str {
        match self {
            Action::SelectNext => "select_next",
            Action::SelectPrev => "select_prev",
            Action::Confirm => "confirm",
            Action::Dismiss => "dismiss",
            Action::Quit => "quit",
            Action::Refresh => "refresh",
            Action::PrevTab => "prev_tab",
            Action::NextTab => "next_tab",
            Action::MoveTabLeft => "move_tab_left",
            Action::MoveTabRight => "move_tab_right",
            Action::ToggleHelp => "toggle_help",
            Action::ToggleMultiSelect => "toggle_multi_select",
            Action::ToggleProviders => "toggle_providers",
            Action::ToggleDebug => "toggle_debug",
            Action::ToggleStatusBarKeys => "toggle_status_bar_keys",
            Action::CycleHost => "cycle_host",
            Action::CycleLayout => "cycle_layout",
            Action::CycleTheme => "cycle_theme",
            Action::OpenActionMenu => "open_action_menu",
            Action::OpenBranchInput => "open_branch_input",
            Action::OpenIssueSearch => "open_issue_search",
            Action::OpenFilePicker => "open_file_picker",
            Action::Dispatch(intent) => match intent {
                Intent::SwitchToWorkspace => "switch_to_workspace",
                Intent::CreateWorkspace => "create_workspace",
                Intent::RemoveCheckout => "remove_checkout",
                Intent::CreateCheckout => "create_checkout",
                Intent::GenerateBranchName => "generate_branch_name",
                Intent::OpenChangeRequest => "open_change_request",
                Intent::OpenIssue => "open_issue",
                Intent::LinkIssuesToChangeRequest => "link_issues_to_change_request",
                Intent::TeleportSession => "teleport_session",
                Intent::ArchiveSession => "archive_session",
                Intent::CloseChangeRequest => "close_change_request",
            },
        }
    }

    /// Human-readable description of the action, suitable for help screen display.
    pub fn description(&self) -> &'static str {
        match self {
            Action::SelectNext => "Move selection down",
            Action::SelectPrev => "Move selection up",
            Action::Confirm => "Confirm / execute",
            Action::Dismiss => "Dismiss / go back",
            Action::Quit => "Quit the application",
            Action::Refresh => "Refresh all providers",
            Action::PrevTab => "Switch to previous tab",
            Action::NextTab => "Switch to next tab",
            Action::MoveTabLeft => "Move current tab left",
            Action::MoveTabRight => "Move current tab right",
            Action::ToggleHelp => "Toggle help screen",
            Action::ToggleMultiSelect => "Toggle multi-select",
            Action::ToggleProviders => "Toggle provider config",
            Action::ToggleDebug => "Toggle debug panel",
            Action::ToggleStatusBarKeys => "Toggle status bar key hints",
            Action::CycleHost => "Cycle host filter",
            Action::CycleLayout => "Cycle layout",
            Action::CycleTheme => "Cycle colour theme",
            Action::OpenActionMenu => "Open action menu",
            Action::OpenBranchInput => "New branch input",
            Action::OpenIssueSearch => "Search issues",
            Action::OpenFilePicker => "Open file picker",
            Action::Dispatch(intent) => match intent {
                Intent::SwitchToWorkspace => "Switch to workspace",
                Intent::CreateWorkspace => "Create workspace",
                Intent::RemoveCheckout => "Remove checkout",
                Intent::CreateCheckout => "Create checkout",
                Intent::GenerateBranchName => "Generate branch name",
                Intent::OpenChangeRequest => "Open change request in browser",
                Intent::OpenIssue => "Open issue in browser",
                Intent::LinkIssuesToChangeRequest => "Link issues to change request",
                Intent::TeleportSession => "Teleport session",
                Intent::ArchiveSession => "Archive session",
                Intent::CloseChangeRequest => "Close change request",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every non-Dispatch action round-trips through config strings.
    #[test]
    fn round_trip_non_dispatch_actions() {
        let actions = [
            Action::SelectNext,
            Action::SelectPrev,
            Action::Confirm,
            Action::Dismiss,
            Action::Quit,
            Action::Refresh,
            Action::PrevTab,
            Action::NextTab,
            Action::MoveTabLeft,
            Action::MoveTabRight,
            Action::ToggleHelp,
            Action::ToggleMultiSelect,
            Action::ToggleProviders,
            Action::ToggleDebug,
            Action::ToggleStatusBarKeys,
            Action::CycleHost,
            Action::CycleLayout,
            Action::CycleTheme,
            Action::OpenActionMenu,
            Action::OpenBranchInput,
            Action::OpenIssueSearch,
            Action::OpenFilePicker,
        ];
        for action in actions {
            let s = action.as_config_str();
            let parsed = Action::from_config_str(s).unwrap_or_else(|| panic!("failed to parse config str: {s}"));
            assert_eq!(parsed, action, "round-trip failed for {s}");
        }
    }

    /// Every Intent variant round-trips through Dispatch config strings.
    #[test]
    fn round_trip_dispatch_actions() {
        let intents = [
            Intent::SwitchToWorkspace,
            Intent::CreateWorkspace,
            Intent::RemoveCheckout,
            Intent::CreateCheckout,
            Intent::GenerateBranchName,
            Intent::OpenChangeRequest,
            Intent::OpenIssue,
            Intent::LinkIssuesToChangeRequest,
            Intent::TeleportSession,
            Intent::ArchiveSession,
            Intent::CloseChangeRequest,
        ];
        for intent in intents {
            let action = Action::Dispatch(intent);
            let s = action.as_config_str();
            let parsed = Action::from_config_str(s).unwrap_or_else(|| panic!("failed to parse config str: {s}"));
            assert_eq!(parsed, action, "round-trip failed for {s}");
        }
    }

    /// Unknown strings return None.
    #[test]
    fn unknown_string_returns_none() {
        assert_eq!(Action::from_config_str("nonexistent_action"), None);
        assert_eq!(Action::from_config_str(""), None);
        assert_eq!(Action::from_config_str("SelectNext"), None); // wrong case
    }

    /// Every action has a non-empty description.
    #[test]
    fn all_actions_have_descriptions() {
        let all_actions: Vec<Action> = vec![
            Action::SelectNext,
            Action::SelectPrev,
            Action::Confirm,
            Action::Dismiss,
            Action::Quit,
            Action::Refresh,
            Action::PrevTab,
            Action::NextTab,
            Action::MoveTabLeft,
            Action::MoveTabRight,
            Action::ToggleHelp,
            Action::ToggleMultiSelect,
            Action::ToggleProviders,
            Action::ToggleDebug,
            Action::ToggleStatusBarKeys,
            Action::CycleHost,
            Action::CycleLayout,
            Action::CycleTheme,
            Action::OpenActionMenu,
            Action::OpenBranchInput,
            Action::OpenIssueSearch,
            Action::OpenFilePicker,
            Action::Dispatch(Intent::SwitchToWorkspace),
            Action::Dispatch(Intent::CreateWorkspace),
            Action::Dispatch(Intent::RemoveCheckout),
            Action::Dispatch(Intent::CreateCheckout),
            Action::Dispatch(Intent::GenerateBranchName),
            Action::Dispatch(Intent::OpenChangeRequest),
            Action::Dispatch(Intent::OpenIssue),
            Action::Dispatch(Intent::LinkIssuesToChangeRequest),
            Action::Dispatch(Intent::TeleportSession),
            Action::Dispatch(Intent::ArchiveSession),
            Action::Dispatch(Intent::CloseChangeRequest),
        ];
        for action in all_actions {
            let desc = action.description();
            assert!(!desc.is_empty(), "empty description for {:?}", action);
        }
    }
}
