use std::collections::HashSet;

use indexmap::IndexMap;

use flotilla_protocol::{Issue, IssuePage};

pub struct IssueCache {
    pub entries: IndexMap<String, Issue>,
    pub next_page: u32,
    pub has_more: bool,
    pub pinned: HashSet<String>,
    pub total_count: Option<u32>,
}

impl Default for IssueCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IssueCache {
    pub fn new() -> Self {
        Self {
            entries: IndexMap::new(),
            next_page: 1,
            has_more: true,
            pinned: HashSet::new(),
            total_count: None,
        }
    }

    pub fn merge_page(&mut self, page: IssuePage) {
        for issue in page.issues {
            self.entries.insert(issue.id.clone(), issue);
        }
        self.next_page += 1;
        self.has_more = page.has_more;
        if page.total_count.is_some() {
            self.total_count = page.total_count;
        }
    }

    pub fn pin(&mut self, ids: &[String]) {
        for id in ids {
            self.pinned.insert(id.clone());
        }
    }

    pub fn missing_ids(&self, ids: &[String]) -> Vec<String> {
        ids.iter()
            .filter(|id| !self.entries.contains_key(id.as_str()))
            .cloned()
            .collect()
    }

    pub fn add_pinned(&mut self, issues: Vec<Issue>) {
        for issue in issues {
            self.pinned.insert(issue.id.clone());
            self.entries.insert(issue.id.clone(), issue);
        }
    }

    pub fn to_index_map(&self) -> IndexMap<String, Issue> {
        self.entries.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &str) -> Issue {
        Issue {
            id: id.to_string(),
            title: format!("Issue {}", id),
            labels: vec![],
            association_keys: vec![],
        }
    }

    #[test]
    fn merge_page_appends_issues() {
        let mut cache = IssueCache::new();
        let page = IssuePage {
            issues: vec![issue("1"), issue("2")],
            total_count: Some(10),
            has_more: true,
        };
        cache.merge_page(page);
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(cache.total_count, Some(10));
        assert!(cache.has_more);
    }

    #[test]
    fn pin_issues_marks_as_pinned() {
        let mut cache = IssueCache::new();
        cache.merge_page(IssuePage {
            issues: vec![issue("1"), issue("2")],
            total_count: None,
            has_more: false,
        });
        cache.pin(&["1".to_string()]);
        assert!(cache.pinned.contains("1"));
        assert!(!cache.pinned.contains("2"));
    }

    #[test]
    fn missing_ids_returns_unpinned_absent_ids() {
        let mut cache = IssueCache::new();
        cache.merge_page(IssuePage {
            issues: vec![issue("1")],
            total_count: None,
            has_more: false,
        });
        let missing = cache.missing_ids(&["1".to_string(), "3".to_string(), "5".to_string()]);
        assert_eq!(missing, vec!["3", "5"]);
    }

    #[test]
    fn add_pinned_inserts_and_pins() {
        let mut cache = IssueCache::new();
        cache.add_pinned(vec![issue("99")]);
        assert!(cache.entries.contains_key("99"));
        assert!(cache.pinned.contains("99"));
    }

    #[test]
    fn to_index_map_returns_all_entries() {
        let mut cache = IssueCache::new();
        cache.merge_page(IssuePage {
            issues: vec![issue("1"), issue("2")],
            total_count: None,
            has_more: false,
        });
        cache.add_pinned(vec![issue("99")]);
        let map = cache.to_index_map();
        assert_eq!(map.len(), 3);
    }
}
