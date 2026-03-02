use std::time::Duration;

use color_eyre::Result;
use crossterm::event::{EventStream, KeyCode, KeyEventKind};
use futures::{FutureExt, StreamExt};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, Paragraph, Tabs},
    DefaultTerminal, Frame,
};
use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter, PartialEq)]
enum Tab {
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
    fn next(self) -> Self {
        let i = (self as usize + 1) % Self::iter().count();
        Self::from_repr(i).unwrap_or(self)
    }
    fn prev(self) -> Self {
        let count = Self::iter().count();
        let i = (self as usize + count - 1) % count;
        Self::from_repr(i).unwrap_or(self)
    }
}

#[derive(Default)]
struct App {
    should_quit: bool,
    current_tab: Tab,
}

impl App {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => self.current_tab = self.current_tab.next(),
            KeyCode::BackTab => self.current_tab = self.current_tab.prev(),
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // tab bar
                Constraint::Min(0),    // main content
                Constraint::Length(1), // status bar
            ])
            .split(frame.area());

        // Tab bar
        let titles = Tab::iter().map(|t| t.to_string());
        let tabs = Tabs::new(titles)
            .select(self.current_tab as usize)
            .highlight_style(Style::default().bold().fg(Color::Cyan))
            .divider(" | ")
            .block(Block::bordered().title(" cmux-controller "));
        frame.render_widget(tabs, chunks[0]);

        // Main content (placeholder)
        let content = Paragraph::new(format!("Tab: {}", self.current_tab))
            .block(Block::bordered());
        frame.render_widget(content, chunks[1]);

        // Status bar
        let status = Paragraph::new(" tab:switch  q:quit")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(status, chunks[2]);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let result = run(&mut terminal).await;
    ratatui::restore();
    result
}

async fn run(terminal: &mut DefaultTerminal) -> Result<()> {
    let mut app = App::default();
    let mut reader = EventStream::new();
    let tick_rate = Duration::from_millis(250);
    let mut interval = tokio::time::interval(tick_rate);

    loop {
        terminal.draw(|f| app.render(f))?;

        let delay = interval.tick();
        let event = reader.next().fuse();

        tokio::select! {
            _ = delay => {}
            maybe = event => match maybe {
                Some(Ok(crossterm::event::Event::Key(k))) if k.kind == KeyEventKind::Press => {
                    app.handle_key(k);
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
