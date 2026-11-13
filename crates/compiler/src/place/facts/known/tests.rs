use super::*;

fn bits(mask: u64, value: u64) -> Bits {
    Bits { mask, value }
}

fn get(facts: &KnownBits, id: usize) -> Option<(u64, u64)> {
    facts.get(id).map(|bits| (bits.mask, bits.value))
}

#[test]
fn forks_keep_old_values_across_leaf_and_directory_growth() {
    let mut parent = KnownBits::default();
    for id in [0, 31, 32, 1023, 1024, 32767] {
        parent.insert(id, bits(0xff, 5));
    }
    assert!(parent.get(32768).is_none());
    assert!(parent.get(usize::MAX).is_none());
    let mut child = parent.clone();
    child.insert(31, bits(0xff, 7));
    child.insert(32768, bits(1, 0));
    child.insert(usize::MAX, bits(u64::MAX, 11));
    let mut sibling = parent.clone();
    sibling.insert(32, bits(0xff, 9));

    for id in [0, 31, 32, 1023, 1024, 32767] {
        assert_eq!(get(&parent, id), Some((0xff, 5)));
    }
    assert_eq!(get(&child, 31), Some((0xff, 7)));
    assert_eq!(get(&child, 32), Some((0xff, 5)));
    assert_eq!(get(&child, 32768), Some((1, 0)));
    assert_eq!(get(&child, usize::MAX), Some((u64::MAX, 11)));
    assert_eq!(get(&sibling, 31), Some((0xff, 5)));
    assert_eq!(get(&sibling, 32), Some((0xff, 9)));
    assert!(parent.get(32768).is_none());
    assert!(sibling.get(usize::MAX).is_none());
    for id in [1, 33, 1022, 1025, usize::MAX - 1] {
        assert!(child.get(id).is_none());
    }
}

#[test]
fn joins_align_roots_and_keep_only_common_bits() {
    let mut small = KnownBits::default();
    small.insert(3, bits(0xff, 5));
    small.insert(31, bits(1, 0));
    let mut large = small.clone();
    large.insert(3, bits(0xff, 7));
    large.insert(31, bits(1, 1));
    large.insert(1024, bits(0xff, 9));
    large.insert(usize::MAX, bits(0xff, 11));

    for (first, second) in [(&small, &large), (&large, &small)] {
        let mut joined = first.clone();
        joined.retain_common(second);
        assert_eq!(get(&joined, 3), Some((0xfd, 5)));
        assert!(joined.get(31).is_none());
        assert!(joined.get(1024).is_none());
        assert!(joined.get(usize::MAX).is_none());
        joined.insert(32, bits(1, 1));
        assert_eq!(get(&joined, 32), Some((1, 1)));
        joined.retain_common(&KnownBits::default());
        assert!(joined.is_empty());
    }
    assert_eq!(get(&small, 3), Some((0xff, 5)));
    assert_eq!(get(&large, 3), Some((0xff, 7)));

    let mut disjoint = KnownBits::default();
    disjoint.insert(32768, bits(1, 1));
    let mut joined = small.clone();
    joined.retain_common(&disjoint);
    assert!(joined.is_empty());
    disjoint.retain_common(&small);
    assert!(disjoint.is_empty());
    disjoint.insert(0, bits(1, 0));
    assert_eq!(get(&disjoint, 0), Some((1, 0)));
}

#[test]
fn conflict_queries_visit_only_present_facts_with_their_original_ids() {
    let mut facts = KnownBits::default();
    let ids = [0, 31, 32, 1024, usize::MAX];
    for id in ids {
        facts.insert(id, bits(1, 0));
    }
    let mut visited = Vec::new();
    assert!(!facts.conflicts_with(|id| {
        visited.push(id);
        Bits::default()
    }));
    assert_eq!(visited, ids);
    assert!(facts.conflicts_with(|id| bits(1, u64::from(id == 1024))));
}
