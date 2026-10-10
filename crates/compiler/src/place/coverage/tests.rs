use super::*;

#[test]
fn coverage_queries_do_not_reuse_previous_demands() {
    let successors = [vec![1, 2], vec![3], vec![3], Vec::new()];
    let mut coverage = Coverage::from_successors(successors.to_vec(), 3);
    assert!(coverage.all_paths_reach(0, [1, 2]));
    assert!(coverage.all_paths_reach(0, [2, 1, 1]));
    assert!(!coverage.all_paths_reach(0, [1]));
    assert!(coverage.all_paths_reach(0, [1, 2]));
    assert!(!coverage.all_paths_reach(0, []));
}

#[test]
fn converging_paths_can_reuse_an_explored_tail() {
    let successors = [vec![1, 2], vec![3], vec![3], vec![4], vec![5], Vec::new()];
    let mut coverage = Coverage::from_successors(successors.to_vec(), 5);
    assert!(coverage.all_paths_reach(0, [4]));
}
