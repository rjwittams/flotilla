//! Delta computation: diff two IndexMaps to produce keyed entry operations,
//! and full `ProviderData` snapshot diffing.

use std::hash::Hash;

use indexmap::IndexMap;

use flotilla_protocol::{Change, EntryOp, ProviderData};

/// Diff two IndexMaps, producing `(key, EntryOp)` pairs for all differences.
///
/// - Keys in `curr` but not `prev` → `Added`
/// - Keys in both with different values → `Updated`
/// - Keys in `prev` but not `curr` → `Removed`
/// - Keys in both with equal values → omitted
pub fn diff_indexmap<K, V>(prev: &IndexMap<K, V>, curr: &IndexMap<K, V>) -> Vec<(K, EntryOp<V>)>
where
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
{
    let mut changes = Vec::new();

    for (key, curr_val) in curr {
        match prev.get(key) {
            Some(prev_val) if prev_val == curr_val => {}
            Some(_) => changes.push((key.clone(), EntryOp::Updated(curr_val.clone()))),
            None => changes.push((key.clone(), EntryOp::Added(curr_val.clone()))),
        }
    }

    for key in prev.keys() {
        if !curr.contains_key(key) {
            changes.push((key.clone(), EntryOp::Removed));
        }
    }

    changes
}

/// Diff two `ProviderData` snapshots, producing a `Vec<Change>` covering all collections.
pub fn diff_provider_data(prev: &ProviderData, curr: &ProviderData) -> Vec<Change> {
    let mut changes = Vec::new();

    for (key, op) in diff_indexmap(&prev.checkouts, &curr.checkouts) {
        changes.push(Change::Checkout { key, op });
    }
    for (key, op) in diff_indexmap(&prev.change_requests, &curr.change_requests) {
        changes.push(Change::ChangeRequest { key, op });
    }
    for (key, op) in diff_indexmap(&prev.issues, &curr.issues) {
        changes.push(Change::Issue { key, op });
    }
    for (key, op) in diff_indexmap(&prev.sessions, &curr.sessions) {
        changes.push(Change::Session { key, op });
    }
    for (key, op) in diff_indexmap(&prev.workspaces, &curr.workspaces) {
        changes.push(Change::Workspace { key, op });
    }
    for (key, op) in diff_indexmap(&prev.branches, &curr.branches) {
        changes.push(Change::Branch { key, op });
    }

    changes
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use flotilla_protocol::delta::{Branch, BranchStatus};
    use flotilla_protocol::{
        ChangeRequest, ChangeRequestStatus, Checkout, CloudAgentSession, Issue, SessionStatus,
        Workspace,
    };

    use super::*;

    fn checkout(branch: &str) -> Checkout {
        Checkout {
            branch: branch.into(),
            is_trunk: false,
            trunk_ahead_behind: None,
            remote_ahead_behind: None,
            working_tree: None,
            last_commit: None,
            correlation_keys: vec![],
            association_keys: vec![],
        }
    }

    fn change_request(title: &str) -> ChangeRequest {
        ChangeRequest {
            title: title.into(),
            branch: "main".into(),
            status: ChangeRequestStatus::Open,
            body: None,
            correlation_keys: vec![],
            association_keys: vec![],
        }
    }

    fn issue(title: &str) -> Issue {
        Issue {
            title: title.into(),
            labels: vec![],
            association_keys: vec![],
        }
    }

    fn session(title: &str) -> CloudAgentSession {
        CloudAgentSession {
            title: title.into(),
            status: SessionStatus::Idle,
            model: None,
            updated_at: None,
            correlation_keys: vec![],
        }
    }

    fn workspace(name: &str) -> Workspace {
        Workspace {
            name: name.into(),
            directories: vec![],
            correlation_keys: vec![],
        }
    }

    // --- diff_indexmap tests ---

    #[test]
    fn empty_to_empty() {
        let prev: IndexMap<String, i32> = IndexMap::new();
        let curr: IndexMap<String, i32> = IndexMap::new();
        assert!(diff_indexmap(&prev, &curr).is_empty());
    }

    #[test]
    fn empty_to_entries_all_added() {
        let prev: IndexMap<String, i32> = IndexMap::new();
        let curr: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 1), ("b".into(), 2)]);
        let changes = diff_indexmap(&prev, &curr);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0], ("a".into(), EntryOp::Added(1)));
        assert_eq!(changes[1], ("b".into(), EntryOp::Added(2)));
    }

    #[test]
    fn entries_to_empty_all_removed() {
        let prev: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 1), ("b".into(), 2)]);
        let curr: IndexMap<String, i32> = IndexMap::new();
        let changes = diff_indexmap(&prev, &curr);
        assert_eq!(changes.len(), 2);
        assert!(changes.contains(&("a".into(), EntryOp::Removed)));
        assert!(changes.contains(&("b".into(), EntryOp::Removed)));
    }

    #[test]
    fn identical_no_changes() {
        let map: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 1), ("b".into(), 2)]);
        assert!(diff_indexmap(&map, &map).is_empty());
    }

    #[test]
    fn mixed_add_update_remove() {
        let prev: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 1), ("b".into(), 2)]);
        let curr: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 10), ("c".into(), 3)]);
        let changes = diff_indexmap(&prev, &curr);
        assert_eq!(changes.len(), 3);
        assert!(changes.contains(&("a".into(), EntryOp::Updated(10))));
        assert!(changes.contains(&("c".into(), EntryOp::Added(3))));
        assert!(changes.contains(&("b".into(), EntryOp::Removed)));
    }

    #[test]
    fn value_unchanged_key_not_emitted() {
        let prev: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 1), ("b".into(), 2), ("c".into(), 3)]);
        let curr: IndexMap<String, i32> =
            IndexMap::from([("a".into(), 1), ("b".into(), 99), ("c".into(), 3)]);
        let changes = diff_indexmap(&prev, &curr);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], ("b".into(), EntryOp::Updated(99)));
    }

    // --- diff_provider_data tests ---

    #[test]
    fn diff_empty_snapshots() {
        let prev = ProviderData::default();
        let curr = ProviderData::default();
        assert!(diff_provider_data(&prev, &curr).is_empty());
    }

    #[test]
    fn diff_checkout_added() {
        let prev = ProviderData::default();
        let mut curr = ProviderData::default();
        curr.checkouts
            .insert(PathBuf::from("/wt/feat"), checkout("feat"));
        let changes = diff_provider_data(&prev, &curr);
        assert_eq!(changes.len(), 1);
        match &changes[0] {
            Change::Checkout { key, op: EntryOp::Added(co) } => {
                assert_eq!(key, &PathBuf::from("/wt/feat"));
                assert_eq!(co.branch, "feat");
            }
            other => panic!("unexpected change: {other:?}"),
        }
    }

    #[test]
    fn diff_checkout_removed() {
        let mut prev = ProviderData::default();
        prev.checkouts
            .insert(PathBuf::from("/wt/old"), checkout("old"));
        let curr = ProviderData::default();
        let changes = diff_provider_data(&prev, &curr);
        assert_eq!(changes.len(), 1);
        match &changes[0] {
            Change::Checkout { key, op: EntryOp::Removed } => {
                assert_eq!(key, &PathBuf::from("/wt/old"));
            }
            other => panic!("unexpected change: {other:?}"),
        }
    }

    #[test]
    fn diff_change_request_updated() {
        let mut prev = ProviderData::default();
        prev.change_requests
            .insert("42".into(), change_request("old title"));
        let mut curr = ProviderData::default();
        curr.change_requests
            .insert("42".into(), change_request("new title"));
        let changes = diff_provider_data(&prev, &curr);
        assert_eq!(changes.len(), 1);
        match &changes[0] {
            Change::ChangeRequest { key, op: EntryOp::Updated(cr) } => {
                assert_eq!(key, "42");
                assert_eq!(cr.title, "new title");
            }
            other => panic!("unexpected change: {other:?}"),
        }
    }

    #[test]
    fn diff_branch_added_and_removed() {
        let mut prev = ProviderData::default();
        prev.branches.insert(
            "old-branch".into(),
            Branch {
                status: BranchStatus::Remote,
            },
        );
        let mut curr = ProviderData::default();
        curr.branches.insert(
            "new-branch".into(),
            Branch {
                status: BranchStatus::Merged,
            },
        );
        let changes = diff_provider_data(&prev, &curr);
        assert_eq!(changes.len(), 2);
        // One added, one removed
        let added = changes.iter().any(|c| matches!(c, Change::Branch { key, op: EntryOp::Added(_) } if key == "new-branch"));
        let removed = changes.iter().any(|c| matches!(c, Change::Branch { key, op: EntryOp::Removed } if key == "old-branch"));
        assert!(added, "expected new-branch Added");
        assert!(removed, "expected old-branch Removed");
    }

    #[test]
    fn diff_mixed_collections() {
        let mut prev = ProviderData::default();
        prev.checkouts
            .insert(PathBuf::from("/wt/main"), checkout("main"));
        prev.issues.insert("1".into(), issue("bug"));
        prev.sessions.insert("s1".into(), session("session 1"));

        let mut curr = ProviderData::default();
        curr.checkouts
            .insert(PathBuf::from("/wt/main"), checkout("main")); // unchanged
        curr.issues.insert("1".into(), issue("bug fix")); // updated title
        curr.workspaces.insert("w1".into(), workspace("dev")); // added
        // sessions removed

        let changes = diff_provider_data(&prev, &curr);
        assert_eq!(changes.len(), 3);
        let has_issue_update = changes
            .iter()
            .any(|c| matches!(c, Change::Issue { key, op: EntryOp::Updated(_) } if key == "1"));
        let has_ws_add = changes
            .iter()
            .any(|c| matches!(c, Change::Workspace { key, op: EntryOp::Added(_) } if key == "w1"));
        let has_session_remove = changes
            .iter()
            .any(|c| matches!(c, Change::Session { key, op: EntryOp::Removed } if key == "s1"));
        assert!(has_issue_update, "expected issue Updated");
        assert!(has_ws_add, "expected workspace Added");
        assert!(has_session_remove, "expected session Removed");
    }

    #[test]
    fn diff_identical_snapshots_no_changes() {
        let mut pd = ProviderData::default();
        pd.checkouts
            .insert(PathBuf::from("/wt/main"), checkout("main"));
        pd.change_requests
            .insert("1".into(), change_request("pr 1"));
        pd.issues.insert("10".into(), issue("task"));
        pd.sessions.insert("s1".into(), session("sess"));
        pd.workspaces.insert("w1".into(), workspace("dev"));
        pd.branches.insert(
            "feat".into(),
            Branch {
                status: BranchStatus::Remote,
            },
        );
        assert!(diff_provider_data(&pd, &pd).is_empty());
    }
}
