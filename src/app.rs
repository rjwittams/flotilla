use crossterm::event::{KeyCode, KeyEvent};
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

use crate::data::DataStore;
use std::path::PathBuf;

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
    pub list_index: usize,
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
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => {
                self.current_tab = self.current_tab.next();
                self.list_index = 0;
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.prev();
                self.list_index = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
            KeyCode::Char('r') => {} // refresh handled in main loop
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        // Will be used for auto-refresh
    }

    fn select_next(&mut self) {
        let len = self.current_list_len();
        if len > 0 {
            self.list_index = (self.list_index + 1).min(len - 1);
        }
    }

    fn select_prev(&mut self) {
        self.list_index = self.list_index.saturating_sub(1);
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
