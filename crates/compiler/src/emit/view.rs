//! Operand-stack coverage references the placed graph rather than copying code.
use super::selection::Selection;
use crate::body::{BlockItem, Exit, FunctionGraph};

pub(super) struct OperandView {
    pub(super) producer: Vec<Option<BlockItem>>,
    inline_values: Vec<bool>,
    inline_effects: Vec<bool>,
    pub(super) uses: Vec<usize>,
}
impl OperandView {
    pub(super) fn new(graph: &FunctionGraph, selection: &Selection, reachable: &[bool]) -> Self {
        let mut this = Self {
            producer: vec![None; graph.values.len()],
            inline_values: vec![false; graph.values.len()],
            inline_effects: vec![false; graph.effects.len()],
            uses: vec![0; graph.values.len()],
        };
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            for &item in &block.items {
                if !selection.enabled(item) {
                    continue;
                }
                for result in selection.results(graph, &item) {
                    this.producer[result] = Some(item);
                }
                for input in graph.inputs(item) {
                    this.uses[selection.resolve(input)] += 1;
                }
            }
            match &block.exit {
                Exit::If {
                    condition,
                    taken,
                    otherwise,
                } => {
                    let (condition, _) = selection.condition(graph, *condition, false);
                    this.uses[condition] += 1;
                    for &input in taken.arguments.iter().chain(&otherwise.arguments) {
                        this.uses[selection.resolve(input)] += 1;
                    }
                }
                _ => {
                    for input in block.exit.inputs() {
                        this.uses[selection.resolve(input)] += 1;
                    }
                }
            }
        }
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            let items: Vec<_> = block
                .items
                .iter()
                .copied()
                .filter(|&item| selection.enabled(item))
                .collect();
            let mut cursor = items.len();
            let exit_inputs: Vec<_> = match &block.exit {
                Exit::If { condition, .. } => vec![selection.condition(graph, *condition, false).0],
                Exit::Switch { selector, .. } => vec![*selector],
                _ => block.exit.inputs(),
            };
            this.cover(
                graph,
                selection,
                &items,
                &mut cursor,
                exit_inputs.into_iter(),
            );
            for position in (0..items.len()).rev() {
                let item = items[position];
                if this.inline(item) {
                    continue;
                }
                let mut cursor = position;
                this.cover(graph, selection, &items, &mut cursor, graph.inputs(item));
            }
        }
        this
    }
    pub(super) fn inline(&self, item: BlockItem) -> bool {
        match item {
            BlockItem::Evaluate(value) => self.inline_values[value],
            BlockItem::Effect(effect) => self.inline_effects[effect.0],
        }
    }
    fn cover(
        &mut self,
        graph: &FunctionGraph,
        selection: &Selection,
        items: &[BlockItem],
        cursor: &mut usize,
        operands: impl Iterator<Item = usize>,
    ) {
        let mut pending: Vec<_> = operands.collect();
        while let Some(input) = pending.pop() {
            let input = selection.resolve(input);
            let Some(producer) = self.producer[input] else {
                continue;
            };
            if *cursor == 0
                || items[*cursor - 1] != producer
                || self.uses[input] != 1
                || selection.results(graph, &producer).count() != 1
            {
                continue;
            }
            match producer {
                BlockItem::Evaluate(value) => self.inline_values[value] = true,
                BlockItem::Effect(effect) => self.inline_effects[effect.0] = true,
            }
            *cursor -= 1;
            pending.extend(graph.inputs(producer));
        }
    }
}
