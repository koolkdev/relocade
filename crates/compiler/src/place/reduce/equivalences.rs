//! Full-carrier aliases grouped by their stable normal form in one reduction context.
//! Logical-bit observations and partial-demand substitutions are not equivalences.
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
    pub(super) fn recipes(&self, residual: usize) -> &[usize] {
        match self.originals.get(&residual) {
            Some(Sources::One(recipe)) => std::slice::from_ref(recipe),
            Some(Sources::Many(recipes)) => recipes,
            None => &[],
        }
    }

    /// The reducer finishes the residual before recording an alias. A recipe
    /// with aliases is already a stable normal form and cannot be reduced again.
    pub(super) fn record(&mut self, recipe: usize, residual: usize) {
        debug_assert!(self.recipes(recipe).is_empty());
        self.originals
            .entry(residual)
            .and_modify(|sources| match sources {
                Sources::One(first) => *sources = Sources::Many(vec![*first, recipe]),
                Sources::Many(recipes) => recipes.push(recipe),
            })
            .or_insert(Sources::One(recipe));
    }
}
