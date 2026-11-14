//! Share block-end renamings without copying the set of live definitions.
//! A block may update its own nodes; descendants copy only the changed radix path.
use crate::body::BlockId;

#[derive(Clone, Copy, Default)]
pub(super) struct Snapshot(u32);

#[derive(Clone)]
struct Node {
    owner: usize,
    entries: [u32; 16],
    depth: u32,
    predicates: bool,
}

pub(super) struct Versions {
    nodes: Vec<Node>,
}

impl Versions {
    pub(super) fn new() -> Self {
        Self {
            nodes: vec![Node {
                owner: usize::MAX,
                entries: [0; 16],
                depth: 0,
                predicates: false,
            }],
        }
    }

    pub(super) fn value(&self, snapshot: Snapshot, value: usize) -> Option<usize> {
        self.get(snapshot.0, value << 1)
    }

    pub(super) fn truth(&self, snapshot: Snapshot, predicate: usize) -> Option<bool> {
        self.get(snapshot.0, predicate << 1 | 1)
            .map(|truth| truth != 0)
    }

    pub(super) fn with_value(
        &mut self,
        snapshot: Snapshot,
        block: BlockId,
        value: usize,
        replacement: usize,
    ) -> Snapshot {
        Snapshot(self.set(snapshot.0, block.0, value << 1, replacement))
    }

    pub(super) fn with_truth(
        &mut self,
        snapshot: Snapshot,
        block: BlockId,
        predicate: usize,
        truth: bool,
    ) -> Snapshot {
        Snapshot(self.set(snapshot.0, block.0, predicate << 1 | 1, usize::from(truth)))
    }

    fn get(&self, mut root: u32, key: usize) -> Option<usize> {
        let key = u64::try_from(key).unwrap();
        let depth = self.nodes[root as usize].depth;
        if key >> ((depth + 1) * 4) != 0 {
            return None;
        }
        for depth in (0..=depth).rev() {
            if root == 0 {
                return None;
            }
            root = self.nodes[root as usize].entries[((key >> (depth * 4)) & 15) as usize];
        }
        (root != 0).then(|| root as usize - 1)
    }

    fn set(&mut self, mut root: u32, owner: usize, key: usize, value: usize) -> u32 {
        let key = u64::try_from(key).unwrap();
        assert!(key < 1 << 36, "function keys fit 36 bits");
        let depth = (63 - key.max(1).leading_zeros()) / 4;
        let value = u32::try_from(value + 1).expect("function value IDs fit u32");
        while root != 0 && self.nodes[root as usize].depth < depth {
            let mut node = Node {
                owner,
                entries: [0; 16],
                depth: self.nodes[root as usize].depth + 1,
                predicates: self.nodes[root as usize].predicates,
            };
            node.entries[0] = root;
            root = self.push(node);
        }
        let depth = depth.max(self.nodes[root as usize].depth);
        self.write(root, owner, key, value, depth)
    }

    fn push(&mut self, node: Node) -> u32 {
        let index = u32::try_from(self.nodes.len()).expect("function version storage fits u32");
        self.nodes.push(node);
        index
    }

    fn write(&mut self, root: u32, owner: usize, key: u64, value: u32, depth: u32) -> u32 {
        let root = if root == 0 || self.nodes[root as usize].owner != owner {
            let mut node = self.nodes[root as usize].clone();
            node.owner = owner;
            node.depth = depth;
            self.push(node)
        } else {
            root
        };
        let slot = ((key >> (depth * 4)) & 15) as usize;
        let entry = self.nodes[root as usize].entries[slot];
        let entry = if depth == 0 {
            value
        } else {
            self.write(entry, owner, key, value, depth - 1)
        };
        self.nodes[root as usize].entries[slot] = entry;
        self.nodes[root as usize].predicates |= key & 1 != 0;
        root
    }

    pub(super) fn contradict(&self, left: Snapshot, right: Snapshot) -> bool {
        let mut pending = vec![(left.0, right.0)];
        while let Some((left, right)) = pending.pop() {
            if left == right || left == 0 || right == 0 {
                continue;
            }
            let a = &self.nodes[left as usize];
            let b = &self.nodes[right as usize];
            if !a.predicates || !b.predicates {
                continue;
            }
            if a.depth > b.depth {
                pending.push((a.entries[0], right));
                continue;
            }
            if b.depth > a.depth {
                pending.push((left, b.entries[0]));
                continue;
            }
            if a.depth == 0 {
                if (1..16).step_by(2).any(|slot| {
                    a.entries[slot] != 0
                        && b.entries[slot] != 0
                        && a.entries[slot] != b.entries[slot]
                }) {
                    return true;
                }
            } else {
                pending.extend(a.entries.into_iter().zip(b.entries));
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_updates_and_growth_preserve_prior_snapshots() {
        let mut versions = Versions::new();
        let parent = versions.with_value(Snapshot::default(), BlockId(0), 1, 7);
        let a = versions.with_value(parent, BlockId(1), 1, 11);
        let a = versions.with_value(a, BlockId(1), 0x80000, 19);
        let b = versions.with_value(parent, BlockId(2), 2, 23);
        assert_eq!(versions.value(parent, 1), Some(7));
        assert_eq!(versions.value(parent, 0x80000), None);
        assert_eq!(versions.value(a, 1), Some(11));
        assert_eq!(versions.value(a, 0x80000), Some(19));
        assert_eq!(versions.value(a, 2), None);
        assert_eq!(versions.value(b, 1), Some(7));
        assert_eq!(versions.value(b, 2), Some(23));
    }

    #[test]
    fn only_opposing_propositions_exclude_paths() {
        let mut versions = Versions::new();
        let parent = versions.with_truth(Snapshot::default(), BlockId(0), 1, true);
        let a = versions.with_value(parent, BlockId(1), 1, 11);
        let b = versions.with_value(parent, BlockId(2), 1, 23);
        let b = versions.with_truth(b, BlockId(2), 0x80000, false);
        assert_eq!(versions.truth(a, 1), Some(true));
        assert_eq!(versions.value(a, 1), Some(11));
        assert!(!versions.contradict(a, b));
        let c = versions.with_truth(b, BlockId(3), 1, false);
        assert!(versions.contradict(a, c));
        assert!(versions.contradict(c, a));
        assert!(!versions.contradict(a, Snapshot::default()));
    }
}
