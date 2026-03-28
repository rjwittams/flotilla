use flotilla_core::data::SectionKind;
use flotilla_protocol::WorkItem;
use ratatui::{layout::Constraint, style::Style, text::Span, widgets::Cell};

use super::section_table::{ColumnDef, RenderCtx};
use crate::ui_helpers;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return the column definitions for the given section kind.
pub fn columns_for_section(kind: SectionKind) -> Vec<ColumnDef<WorkItem>> {
    match kind {
        SectionKind::Checkouts => checkout_columns(),
        SectionKind::CloudAgents => cloud_agent_columns(),
        SectionKind::ChangeRequests => change_request_columns(),
        SectionKind::Issues => issue_columns(),
        SectionKind::AttachableSets => attachable_set_columns(),
        SectionKind::RemoteBranches => remote_branch_columns(),
    }
}

// ---------------------------------------------------------------------------
// Section column sets
// ---------------------------------------------------------------------------

fn checkout_columns() -> Vec<ColumnDef<WorkItem>> {
    vec![
        icon_column(),
        col("Source", Constraint::Length(10), |item, ctx| {
            let text = item.source.clone().unwrap_or_default();
            styled_cell(text, ctx.theme.source)
        }),
        col("Path", Constraint::Fill(1), |item, ctx| {
            let text = if let Some(p) = item.checkout_key() {
                p.path.display().to_string()
            } else if let Some(ref ses_key) = item.session_key {
                ses_key.clone()
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.path)
        }),
        col("Description", Constraint::Fill(2), |item, ctx| styled_cell(item.description.clone(), ctx.theme.text)),
        col("Branch", Constraint::Fill(1), |item, ctx| {
            let text = item.branch.clone().unwrap_or_else(|| "—".to_string());
            styled_cell(text, ctx.theme.branch)
        }),
        col("WT", Constraint::Length(3), |item, ctx| {
            let text = ui_helpers::checkout_indicator(item.is_main_checkout, item.checkout_key().is_some()).to_string();
            styled_cell(text, ctx.theme.checkout)
        }),
        col("WS", Constraint::Length(3), |item, ctx| {
            let text = ui_helpers::workspace_indicator(item.workspace_refs.len());
            styled_cell(text, ctx.theme.workspace)
        }),
        col("PR", Constraint::Length(4), |item, ctx| {
            let text = if let Some(ref pr_key) = item.change_request_key {
                if let Some(cr) = ctx.providers.change_requests.get(pr_key.as_str()) {
                    let icon = ui_helpers::change_request_status_icon(&cr.status);
                    format!("#{}{}", pr_key, icon)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.change_request)
        }),
        col("SS", Constraint::Length(4), |item, ctx| {
            let text = if let Some(ref ses_key) = item.session_key {
                if let Some(ses) = ctx.providers.sessions.get(ses_key.as_str()) {
                    ui_helpers::session_status_display(&ses.status).to_string()
                } else {
                    String::new()
                }
            } else if let Some(agent_key) = item.agent_keys.first() {
                if let Some(agent) = ctx.providers.agents.get(agent_key.as_str()) {
                    ui_helpers::agent_status_display(&agent.status)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.session)
        }),
        col("Issues", Constraint::Length(6), |item, ctx| {
            let text = item.issue_keys.iter().map(|k| format!("#{}", k)).collect::<Vec<_>>().join(",");
            styled_cell(text, ctx.theme.issue)
        }),
        col("Git", Constraint::Length(5), |item, ctx| {
            let text = if let Some(wt_key) = item.checkout_key() {
                if let Some(co) = ctx.providers.checkouts.get(wt_key) {
                    ui_helpers::git_status_display(co)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.git_status)
        }),
    ]
}

fn cloud_agent_columns() -> Vec<ColumnDef<WorkItem>> {
    vec![
        icon_column(),
        col("Source", Constraint::Length(10), |item, ctx| {
            let text = item.source.clone().unwrap_or_default();
            styled_cell(text, ctx.theme.source)
        }),
        col("Key", Constraint::Fill(1), |item, ctx| {
            let text = item.session_key.clone().unwrap_or_default();
            styled_cell(text, ctx.theme.path)
        }),
        col("Description", Constraint::Fill(2), |item, ctx| styled_cell(item.description.clone(), ctx.theme.text)),
        col("Branch", Constraint::Fill(1), |item, ctx| {
            let text = item.branch.clone().unwrap_or_else(|| "—".to_string());
            styled_cell(text, ctx.theme.branch)
        }),
        col("Status", Constraint::Length(8), |item, ctx| {
            let text = if let Some(ref ses_key) = item.session_key {
                if let Some(ses) = ctx.providers.sessions.get(ses_key.as_str()) {
                    ui_helpers::session_status_display(&ses.status).to_string()
                } else {
                    String::new()
                }
            } else if let Some(agent_key) = item.agent_keys.first() {
                if let Some(agent) = ctx.providers.agents.get(agent_key.as_str()) {
                    ui_helpers::agent_status_display(&agent.status)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.session)
        }),
    ]
}

fn change_request_columns() -> Vec<ColumnDef<WorkItem>> {
    vec![
        icon_column(),
        col("PR#", Constraint::Length(6), |item, ctx| {
            let text = if let Some(ref pr_key) = item.change_request_key {
                if let Some(cr) = ctx.providers.change_requests.get(pr_key.as_str()) {
                    let icon = ui_helpers::change_request_status_icon(&cr.status);
                    format!("#{}{}", pr_key, icon)
                } else {
                    format!("#{}", pr_key)
                }
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.change_request)
        }),
        col("Title", Constraint::Fill(2), |item, ctx| styled_cell(item.description.clone(), ctx.theme.text)),
        col("Branch", Constraint::Fill(1), |item, ctx| {
            let text = item.branch.clone().unwrap_or_else(|| "—".to_string());
            styled_cell(text, ctx.theme.branch)
        }),
        col("State", Constraint::Length(8), |item, ctx| {
            let text = if let Some(ref pr_key) = item.change_request_key {
                if let Some(cr) = ctx.providers.change_requests.get(pr_key.as_str()) {
                    format!("{:?}", cr.status).to_lowercase()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            styled_cell(text, ctx.theme.change_request)
        }),
        col("Issues", Constraint::Length(8), |item, ctx| {
            let text = item.issue_keys.iter().map(|k| format!("#{}", k)).collect::<Vec<_>>().join(",");
            styled_cell(text, ctx.theme.issue)
        }),
    ]
}

fn issue_columns() -> Vec<ColumnDef<WorkItem>> {
    vec![
        icon_column(),
        col("ID", Constraint::Length(6), |item, ctx| {
            let text = item.issue_keys.first().map(|k| format!("#{}", k)).unwrap_or_default();
            styled_cell(text, ctx.theme.issue)
        }),
        col("Title", Constraint::Fill(2), |item, ctx| styled_cell(item.description.clone(), ctx.theme.text)),
        col("Labels", Constraint::Fill(1), |item, ctx| {
            let text = item
                .issue_keys
                .first()
                .and_then(|k| ctx.providers.issues.get(k.as_str()))
                .map(|issue| issue.labels.join(", "))
                .unwrap_or_default();
            styled_cell(text, ctx.theme.muted)
        }),
        col("PR", Constraint::Length(6), |item, ctx| {
            let text = item.change_request_key.as_ref().map(|k| format!("#{}", k)).unwrap_or_default();
            styled_cell(text, ctx.theme.change_request)
        }),
    ]
}

fn attachable_set_columns() -> Vec<ColumnDef<WorkItem>> {
    vec![icon_column(), col("Description", Constraint::Fill(2), |item, ctx| styled_cell(item.description.clone(), ctx.theme.text))]
}

fn remote_branch_columns() -> Vec<ColumnDef<WorkItem>> {
    vec![
        icon_column(),
        col("Branch", Constraint::Fill(1), |item, ctx| {
            let text = item.branch.clone().unwrap_or_else(|| "—".to_string());
            styled_cell(text, ctx.theme.branch)
        }),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Shared icon column: renders the work-item icon with its kind-appropriate color.
fn icon_column() -> ColumnDef<WorkItem> {
    col("", Constraint::Length(3), |item, ctx| {
        let session_status = item.session_key.as_deref().and_then(|k| ctx.providers.sessions.get(k)).map(|s| &s.status);
        let has_workspace = !item.workspace_refs.is_empty();
        let (icon, color) = ui_helpers::work_item_icon(&item.kind, has_workspace, session_status, ctx.theme);
        let text = format!(" {}", icon);
        styled_cell(text, color)
    })
}

/// Build a `ColumnDef<WorkItem>` with the given header, width, and extract closure.
fn col<F>(header: &str, width: Constraint, extract: F) -> ColumnDef<WorkItem>
where
    F: Fn(&WorkItem, &RenderCtx) -> Cell<'static> + 'static,
{
    ColumnDef { header: header.to_string(), width, extract: Box::new(extract) }
}

/// Produce a `Cell` with the given text and foreground color.
fn styled_cell(text: String, color: ratatui::style::Color) -> Cell<'static> {
    Cell::from(Span::styled(text, Style::default().fg(color)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_for_section_returns_expected_counts() {
        assert_eq!(columns_for_section(SectionKind::Checkouts).len(), 11);
        assert_eq!(columns_for_section(SectionKind::CloudAgents).len(), 6);
        assert_eq!(columns_for_section(SectionKind::ChangeRequests).len(), 6);
        assert_eq!(columns_for_section(SectionKind::Issues).len(), 5);
        assert_eq!(columns_for_section(SectionKind::AttachableSets).len(), 2);
        assert_eq!(columns_for_section(SectionKind::RemoteBranches).len(), 2);
    }

    #[test]
    fn issue_columns_have_expected_headers() {
        let cols = issue_columns();
        let headers: Vec<&str> = cols.iter().map(|c| c.header.as_str()).collect();
        assert_eq!(headers, vec!["", "ID", "Title", "Labels", "PR"]);
    }

    #[test]
    fn checkout_columns_have_expected_headers() {
        let cols = checkout_columns();
        let headers: Vec<&str> = cols.iter().map(|c| c.header.as_str()).collect();
        assert_eq!(headers, vec!["", "Source", "Path", "Description", "Branch", "WT", "WS", "PR", "SS", "Issues", "Git"]);
    }

    #[test]
    fn cloud_agent_columns_have_expected_headers() {
        let cols = cloud_agent_columns();
        let headers: Vec<&str> = cols.iter().map(|c| c.header.as_str()).collect();
        assert_eq!(headers, vec!["", "Source", "Key", "Description", "Branch", "Status"]);
    }

    #[test]
    fn change_request_columns_have_expected_headers() {
        let cols = change_request_columns();
        let headers: Vec<&str> = cols.iter().map(|c| c.header.as_str()).collect();
        assert_eq!(headers, vec!["", "PR#", "Title", "Branch", "State", "Issues"]);
    }
}
