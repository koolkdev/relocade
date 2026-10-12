//! Lower semantic recipes once, before their primitive dependencies are scheduled.
use super::equivalences::Equivalences;
use super::*;
use rustc_hash::FxHashMap;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Lowering {
    next: usize,
    replacements: FxHashMap<usize, usize>,
    originals: Equivalences,
}

impl Lowering {
    /// Values are appended in dependency order. Only changed recipes need a
    /// mapping; emitted primitives use the same value table as semantic recipes.
    pub(super) fn prepare(&mut self, values: &mut ValueTable) {
        let end = values.len();
        for id in self.next..end {
            let Some(original) = values.expression(id) else {
                continue;
            };
            if self.replacements.is_empty() && !original.expression.needs_lowering() {
                continue;
            }
            let expression = original.expression.map(|&input| self.value(input));
            let rewritten = if expression == original.expression {
                id
            } else {
                crate::expression::fold_carrier(
                    values,
                    values[id].ty,
                    expression,
                    original.component,
                )
            };
            let lowered = crate::expression::lower(values, rewritten);
            if lowered != rewritten {
                self.replacements.insert(rewritten, lowered);
                self.originals.record(rewritten, lowered);
            }
            if lowered != id {
                self.replacements.insert(id, lowered);
                if id != rewritten {
                    self.originals.record(id, lowered);
                }
            }
        }
        // Expansion constructs primitives with already-lowered inputs.
        self.next = values.len();
    }

    pub(super) fn value(&self, id: usize) -> usize {
        self.replacements.get(&id).copied().unwrap_or(id)
    }

    /// Semantic and primitive forms both name the same complete carrier. Keep
    /// both available to join reuse, but only live primitive forms create demand.
    pub(super) fn recipes(&self, reducer: &Reducer, value: usize) -> Vec<usize> {
        let mut recipes = Vec::new();
        let mut recipe = value;
        let mut index = 0;
        loop {
            for alternative in reducer
                .originals(recipe)
                .iter()
                .chain(self.originals.sources(recipe))
                .chain(self.replacements.get(&recipe))
                .copied()
            {
                if alternative != value && !recipes.contains(&alternative) {
                    recipes.push(alternative);
                }
            }
            let Some(&next) = recipes.get(index) else {
                break;
            };
            recipe = next;
            index += 1;
        }
        recipes
    }
}
