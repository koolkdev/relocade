//! SSA edits preserve definition locations and rewrite all executable operands.
use super::{BlockId, BlockItem, SsaGraph, ValueDefinition};
use crate::Expression;

impl SsaGraph {
    pub(crate) fn append(&mut self, block: BlockId, item: BlockItem) {
        match item {
            BlockItem::Evaluate(producer) => {
                for result in self.values.expression_results(producer) {
                    self.values.place(result, block);
                }
            }
            BlockItem::Effect(effect) => {
                self.effects[effect.0].origin = block;
                for &result in &self.effects[effect.0].results {
                    self.values.place(result, block);
                }
            }
        }
        self.blocks[block.0].items.push(item);
    }

    pub(crate) fn move_calculation(&mut self, producer: usize, target: BlockId) {
        let source = self
            .values
            .definition_block(producer)
            .expect("a placed calculation");
        if source == target {
            return;
        }
        let item = BlockItem::Evaluate(producer);
        self.blocks[source.0]
            .items
            .retain(|&candidate| candidate != item);
        self.append(target, item);
    }

    pub(crate) fn set_expression(&mut self, producer: usize, expression: Expression<usize>) {
        debug_assert!(matches!(
            self.values[producer].definition,
            ValueDefinition::Expression(_)
        ));
        self.values.values[producer].definition = ValueDefinition::Expression(expression);
    }

    pub(crate) fn rewrite_inputs(
        &mut self,
        mut replacement: impl FnMut(usize) -> usize,
        mut changed: impl FnMut(usize),
    ) {
        for (id, value) in self.values.values.iter_mut().enumerate() {
            if let ValueDefinition::Expression(expression) = value.definition {
                let rewritten = expression.map(|&input| replacement(input));
                if rewritten != expression {
                    value.definition = ValueDefinition::Expression(rewritten);
                    changed(id);
                }
            }
        }
        for effect in &mut self.effects {
            effect.operation = effect.operation.clone().map_inputs(&mut replacement);
        }
        for block in &mut self.blocks {
            block.exit.map_inputs(&mut replacement);
        }
    }
}
