//! Reclaim unused storage after placement has released its value-ID caches.
use super::{BlockItem, EffectId, Exit, FunctionGraph, ValueDefinition};

#[cfg(test)]
mod tests;

/// Stable-order indices for the retained values and effects.
pub(super) struct Remapping {
    pub(super) values: Vec<Option<usize>>,
    effects: Vec<Option<usize>>,
}

impl Remapping {
    fn new(values: Vec<bool>, effects: Vec<bool>) -> Self {
        fn indices(retained: Vec<bool>) -> Vec<Option<usize>> {
            let mut next = 0;
            retained
                .into_iter()
                .map(|keep| {
                    keep.then(|| {
                        let index = next;
                        next += 1;
                        index
                    })
                })
                .collect()
        }
        Self {
            values: indices(values),
            effects: indices(effects),
        }
    }

    pub(super) fn value(&self, value: usize) -> usize {
        self.values[value].expect("a retained reference has retained value storage")
    }

    pub(super) fn producer(&self, producer: BlockItem) -> BlockItem {
        match producer {
            BlockItem::Evaluate(value) => BlockItem::Evaluate(self.value(value)),
            BlockItem::Effect(effect) => BlockItem::Effect(EffectId(
                self.effects[effect.0].expect("a retained result has a retained effect"),
            )),
        }
    }
}

impl FunctionGraph {
    /// Reclaim storage after pruning schedules and parameters. All value and
    /// effect references are remapped; block IDs and lexical layout stay stable.
    pub(crate) fn compact(&mut self, mut live_values: Vec<bool>) {
        let reachable = self.reachable();
        for (index, block) in self.blocks.iter_mut().enumerate() {
            if !reachable[index] {
                block.items = Vec::new();
                block.parameters = Vec::new();
                block.exit = Exit::Trap;
                continue;
            }
            let Some(active) = block.exit.constant_edge_index(&self.values) else {
                continue;
            };
            for (index, edge) in block.exit.edges_mut().into_iter().enumerate() {
                if index == active {
                    continue;
                }
                // Inactive edges retain well-formed tuples without keeping the
                // discarded calculations that originally supplied their values.
                for argument in &mut edge.arguments {
                    if !matches!(
                        self.values[*argument].definition,
                        ValueDefinition::Constant(0)
                    ) {
                        let ty = self.values[*argument].ty;
                        *argument = self.values.constant(ty, 0);
                    }
                    live_values.resize(self.values.len(), false);
                    live_values[*argument] = true;
                }
            }
        }

        // Unused entry parameters still belong to the function signature.
        for &parameter in &self.blocks[self.entry.0].parameters {
            live_values[parameter] = true;
        }
        let mut effects = vec![false; self.effects.len()];
        for block in &self.blocks {
            for &item in &block.items {
                if let BlockItem::Effect(effect) = item {
                    effects[effect.0] = true;
                }
                // A retained producer keeps its declared result tuple, even
                // when only a later component is used by the finished graph.
                for result in self.results(item) {
                    live_values[result] = true;
                }
            }
        }

        let remapping = Remapping::new(live_values, effects);
        self.values.compact(&remapping);
        self.effects = std::mem::take(&mut self.effects)
            .into_iter()
            .enumerate()
            .filter_map(|(index, mut effect)| {
                remapping.effects[index]?;
                effect.operation = effect.operation.map_inputs(|value| remapping.value(value));
                for result in &mut effect.results {
                    *result = remapping.value(*result);
                }
                Some(effect)
            })
            .collect();
        self.effects.shrink_to_fit();
        for block in &mut self.blocks {
            for parameter in &mut block.parameters {
                *parameter = remapping.value(*parameter);
            }
            for item in &mut block.items {
                *item = remapping.producer(*item);
            }
            block.exit.map_inputs(|value| remapping.value(value));
        }
    }
}
