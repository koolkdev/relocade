//! Reclaim unused storage after placement has released its value-ID caches.
use super::{
    super::{BlockItem, EffectId, Exit, FunctionGraph},
    liveness::Retained,
};

/// Retained definitions and final indices for all references, including replacements.
pub(in crate::body) struct Remapping {
    pub(in crate::body) retained_values: Vec<bool>,
    values: Vec<Option<usize>>,
    effects: Vec<Option<usize>>,
}

impl Remapping {
    fn new(retained: Retained) -> Self {
        fn indices(retained: impl IntoIterator<Item = bool>) -> Vec<Option<usize>> {
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
            values: indices(retained.values.iter().copied()),
            retained_values: retained.values,
            effects: indices(retained.effects),
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

impl FunctionGraph {
    /// Redirect uses and reclaim scalar definitions removed from the schedule.
    /// Each replacement names its final surviving value; unchanged IDs name themselves.
    /// The already compacted graph keeps its effects and control flow.
    pub(crate) fn replace_values(&mut self, replacements: Vec<usize>) {
        let mut remapping = Remapping::new(Retained {
            values: replacements
                .iter()
                .enumerate()
                .map(|(id, &replacement)| id == replacement)
                .collect(),
            effects: vec![true; self.effects.len()],
        });
        // Final representatives retain their own index throughout this rewrite.
        for (id, replacement) in replacements.into_iter().enumerate() {
            remapping.values[id] = remapping.values[replacement];
        }
        remap(self, remapping);
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

    remap(graph, Remapping::new(retained));
}

fn remap(graph: &mut FunctionGraph, remapping: Remapping) {
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
