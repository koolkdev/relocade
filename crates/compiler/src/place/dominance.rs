/// Immediate dominators in reverse postorder. Missing entries are unreachable
/// from the chosen root, including blocks with no path to the synthetic exit.
pub(super) struct Dominators {
    pub(super) parent: Vec<Option<usize>>,
    pub(super) rank: Vec<usize>,
    interval: Vec<(usize, usize)>,
    ancestors: Vec<Vec<usize>>,
}
impl Dominators {
    pub(super) fn new(root: usize, successors: &[Vec<usize>], predecessors: &[Vec<usize>]) -> Self {
        let mut seen = vec![false; successors.len()];
        let mut postorder = Vec::new();
        let mut work = vec![(root, 0)];
        seen[root] = true;
        while let Some((node, next)) = work.pop() {
            if next == successors[node].len() {
                postorder.push(node);
                continue;
            }
            work.push((node, next + 1));
            let successor = successors[node][next];
            if !std::mem::replace(&mut seen[successor], true) {
                work.push((successor, 0));
            }
        }
        postorder.reverse();
        let mut rank = vec![usize::MAX; successors.len()];
        for (position, &node) in postorder.iter().enumerate() {
            rank[node] = position;
        }
        let mut tree = Self {
            parent: vec![None; successors.len()],
            rank,
            interval: Vec::new(),
            ancestors: Vec::new(),
        };
        tree.parent[root] = Some(root);
        loop {
            let mut changed = false;
            for &node in &postorder[1..] {
                let mut common = None;
                for &predecessor in &predecessors[node] {
                    if tree.parent[predecessor].is_some() {
                        common = Some(match common {
                            None => predecessor,
                            Some(previous) => tree.common(previous, predecessor).unwrap(),
                        });
                    }
                }
                if tree.parent[node] != common {
                    tree.parent[node] = common;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        tree.index();
        tree
    }
    fn index(&mut self) {
        let count = self.parent.len();
        let mut children = vec![Vec::new(); count];
        let mut root = None;
        for (node, &parent) in self.parent.iter().enumerate() {
            if let Some(parent) = parent {
                if parent == node {
                    root = Some(node);
                } else {
                    children[parent].push(node);
                }
            }
        }
        self.interval = vec![(usize::MAX, usize::MAX); count];
        let mut work = vec![(root.unwrap(), false)];
        let mut clock = 0;
        while let Some((node, leaving)) = work.pop() {
            if leaving {
                self.interval[node].1 = clock;
            } else {
                self.interval[node].0 = clock;
                clock += 1;
                work.push((node, true));
                work.extend(children[node].iter().rev().map(|&child| (child, false)));
            }
        }
        self.ancestors.push(
            self.parent
                .iter()
                .enumerate()
                .map(|(node, parent)| parent.unwrap_or(node))
                .collect(),
        );
        while (1usize << self.ancestors.len()) < count {
            let previous = self.ancestors.last().unwrap();
            let next = previous.iter().map(|&parent| previous[parent]).collect();
            self.ancestors.push(next);
        }
    }
    fn encloses(&self, a: usize, b: usize) -> bool {
        self.interval[a].0 <= self.interval[b].0 && self.interval[b].0 < self.interval[a].1
    }
    pub(super) fn common(&self, mut a: usize, mut b: usize) -> Option<usize> {
        self.parent[a]?;
        self.parent[b]?;
        if self.ancestors.is_empty() {
            // Cooper's construction still walks its provisional parent tree.
            while a != b {
                if self.rank[a] > self.rank[b] {
                    a = self.parent[a]?;
                } else {
                    b = self.parent[b]?;
                }
            }
            return Some(a);
        }
        if self.encloses(a, b) {
            return Some(a);
        }
        if self.encloses(b, a) {
            return Some(b);
        }
        for ancestors in self.ancestors.iter().rev() {
            let ancestor = ancestors[a];
            if !self.encloses(ancestor, b) {
                a = ancestor;
            }
        }
        self.parent[a]
    }
    pub(super) fn dominates(&self, a: usize, b: usize) -> bool {
        self.parent[a].is_some() && self.parent[b].is_some() && self.encloses(a, b)
    }
}
