//! Carrier-preserving semantic reductions valid in the active path context.
use rustc_hash::FxHashMap;

#[derive(Default)]
pub(super) struct Equivalences {
    originals: FxHashMap<usize, Vec<usize>>,
    log: Vec<usize>,
}

impl Equivalences {
    pub(super) fn checkpoint(&self) -> usize {
        self.log.len()
    }

    pub(super) fn restore(&mut self, checkpoint: usize) {
        for residual in self.log.drain(checkpoint..).rev() {
            let originals = self.originals.get_mut(&residual).unwrap();
            originals.pop();
            if originals.is_empty() {
                self.originals.remove(&residual);
            }
        }
    }

    pub(super) fn record(&mut self, recipe: usize, residual: usize) {
        self.originals.entry(residual).or_default().push(recipe);
        self.log.push(residual);
    }

    /// A reduction preserves the full carrier. Logical-bit observations and
    /// partial-demand substitutions must not enter this equivalence relation.
    pub(super) fn recipes(&self, residual: usize) -> Vec<usize> {
        let Some(originals) = self.originals.get(&residual) else {
            return Vec::new();
        };
        let mut recipes = originals.clone();
        let mut index = 0;
        while index < recipes.len() {
            if let Some(originals) = self.originals.get(&recipes[index]) {
                for &recipe in originals {
                    if recipe != residual && !recipes.contains(&recipe) {
                        recipes.push(recipe);
                    }
                }
            }
            index += 1;
        }
        recipes
    }
}
