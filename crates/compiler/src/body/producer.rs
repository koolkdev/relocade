//! Operation producers and their ordered input and result views.

use super::{BlockId, FunctionGraph, Operation, ValueDefinition};
use crate::Expression;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct EffectId(pub(crate) usize);

/// A producer handle; placing it in a block schedules that execution.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum BlockItem {
    Evaluate(usize),
    Effect(EffectId),
}

pub(crate) struct Effect {
    pub(crate) results: Vec<usize>,
    pub(crate) operation: Operation,
    pub(crate) origin: BlockId,
}

impl FunctionGraph {
    /// Find the defining operation, independently of whether it has been placed.
    /// Constants and block parameters have no operation producer.
    pub(crate) fn producer_of(&self, value: usize) -> Option<BlockItem> {
        match self.values[value].definition {
            ValueDefinition::Expression(_) => Some(BlockItem::Evaluate(value)),
            ValueDefinition::Result { effect, .. } => Some(BlockItem::Effect(effect)),
            ValueDefinition::Constant(_) | ValueDefinition::Parameter { .. } => None,
        }
    }

    /// Inputs in execution and Wasm stack order, without copying their storage.
    pub(crate) fn inputs(
        &self,
        producer: BlockItem,
    ) -> impl DoubleEndedIterator<Item = usize> + '_ {
        let (expression, operation) = match producer {
            BlockItem::Evaluate(value) => {
                let ValueDefinition::Expression(expression) = &self.values[value].definition else {
                    panic!("evaluate names an expression")
                };
                (Some(expression), None)
            }
            BlockItem::Effect(effect) => (None, Some(&self.effects[effect.0].operation)),
        };
        expression
            .into_iter()
            .flat_map(Expression::inputs)
            .copied()
            .chain(operation.into_iter().flat_map(Operation::inputs))
    }

    /// Results in their declared order, before instruction selection covers them.
    pub(crate) fn results<'a>(&'a self, producer: &'a BlockItem) -> &'a [usize] {
        match producer {
            BlockItem::Evaluate(value) => std::slice::from_ref(value),
            BlockItem::Effect(effect) => &self.effects[effect.0].results,
        }
    }
}

#[cfg(test)]
mod tests;
