use crossterm::event::{KeyCode, KeyEvent};
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

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
}

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => self.current_tab = self.current_tab.next(),
            KeyCode::BackTab => self.current_tab = self.current_tab.prev(),
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        // Will be used for auto-refresh
    }
}
