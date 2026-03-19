use std::collections::HashMap;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

use crate::{
    app::{InFlightCommand, RepoViewLayout, TuiModel, UiState},
    keymap::{Keymap, ModeId},
    theme::Theme,
    ui_helpers,
    widgets::work_item_table::WorkItemTable,
};

const PREVIEW_SPLIT_RIGHT_PERCENT: u16 = 40;
const PREVIEW_SPLIT_BELOW_PERCENT: u16 = 40;
const MIN_TABLE_WIDTH: u16 = 50;
const MIN_PREVIEW_WIDTH: u16 = 32;
const MIN_TABLE_HEIGHT: u16 = 8;
const MIN_PREVIEW_HEIGHT: u16 = 6;
const PREVIEW_BELOW_ASPECT_RATIO_THRESHOLD: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedPreviewPosition {
    Right,
    Below,
}

fn resolve_preview_position(area: Rect, layout: RepoViewLayout) -> Option<ResolvedPreviewPosition> {
    match layout {
        RepoViewLayout::Right => Some(ResolvedPreviewPosition::Right),
        RepoViewLayout::Below => Some(ResolvedPreviewPosition::Below),
        RepoViewLayout::Auto => Some(resolve_auto_preview_position(area)),
        RepoViewLayout::Zoom => None,
    }
}

fn resolve_auto_preview_position(area: Rect) -> ResolvedPreviewPosition {
    let right_preview_width = area.width.saturating_mul(PREVIEW_SPLIT_RIGHT_PERCENT) / 100;
    let right_table_width = area.width.saturating_sub(right_preview_width);
    let below_preview_height = area.height.saturating_mul(PREVIEW_SPLIT_BELOW_PERCENT) / 100;
    let below_table_height = area.height.saturating_sub(below_preview_height);

    let right_viable = right_table_width >= MIN_TABLE_WIDTH && right_preview_width >= MIN_PREVIEW_WIDTH;
    let below_viable = below_table_height >= MIN_TABLE_HEIGHT && below_preview_height >= MIN_PREVIEW_HEIGHT;

    match (right_viable, below_viable) {
        (true, false) => ResolvedPreviewPosition::Right,
        (false, true) => ResolvedPreviewPosition::Below,
        (false, false) => ResolvedPreviewPosition::Right,
        (true, true) => {
            let aspect_ratio = area.width as f32 / area.height as f32;
            if aspect_ratio < PREVIEW_BELOW_ASPECT_RATIO_THRESHOLD {
                ResolvedPreviewPosition::Below
            } else {
                ResolvedPreviewPosition::Right
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    model: &TuiModel,
    ui: &mut UiState,
    in_flight: &HashMap<u64, InFlightCommand>,
    theme: &Theme,
    _keymap: &Keymap,
    frame: &mut Frame,
    active_widget_mode: Option<ModeId>,
    active_widget_data: crate::widgets::WidgetStatusData,
    tab_bar: &mut crate::widgets::tab_bar::TabBar,
    status_bar_widget: &mut crate::widgets::status_bar_widget::StatusBarWidget,
    event_log_widget: &mut crate::widgets::event_log::EventLogWidget,
    preview_panel: &crate::widgets::preview_panel::PreviewPanel,
    work_item_table: &WorkItemTable,
) {
    let constraints = vec![Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)];
    let chunks = Layout::default().direction(Direction::Vertical).constraints(constraints).split(frame.area());

    tab_bar.render(model, ui, theme, frame, chunks[0]);
    render_content(model, ui, theme, frame, chunks[1], event_log_widget, preview_panel, work_item_table);

    // When the palette is active, move the status bar to the top of the overlay so the
    // input sits above the results instead of being pinned to the bottom of the screen.
    let status_bar_area = if active_widget_mode == Some(ModeId::CommandPalette) {
        ui_helpers::bottom_anchored_overlay(frame.area(), 1, crate::palette::MAX_PALETTE_ROWS as u16).status_row
    } else {
        chunks[2]
    };
    status_bar_widget.render(model, ui, in_flight, theme, frame, status_bar_area, active_widget_mode, active_widget_data);
}

#[allow(clippy::too_many_arguments)]
fn render_content(
    model: &TuiModel,
    ui: &mut UiState,
    theme: &Theme,
    frame: &mut Frame,
    area: Rect,
    event_log_widget: &mut crate::widgets::event_log::EventLogWidget,
    preview_panel: &crate::widgets::preview_panel::PreviewPanel,
    work_item_table: &WorkItemTable,
) {
    if ui.mode.is_config() {
        event_log_widget.render_config_screen(model, theme, frame, area);
        return;
    }

    let Some(position) = resolve_preview_position(area, ui.view_layout) else {
        work_item_table.render(model, ui, theme, frame, area);
        return;
    };

    let chunks = match position {
        ResolvedPreviewPosition::Right => Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100 - PREVIEW_SPLIT_RIGHT_PERCENT), Constraint::Percentage(PREVIEW_SPLIT_RIGHT_PERCENT)])
            .split(area),
        ResolvedPreviewPosition::Below => Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(100 - PREVIEW_SPLIT_BELOW_PERCENT), Constraint::Percentage(PREVIEW_SPLIT_BELOW_PERCENT)])
            .split(area),
    };

    work_item_table.render(model, ui, theme, frame, chunks[0]);
    preview_panel.render(model, ui, theme, frame, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::RepoViewLayout;

    #[test]
    fn auto_layout_prefers_right_when_wide() {
        let position = resolve_preview_position(Rect::new(0, 0, 160, 40), RepoViewLayout::Auto);
        assert_eq!(position, Some(ResolvedPreviewPosition::Right));
    }

    #[test]
    fn auto_layout_prefers_below_when_tall() {
        let position = resolve_preview_position(Rect::new(0, 0, 90, 50), RepoViewLayout::Auto);
        assert_eq!(position, Some(ResolvedPreviewPosition::Below));
    }

    #[test]
    fn explicit_right_layout() {
        let position = resolve_preview_position(Rect::new(0, 0, 90, 50), RepoViewLayout::Right);
        assert_eq!(position, Some(ResolvedPreviewPosition::Right));
    }

    #[test]
    fn explicit_below_layout() {
        let position = resolve_preview_position(Rect::new(0, 0, 160, 40), RepoViewLayout::Below);
        assert_eq!(position, Some(ResolvedPreviewPosition::Below));
    }

    #[test]
    fn zoom_layout_returns_none() {
        let position = resolve_preview_position(Rect::new(0, 0, 160, 40), RepoViewLayout::Zoom);
        assert_eq!(position, None);
    }

    #[test]
    fn auto_neither_viable_falls_back_to_right() {
        // 60x10: right_preview_width = 24 (< MIN_PREVIEW_WIDTH 32),
        //        below_preview_height = 4 (< MIN_PREVIEW_HEIGHT 6)
        // Both layouts are non-viable, so fallback to Right.
        let result = resolve_auto_preview_position(Rect::new(0, 0, 60, 10));
        assert_eq!(result, ResolvedPreviewPosition::Right);
    }

    #[test]
    fn auto_only_right_viable() {
        // 210x10: right_preview_width = 84 (>= 32), right_table_width = 126 (>= 50) → viable
        //         below_preview_height = 4 (< 6) → not viable
        let result = resolve_auto_preview_position(Rect::new(0, 0, 210, 10));
        assert_eq!(result, ResolvedPreviewPosition::Right);
    }

    #[test]
    fn auto_only_below_viable() {
        // 60x40: right_preview_width = 24 (< 32) → not viable
        //        below_preview_height = 16 (>= 6), below_table_height = 24 (>= 8) → viable
        let result = resolve_auto_preview_position(Rect::new(0, 0, 60, 40));
        assert_eq!(result, ResolvedPreviewPosition::Below);
    }

    #[test]
    fn auto_both_viable_wide_prefers_right() {
        // 160x40: both viable, aspect_ratio = 4.0 (>= 2.0) → Right
        let result = resolve_auto_preview_position(Rect::new(0, 0, 160, 40));
        assert_eq!(result, ResolvedPreviewPosition::Right);
    }

    #[test]
    fn auto_both_viable_tall_prefers_below() {
        // 90x50: both viable, aspect_ratio = 1.8 (< 2.0) → Below
        let result = resolve_auto_preview_position(Rect::new(0, 0, 90, 50));
        assert_eq!(result, ResolvedPreviewPosition::Below);
    }
}
