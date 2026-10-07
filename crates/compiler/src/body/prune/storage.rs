//! Reclaim unused storage after placement has released its value-ID caches.
use super::{
    super::{BlockItem, EffectId, Exit, FunctionGraph},
    liveness::Retained,
};

/// Stable-order indices for the retained values and effects.
pub(in crate::body) struct Remapping {
    pub(in crate::body) values: Vec<Option<usize>>,
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

    pub(in crate::body) fn value(&self, value: usize) -> usize {
        self.values[value].expect("a retained reference has retained value storage")
    }

    pub(in crate::body) fn producer(&self, producer: BlockItem) -> BlockItem {
        match producer {
            BlockItem::Evaluate(value) => BlockItem::Evaluate(self.value(value)),
            BlockItem::Effect(effect) => BlockItem::Effect(EffectId(
                self.effects[effect.0].expect("a retained result has a retained effect"),
            )),
        }
    }
}

pub(super) fn compact(graph: &mut FunctionGraph, reachable: &[bool], mut retained: Retained) {
    for (index, block) in graph.blocks.iter_mut().enumerate() {
        if !reachable[index] {
            block.items = Vec::new();
            block.parameters = Vec::new();
            block.exit = Exit::Trap;
        }
    }

    // Unused entry parameters still belong to the function signature.
    for &parameter in &graph.blocks[graph.entry.0].parameters {
        retained.values[parameter] = true;
    }
    for block in &graph.blocks {
        for &item in &block.items {
            // A retained producer keeps its declared result tuple, even
            // when only a later component is used by the finished graph.
            for result in graph.results(item) {
                retained.values[result] = true;
            }
        }
    }

    let remapping = Remapping::new(retained.values, retained.effects);
    graph.values.compact(&remapping);
    graph.effects = std::mem::take(&mut graph.effects)
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
    graph.effects.shrink_to_fit();
    for block in &mut graph.blocks {
        for parameter in &mut block.parameters {
            *parameter = remapping.value(*parameter);
        }
        for item in &mut block.items {
            *item = remapping.producer(*item);
        }
        block.exit.map_inputs(|value| remapping.value(value));
    }
}
