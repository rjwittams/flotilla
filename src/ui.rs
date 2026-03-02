use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    widgets::{Block, Paragraph, Tabs},
    Frame,
};
use strum::IntoEnumIterator;

use crate::app::App;
use crate::app::Tab;

pub fn render(app: &App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_tabs(app, frame, chunks[0]);
    render_content(app, frame, chunks[1]);
    render_status_bar(frame, chunks[2]);
}

fn render_tabs(app: &App, frame: &mut Frame, area: ratatui::layout::Rect) {
    let titles = Tab::iter().map(|t| t.to_string());
    let tabs = Tabs::new(titles)
        .select(app.current_tab as usize)
        .highlight_style(Style::default().bold().fg(Color::Cyan))
        .divider(" | ")
        .block(Block::bordered().title(" cmux-controller "));
    frame.render_widget(tabs, area);
}

fn render_content(app: &App, frame: &mut Frame, area: ratatui::layout::Rect) {
    let content = Paragraph::new(format!("Tab: {}", app.current_tab))
        .block(Block::bordered());
    frame.render_widget(content, area);
}

fn render_status_bar(frame: &mut Frame, area: ratatui::layout::Rect) {
    let status = Paragraph::new(" tab:switch  enter:select  space:menu  q:quit")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, area);
}
