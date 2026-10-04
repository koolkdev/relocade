use super::*;

#[test]
fn nested_scopes_restore_insertions_overwrites_and_retained_entries() {
    let mut map = ScopedMap::default();
    map.insert(1, 10);
    map.insert(2, 20);
    let outer = map.checkpoint();
    map.insert(1, 11);
    map.insert(1, 11);
    map.insert(3, 30);
    let inner = map.checkpoint();
    map.retain(|&key, value| {
        *value += 100;
        key != 2
    });
    map.insert(3, 31);
    assert_eq!(
        (map.get(&1), map.get(&2), map.get(&3)),
        (Some(&111), None, Some(&31))
    );
    map.restore(inner);
    assert_eq!(
        (map.get(&1), map.get(&2), map.get(&3)),
        (Some(&11), Some(&20), Some(&30))
    );
    map.restore(outer);
    assert_eq!(
        (map.get(&1), map.get(&2), map.get(&3)),
        (Some(&10), Some(&20), None)
    );
}

#[test]
fn snapshots_have_independent_entries_and_scopes() {
    let mut original = ScopedMap::default();
    original.insert(1, 10);
    let scope = original.checkpoint();
    original.insert(1, 20);
    let mut snapshot = original.clone();
    assert_eq!(snapshot.scopes, 0);
    assert!(snapshot.changes.is_empty());
    original.restore(scope);
    assert_eq!(original.get(&1), Some(&10));
    assert_eq!(snapshot.get(&1), Some(&20));

    let scope = snapshot.checkpoint();
    snapshot.retain(|_, _| false);
    snapshot.insert(2, 30);
    snapshot.restore(scope);
    assert_eq!(snapshot.get(&1), Some(&20));
    assert_eq!(snapshot.get(&2), None);
    assert_eq!(original.get(&1), Some(&10));
}
