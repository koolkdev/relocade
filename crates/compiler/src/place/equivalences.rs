//! Full-carrier equivalences, scoped by their reduction or lowering owner.
use rustc_hash::FxHashMap;

#[derive(Default)]
pub(super) struct Equivalences {
    originals: FxHashMap<usize, Sources>,
}

// Most residuals have one original recipe. Allocate only when forms converge.
enum Sources {
    One(usize),
    Many(Vec<usize>),
}

impl Equivalences {
    pub(super) fn sources(&self, residual: usize) -> &[usize] {
        match self.originals.get(&residual) {
            Some(Sources::One(recipe)) => std::slice::from_ref(recipe),
            Some(Sources::Many(recipes)) => recipes,
            None => &[],
        }
    }

    pub(super) fn record(&mut self, recipe: usize, residual: usize) {
        self.originals
            .entry(residual)
            .and_modify(|sources| match sources {
                Sources::One(first) => *sources = Sources::Many(vec![*first, recipe]),
                Sources::Many(recipes) => recipes.push(recipe),
            })
            .or_insert(Sources::One(recipe));
    }

    /// A reduction preserves the full carrier. Logical-bit observations and
    /// partial-demand substitutions must not enter this equivalence relation.
    pub(super) fn recipes(&self, residual: usize) -> Vec<usize> {
        let mut recipes = self.sources(residual).to_vec();
        let mut index = 0;
        while index < recipes.len() {
            for &recipe in self.sources(recipes[index]) {
                if recipe != residual && !recipes.contains(&recipe) {
                    recipes.push(recipe);
                }
            }
            index += 1;
        }
        recipes
    }
}
