//! Carrier-preserving semantic reductions valid in the active path context.
use rustc_hash::FxHashMap;

#[derive(Default)]
pub(super) struct Equivalences {
    originals: FxHashMap<usize, Vec<usize>>,
}

impl Equivalences {
    pub(super) fn record(&mut self, recipe: usize, residual: usize) {
        self.originals.entry(residual).or_default().push(recipe);
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
