use std::any::Any;

use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::{layout::Rect, Frame};

use super::{base_view::BaseView, AppAction, InteractiveWidget, Outcome, RenderContext, WidgetContext, WidgetStatusData};
use crate::keymap::{Action, ModeId};

/// Root widget that wraps the base layer. Will eventually own Tabs,
/// StatusBar, and the modal stack. For now, delegates to BaseView.
pub struct Screen {
    pub base_view: BaseView,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        Self { base_view: BaseView::new() }
    }
}

impl InteractiveWidget for Screen {
    fn handle_action(&mut self, action: Action, ctx: &mut WidgetContext) -> Outcome {
        // Phase 1: Global actions
        match action {
            Action::PrevTab => {
                ctx.app_actions.push(AppAction::PrevTab);
                return Outcome::Consumed;
            }
            Action::NextTab => {
                ctx.app_actions.push(AppAction::NextTab);
                return Outcome::Consumed;
            }
            Action::MoveTabLeft => {
                ctx.app_actions.push(AppAction::MoveTabLeft);
                return Outcome::Consumed;
            }
            Action::MoveTabRight => {
                ctx.app_actions.push(AppAction::MoveTabRight);
                return Outcome::Consumed;
            }
            Action::CycleTheme => {
                ctx.app_actions.push(AppAction::CycleTheme);
                return Outcome::Consumed;
            }
            Action::CycleHost => {
                ctx.app_actions.push(AppAction::CycleHost);
                return Outcome::Consumed;
            }
            Action::ToggleDebug => {
                ctx.app_actions.push(AppAction::ToggleDebug);
                return Outcome::Consumed;
            }
            Action::ToggleStatusBarKeys => {
                ctx.app_actions.push(AppAction::ToggleStatusBarKeys);
                return Outcome::Consumed;
            }
            Action::Refresh => {
                ctx.app_actions.push(AppAction::Refresh);
                return Outcome::Consumed;
            }
            _ => {}
        }
        // Phase 2: Delegate to BaseView
        self.base_view.handle_action(action, ctx)
    }

    fn handle_raw_key(&mut self, key: KeyEvent, ctx: &mut WidgetContext) -> Outcome {
        self.base_view.handle_raw_key(key, ctx)
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, ctx: &mut WidgetContext) -> Outcome {
        self.base_view.handle_mouse(mouse, ctx)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, ctx: &mut RenderContext) {
        self.base_view.render(frame, area, ctx)
    }

    fn mode_id(&self) -> ModeId {
        self.base_view.mode_id()
    }

    fn captures_raw_keys(&self) -> bool {
        self.base_view.captures_raw_keys()
    }

    fn status_data(&self) -> WidgetStatusData {
        self.base_view.status_data()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
