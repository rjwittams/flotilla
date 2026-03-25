use std::sync::OnceLock;

use crate::keymap::Action;

pub const MAX_PALETTE_ROWS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub name: &'static str,
    pub description: &'static str,
    pub key_hint: Option<&'static str>,
    pub action: Action,
}

pub fn all_entries() -> &'static [PaletteEntry] {
    static ENTRIES: OnceLock<Vec<PaletteEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        vec![
            PaletteEntry { name: "search", description: "filter items in view", key_hint: Some("/"), action: Action::OpenIssueSearch },
            PaletteEntry { name: "refresh", description: "refresh active repo", key_hint: Some("r"), action: Action::Refresh },
            PaletteEntry { name: "branch", description: "create a new branch", key_hint: Some("n"), action: Action::OpenBranchInput },
            PaletteEntry { name: "help", description: "show key bindings", key_hint: Some("h"), action: Action::ToggleHelp },
            PaletteEntry { name: "quit", description: "exit flotilla", key_hint: Some("q"), action: Action::Quit },
            PaletteEntry { name: "layout", description: "set view layout", key_hint: Some("l"), action: Action::CycleLayout },
            PaletteEntry { name: "target", description: "set provisioning target", key_hint: None, action: Action::CycleHost },
            PaletteEntry { name: "theme", description: "cycle color theme", key_hint: None, action: Action::CycleTheme },
            PaletteEntry { name: "providers", description: "show provider health", key_hint: None, action: Action::ToggleProviders },
            PaletteEntry { name: "debug", description: "show debug panel", key_hint: None, action: Action::ToggleDebug },
            PaletteEntry { name: "actions", description: "open context menu", key_hint: Some("."), action: Action::OpenActionMenu },
            PaletteEntry { name: "add repo", description: "track a repository", key_hint: None, action: Action::OpenFilePicker },
            PaletteEntry { name: "select", description: "toggle multi-select", key_hint: Some("space"), action: Action::ToggleMultiSelect },
            PaletteEntry { name: "keys", description: "toggle key hints", key_hint: Some("K"), action: Action::ToggleStatusBarKeys },
        ]
    })
}

pub fn filter_entries<'a>(entries: &'a [PaletteEntry], prefix: &str) -> Vec<&'a PaletteEntry> {
    if prefix.is_empty() {
        return entries.iter().collect();
    }
    let lower = prefix.to_lowercase();
    entries.iter().filter(|e| e.name.to_lowercase().starts_with(&lower)).collect()
}

/// Result of parsing a palette-local command (built-in noun-free commands).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteLocalResult<'a> {
    /// A layout subcommand was matched (e.g. "layout zoom").
    SetLayout(&'a str),
    /// A target subcommand was matched (e.g. "target feta").
    SetTarget(&'a str),
    /// A search subcommand was matched with a query (e.g. "search bug fix").
    Search(&'a str),
    /// A bare palette entry action was matched.
    Action(Action),
}

/// Parse a palette-local (built-in) command from the input string.
///
/// Returns `None` if the input is not a recognised palette-local command
/// (e.g. it looks like a noun-verb command).
pub fn parse_palette_local(_input: &str) -> Option<PaletteLocalResult<'_>> {
    todo!("implement in Task 5")
}

/// Return completions for the current palette-local input.
pub fn palette_local_completions(_input: &str) -> Vec<String> {
    todo!("implement in Task 5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_layout_command() {
        let result = parse_palette_local("layout zoom");
        assert_eq!(result, Some(PaletteLocalResult::SetLayout("zoom")));
    }

    #[test]
    fn parse_target_command() {
        let result = parse_palette_local("target feta");
        assert_eq!(result, Some(PaletteLocalResult::SetTarget("feta")));
    }

    #[test]
    fn parse_search_with_query() {
        let result = parse_palette_local("search bug fix");
        assert_eq!(result, Some(PaletteLocalResult::Search("bug fix")));
    }

    #[test]
    fn parse_bare_search_falls_through_to_entry() {
        let result = parse_palette_local("search");
        // "search" without trailing space returns the no-arg entry action
        assert!(matches!(result, Some(PaletteLocalResult::Action(Action::OpenIssueSearch))));
    }

    #[test]
    fn parse_noun_returns_none() {
        let result = parse_palette_local("cr #42 open");
        assert!(result.is_none());
    }

    #[test]
    fn layout_completions() {
        let completions = palette_local_completions("layout z");
        assert_eq!(completions, vec!["zoom"]);
    }

    #[test]
    fn layout_completions_all() {
        let completions = palette_local_completions("layout ");
        assert_eq!(completions.len(), 4);
    }

    #[test]
    fn all_entries_returns_expected_count() {
        let entries = all_entries();
        assert_eq!(entries.len(), 14);
        assert_eq!(entries[0].name, "search");
        assert_eq!(entries[entries.len() - 1].name, "keys");
    }

    #[test]
    fn filter_by_prefix() {
        let entries = all_entries();
        let filtered = filter_entries(entries, "re");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "refresh");
    }

    #[test]
    fn filter_empty_returns_all() {
        let entries = all_entries();
        let filtered = filter_entries(entries, "");
        assert_eq!(filtered.len(), entries.len());
    }

    #[test]
    fn filter_case_insensitive() {
        let entries = all_entries();
        let filtered = filter_entries(entries, "HELP");
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn filter_no_match_returns_empty() {
        let entries = all_entries();
        let filtered = filter_entries(entries, "zzz");
        assert!(filtered.is_empty());
    }
}
