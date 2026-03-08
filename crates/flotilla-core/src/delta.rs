//! Delta computation: diff two IndexMaps to produce keyed entry operations.

use std::hash::Hash;

use indexmap::IndexMap;

use flotilla_protocol::EntryOp;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
