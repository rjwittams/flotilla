use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

pub const CHEVRON_SEPARATOR: &str = "";
const SECTION_GAP: &str = " ";
const SPINNER_FRAMES: [&str; 4] = ["-", "\\", "|", "/"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusBarAction {
    OpenSelected,
    StartSearch,
    Quit,
    NewBranch,
    ToggleKeys,
    OpenHelp,
    Refresh,
    OpenMenu,
    ClearError(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyChip {
    pub key: String,
    pub label: String,
    pub action: StatusBarAction,
}

impl KeyChip {
    pub fn new(key: &str, label: &str, action: StatusBarAction) -> Self {
        Self { key: key.to_string(), label: label.to_string(), action }
    }

    fn render(&self) -> String {
        format!("<{}> {}", self.key, self.label)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskSection {
    pub description: String,
    pub spinner_index: usize,
}

impl TaskSection {
    pub fn new(description: &str, spinner_index: usize) -> Self {
        Self { description: description.to_string(), spinner_index }
    }

    fn render(&self) -> String {
        let spinner = SPINNER_FRAMES[self.spinner_index % SPINNER_FRAMES.len()];
        format!("{spinner} {}", self.description)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusSection {
    Plain(String),
    Error { id: usize, text: String },
}

impl StatusSection {
    pub fn plain(text: &str) -> Self {
        Self::Plain(text.to_string())
    }

    fn render(&self) -> String {
        match self {
            Self::Plain(text) => text.clone(),
            Self::Error { text, .. } => text.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarInput {
    pub width: usize,
    pub keys_visible: bool,
    pub status: StatusSection,
    pub task: Option<TaskSection>,
    pub keys: Vec<KeyChip>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarModel {
    pub status_text: String,
    pub keys_text: String,
    pub task_text: String,
    pub visible_keys: Vec<KeyChip>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBarTarget {
    pub area: Rect,
    pub action: StatusBarAction,
}

impl StatusBarTarget {
    pub fn new(area: Rect, action: StatusBarAction) -> Self {
        Self { area, action }
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.area.x
            && x < self.area.x.saturating_add(self.area.width)
            && y >= self.area.y
            && y < self.area.y.saturating_add(self.area.height)
    }
}

impl StatusBarModel {
    pub fn build(input: StatusBarInput) -> Self {
        let mut status_text = input.status.render();
        let mut task_text = input.task.as_ref().map(TaskSection::render).unwrap_or_default();
        let mut visible_keys = if input.keys_visible { input.keys.clone() } else { vec![] };

        loop {
            let keys_text = render_keys(&visible_keys);
            let total_width = joined_width(&status_text, &keys_text, &task_text);
            if total_width <= input.width {
                return Self { status_text, keys_text, task_text, visible_keys };
            }

            if !visible_keys.is_empty() {
                visible_keys.pop();
                continue;
            }
            if !task_text.is_empty() && displayed_width(&task_text) > 12 {
                task_text = ellipsize(&task_text, displayed_width(&task_text).saturating_sub(1));
                continue;
            }
            if displayed_width(&status_text) > 8 {
                status_text = ellipsize(&status_text, displayed_width(&status_text).saturating_sub(1));
                continue;
            }

            return Self { status_text, keys_text, task_text, visible_keys };
        }
    }
}

fn render_keys(keys: &[KeyChip]) -> String {
    keys.iter().map(KeyChip::render).collect::<Vec<_>>().join(&format!(" {CHEVRON_SEPARATOR}{CHEVRON_SEPARATOR} "))
}

fn joined_width(status: &str, keys: &str, task: &str) -> usize {
    let mut parts = 0;
    let mut width = 0;

    for text in [status, keys, task] {
        if text.is_empty() {
            continue;
        }
        if parts > 0 {
            width += SECTION_GAP.width();
        }
        width += displayed_width(text);
        parts += 1;
    }

    width
}

fn displayed_width(text: &str) -> usize {
    text.width()
}

fn ellipsize(text: &str, max_width: usize) -> String {
    if displayed_width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }

    let mut result = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = ch.len_utf8();
        let glyph_width = ch.to_string().width();
        if used + glyph_width >= max_width {
            break;
        }
        result.push(ch);
        used += glyph_width;
        if ch_width == 0 {
            break;
        }
    }
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_keys_before_truncating_task_or_status() {
        let model = StatusBarModel::build(StatusBarInput {
            width: 48,
            keys_visible: true,
            status: StatusSection::plain("Connected"),
            task: Some(TaskSection::new("Refreshing repository...", 0)),
            keys: vec![
                KeyChip::new("enter", "OPEN", StatusBarAction::OpenSelected),
                KeyChip::new("/", "SEARCH", StatusBarAction::StartSearch),
                KeyChip::new("q", "QUIT", StatusBarAction::Quit),
            ],
        });

        assert!(model.visible_keys.len() < 3);
        assert!(model.task_text.contains("Refreshing"));
        assert!(model.status_text.contains("Connected"));
    }

    #[test]
    fn hidden_keys_remove_middle_section_entirely() {
        let model = StatusBarModel::build(StatusBarInput {
            width: 80,
            keys_visible: false,
            status: StatusSection::plain("Ready"),
            task: None,
            keys: vec![KeyChip::new("q", "QUIT", StatusBarAction::Quit)],
        });

        assert!(model.visible_keys.is_empty());
    }

    #[test]
    fn key_ribbons_use_chevron_separators_when_multiple_actions_are_visible() {
        let model = StatusBarModel::build(StatusBarInput {
            width: 80,
            keys_visible: true,
            status: StatusSection::plain("Ready"),
            task: None,
            keys: vec![KeyChip::new("/", "SEARCH", StatusBarAction::StartSearch), KeyChip::new("n", "NEW", StatusBarAction::NewBranch)],
        });

        assert!(model.keys_text.contains(CHEVRON_SEPARATOR));
    }
}
