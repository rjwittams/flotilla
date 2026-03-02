use ratatui::{
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Tabs},
    Frame,
};
use strum::IntoEnumIterator;

use crate::app::App;
use crate::app::Tab;

pub fn render(app: &mut App, frame: &mut Frame) {
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
    render_action_menu(app, frame);
}

fn render_tabs(app: &App, frame: &mut Frame, area: Rect) {
    let titles = Tab::iter().map(|t| t.to_string());
    let tabs = Tabs::new(titles)
        .select(app.current_tab as usize)
        .highlight_style(Style::default().bold().fg(Color::Cyan))
        .divider(" | ")
        .block(Block::bordered().title(" cmux-controller "));
    frame.render_widget(tabs, area);
}

fn render_status_bar(frame: &mut Frame, area: Rect) {
    let status = Paragraph::new(" tab:switch  enter:select  space:menu  q:quit")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, area);
}

fn render_content(app: &mut App, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    match app.current_tab {
        Tab::Worktrees => render_worktree_list(app, frame, chunks[0]),
        Tab::Prs => render_pr_list(app, frame, chunks[0]),
        Tab::Issues => render_issue_list(app, frame, chunks[0]),
        Tab::Sessions => render_sessions(app, frame, chunks[0]),
    }

    render_preview(app, frame, chunks[1]);
}

fn render_worktree_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .data
        .worktrees
        .iter()
        .map(|wt| {
            let indicator = if app.data.cmux_workspaces.iter().any(|ws| {
                wt.path.to_string_lossy().contains(ws) || ws.contains(&wt.branch)
            }) {
                "●"
            } else {
                "○"
            };

            let ahead = wt
                .main
                .as_ref()
                .map(|m| format!("↑{}", m.ahead))
                .unwrap_or_default();
            let branch = &wt.branch;
            let modified = if wt.working_tree.as_ref().is_some_and(|w| w.modified) {
                "*"
            } else {
                ""
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {indicator} "), Style::default().fg(Color::Green)),
                Span::styled(
                    format!("{branch}{modified:<20}"),
                    Style::default().bold(),
                ),
                Span::styled(format!(" {ahead:<6}"), Style::default().fg(Color::Yellow)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title(" Worktrees "))
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ")
        .highlight_spacing(HighlightSpacing::Always);

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_pr_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .data
        .prs
        .iter()
        .map(|pr| ListItem::new(format!("  PR #{:<5} {}", pr.number, pr.title)))
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(" Pull Requests "))
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_issue_list(app: &mut App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .data
        .issues
        .iter()
        .map(|issue| {
            let labels = issue
                .labels
                .iter()
                .map(|l| l.name.clone())
                .collect::<Vec<_>>()
                .join(",");
            ListItem::new(format!(
                "  #{:<5} {} {}",
                issue.number, issue.title, labels
            ))
        })
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(" Issues "))
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_sessions(_app: &mut App, frame: &mut Frame, area: Rect) {
    let content = Paragraph::new(
        "  Web sessions not yet connected.\n  (Waiting for claude.ai/code API)",
    )
    .block(Block::bordered().title(" Sessions "))
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(content, area);
}

fn render_preview(app: &App, frame: &mut Frame, area: Rect) {
    let text = match app.current_tab {
        Tab::Worktrees => {
            if let Some(i) = app.list_state.selected() {
                if let Some(wt) = app.data.worktrees.get(i) {
                    let sha = wt
                        .commit
                        .as_ref()
                        .and_then(|c| c.short_sha.as_deref())
                        .unwrap_or("?");
                    let msg = wt
                        .commit
                        .as_ref()
                        .and_then(|c| c.message.as_deref())
                        .unwrap_or("");
                    format!(
                        "Branch: {}\nPath: {}\nCommit: {} {}",
                        wt.branch,
                        wt.path.display(),
                        sha,
                        msg
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        }
        Tab::Prs => {
            if let Some(i) = app.list_state.selected() {
                if let Some(pr) = app.data.prs.get(i) {
                    format!(
                        "PR #{}: {}\nBranch: {}\nState: {}",
                        pr.number, pr.title, pr.head_ref_name, pr.state
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        }
        Tab::Issues => {
            if let Some(i) = app.list_state.selected() {
                if let Some(issue) = app.data.issues.get(i) {
                    format!("#{}: {}", issue.number, issue.title)
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        }
        Tab::Sessions => "Not connected".to_string(),
    };

    let preview = Paragraph::new(text)
        .block(Block::bordered().title(" Preview "))
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(preview, area);
}

fn render_action_menu(app: &mut App, frame: &mut Frame) {
    if !app.show_action_menu {
        return;
    }

    let area = popup_area(frame.area(), 40, 40);
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .action_menu_items
        .iter()
        .enumerate()
        .map(|(i, item)| ListItem::new(format!(" {}: {}", i + 1, item)))
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title(" Actions "))
        .highlight_style(Style::default().bg(Color::Blue).bold())
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.action_menu_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let [area] = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .areas(area);
    area
}
