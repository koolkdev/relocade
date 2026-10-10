//! Prove that execution reaches a use before exiting or starting another loop iteration.

use super::dominance::Dominators;

pub(super) struct Coverage {
    successors: Vec<Vec<usize>>,
    exit: usize,
    visited: Vec<usize>,
    required: Vec<usize>,
    generation: usize,
    pending: Vec<usize>,
}

impl Coverage {
    pub(super) fn new(mut successors: Vec<Vec<usize>>, dominators: &Dominators) -> Self {
        let exit = successors.len();
        successors.push(Vec::new());
        for (source, targets) in successors[..exit].iter_mut().enumerate() {
            if dominators.parent[source].is_some()
                && (targets.is_empty()
                    || targets
                        .iter()
                        .any(|&target| dominators.dominates(target, source)))
            {
                targets.push(exit);
            }
        }
        Self::from_successors(successors, exit)
    }

    fn from_successors(successors: Vec<Vec<usize>>, exit: usize) -> Self {
        Self {
            visited: vec![0; successors.len()],
            required: vec![0; successors.len()],
            successors,
            exit,
            generation: 0,
            pending: Vec::new(),
        }
    }

    pub(super) fn postdominators(&self) -> Dominators {
        let mut reversed = vec![Vec::new(); self.successors.len()];
        for (source, targets) in self.successors.iter().enumerate() {
            for &target in targets {
                reversed[target].push(source);
            }
        }
        Dominators::new(self.exit, &reversed, &self.successors)
    }

    // Stop at the first demand on each path. Reuse demand and visit marks between
    // queries; a synthetic exit at every backedge bounds each iteration.
    pub(super) fn all_paths_reach(
        &mut self,
        candidate: usize,
        sites: impl IntoIterator<Item = usize>,
    ) -> bool {
        self.generation += 1;
        for site in sites {
            self.required[site] = self.generation;
        }
        self.pending.clear();
        self.pending.push(candidate);
        while let Some(block) = self.pending.pop() {
            if self.required[block] == self.generation || self.visited[block] == self.generation {
                continue;
            }
            if block == self.exit {
                return false;
            }
            self.visited[block] = self.generation;
            self.pending.extend(&self.successors[block]);
        }
        true
    }
}

#[cfg(test)]
#[path = "coverage/tests.rs"]
mod tests;
