use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

use crate::data::DataStore;
use std::path::PathBuf;

#[derive(Default)]
pub enum PendingAction {
    #[default]
    None,
    SwitchWorktree(usize),
    CreateWorktree(String),
    RemoveWorktree(usize),
    OpenPr(i64),
    Refresh,
}

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter, PartialEq)]
pub enum Tab {
    #[default]
    #[strum(to_string = "Worktrees")]
    Worktrees,
    #[strum(to_string = "PRs")]
    Prs,
    #[strum(to_string = "Issues")]
    Issues,
    #[strum(to_string = "Sessions")]
    Sessions,
}

impl Tab {
    pub fn next(self) -> Self {
        let i = (self as usize + 1) % Self::iter().count();
        Self::from_repr(i).unwrap_or(self)
    }
    pub fn prev(self) -> Self {
        let count = Self::iter().count();
        let i = (self as usize + count - 1) % count;
        Self::from_repr(i).unwrap_or(self)
    }
}

#[derive(Default)]
pub struct App {
    pub should_quit: bool,
    pub current_tab: Tab,
    pub data: DataStore,
    pub repo_root: PathBuf,
    pub list_state: ListState,
    pub pending_action: PendingAction,
    pub show_action_menu: bool,
    pub action_menu_items: Vec<String>,
    pub action_menu_index: usize,
}

impl App {
    pub fn new(repo_root: PathBuf) -> Self {
        Self {
            repo_root,
            ..Default::default()
        }
    }

    pub async fn refresh_data(&mut self) {
        self.data.refresh(&self.repo_root).await;
        if self.list_state.selected().is_none() && self.current_list_len() > 0 {
            self.list_state.select(Some(0));
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.show_action_menu {
            self.handle_menu_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.current_tab = self.current_tab.next();
                self.list_state.select(Some(0));
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.prev();
                self.list_state.select(Some(0));
            }
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
            KeyCode::Char('r') => {} // refresh handled in main loop
            KeyCode::Char(' ') => self.open_action_menu(),
            KeyCode::Enter => {
                if let Some(i) = self.list_state.selected() {
                    match self.current_tab {
                        Tab::Worktrees => self.pending_action = PendingAction::SwitchWorktree(i),
                        Tab::Prs => {
                            if let Some(pr) = self.data.prs.get(i) {
                                self.pending_action = PendingAction::OpenPr(pr.number);
                            }
                        }
                        _ => {}
                    }
                }
            }
            KeyCode::Char('d') if self.current_tab == Tab::Worktrees => {
                if let Some(i) = self.list_state.selected() {
                    self.pending_action = PendingAction::RemoveWorktree(i);
                }
            }
            KeyCode::Char('p') => {
                if let Some(i) = self.list_state.selected() {
                    match self.current_tab {
                        Tab::Worktrees => {
                            // Find PR for this worktree's branch
                            if let Some(wt) = self.data.worktrees.get(i) {
                                if let Some(pr) = self.data.prs.iter().find(|pr| pr.head_ref_name == wt.branch) {
                                    self.pending_action = PendingAction::OpenPr(pr.number);
                                }
                            }
                        }
                        Tab::Prs => {
                            if let Some(pr) = self.data.prs.get(i) {
                                self.pending_action = PendingAction::OpenPr(pr.number);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn open_action_menu(&mut self) {
        let Some(i) = self.list_state.selected() else {
            return;
        };

        let items = match self.current_tab {
            Tab::Worktrees => {
                if let Some(wt) = self.data.worktrees.get(i) {
                    let has_workspace = self.data.cmux_workspaces.iter().any(|ws| {
                        wt.path.to_string_lossy().contains(ws) || ws.contains(&wt.branch)
                    });
                    if has_workspace {
                        vec![
                            "Switch".to_string(),
                            "Remove".to_string(),
                            "View diff".to_string(),
                            "Open PR".to_string(),
                            "Close workspace".to_string(),
                        ]
                    } else {
                        vec![
                            "Create workspace".to_string(),
                            "Remove".to_string(),
                            "View diff".to_string(),
                        ]
                    }
                } else {
                    return;
                }
            }
            Tab::Prs => vec![
                "Checkout & create workspace".to_string(),
                "View in browser".to_string(),
            ],
            Tab::Issues => vec![
                "Create branch & workspace".to_string(),
                "View in browser".to_string(),
            ],
            Tab::Sessions => return,
        };

        self.action_menu_items = items;
        self.action_menu_index = 0;
        self.show_action_menu = true;
    }

    fn handle_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.show_action_menu = false,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.action_menu_index < self.action_menu_items.len().saturating_sub(1) {
                    self.action_menu_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.action_menu_index = self.action_menu_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.execute_menu_action();
                self.show_action_menu = false;
            }
            _ => {}
        }
    }

    fn execute_menu_action(&mut self) {
        let Some(selected_item) = self.action_menu_items.get(self.action_menu_index) else {
            return;
        };
        let Some(list_index) = self.list_state.selected() else {
            return;
        };

        match selected_item.as_str() {
            "Switch" | "Create workspace" => {
                self.pending_action = PendingAction::SwitchWorktree(list_index);
            }
            "Remove" => {
                self.pending_action = PendingAction::RemoveWorktree(list_index);
            }
            "Open PR" | "View in browser" => {
                match self.current_tab {
                    Tab::Worktrees => {
                        if let Some(wt) = self.data.worktrees.get(list_index) {
                            if let Some(pr) = self.data.prs.iter().find(|pr| pr.head_ref_name == wt.branch) {
                                self.pending_action = PendingAction::OpenPr(pr.number);
                            }
                        }
                    }
                    Tab::Prs => {
                        if let Some(pr) = self.data.prs.get(list_index) {
                            self.pending_action = PendingAction::OpenPr(pr.number);
                        }
                    }
                    Tab::Issues => {
                        if let Some(issue) = self.data.issues.get(list_index) {
                            self.pending_action = PendingAction::OpenPr(issue.number);
                        }
                    }
                    _ => {}
                }
            }
            "Checkout & create workspace" => {
                if let Some(pr) = self.data.prs.get(list_index) {
                    self.pending_action = PendingAction::CreateWorktree(pr.head_ref_name.clone());
                }
            }
            "Create branch & workspace" => {
                if let Some(issue) = self.data.issues.get(list_index) {
                    let branch_name = format!("issue-{}", issue.number);
                    self.pending_action = PendingAction::CreateWorktree(branch_name);
                }
            }
            // "View diff", "Close workspace" => no-op for now
            _ => {}
        }
    }

    pub fn take_pending_action(&mut self) -> PendingAction {
        std::mem::take(&mut self.pending_action)
    }

    pub fn tick(&mut self) {
        // Will be used for auto-refresh
    }

    fn select_next(&mut self) {
        if self.current_list_len() > 0 {
            self.list_state.select_next();
        }
    }

    fn select_prev(&mut self) {
        if self.current_list_len() > 0 {
            self.list_state.select_previous();
        }
    }

    fn current_list_len(&self) -> usize {
        match self.current_tab {
            Tab::Worktrees => self.data.worktrees.len(),
            Tab::Prs => self.data.prs.len(),
            Tab::Issues => self.data.issues.len(),
            Tab::Sessions => 0,
        }
    }
}
