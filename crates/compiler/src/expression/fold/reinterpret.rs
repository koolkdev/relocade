//! Local cast factoring and roundtrip cancellation.

use super::Folder;
use crate::{
    body::ValueDefinition,
    expression::{BinaryOp as Binary, UnaryOp as Unary},
    Expression, Type,
};

impl Folder<'_> {
    // Whole-bit operations commute with equal-width integer reinterpretations.
    // Check both inputs before constructing anything: packed inputs should keep
    // their carrier instead of acquiring an extraction and a later insertion.
    pub(super) fn common_reinterpret_type(
        &self,
        target: Type,
        expression: Expression<usize>,
    ) -> Option<Type> {
        let Expression::Binary {
            operator: Binary::Bitwise(_),
            left,
            right,
        } = expression
        else {
            return None;
        };
        let source = self
            .reinterpret_input(left)
            .or_else(|| self.reinterpret_input(right))?;
        let source = self.values[source].ty;
        if !source.is_integer() || source.bits() != target.bits() {
            return None;
        }
        [left, right]
            .into_iter()
            .all(|input| {
                matches!(self.values[input].definition, ValueDefinition::Literal(_))
                    || self
                        .reinterpret_input(input)
                        .is_some_and(|original| self.values[original].ty == source)
            })
            .then_some(source)
    }

    pub(super) fn reinterpret(&mut self, input: usize, target: Type) -> usize {
        // Construction observes logical bits. A V64 roundtrip through I64 may
        // discard the unused high carrier half here, but not during refolding.
        if let Some(original) = self.reinterpret_input(input) {
            if self.values[original].ty == target {
                return original;
            }
        }
        self.fold(
            target,
            Expression::Unary {
                operator: Unary::Reinterpret,
                input,
            },
        )
    }

    pub(super) fn reinterpret_input(&self, input: usize) -> Option<usize> {
        match self.values[input].definition {
            ValueDefinition::Expression(Expression::Unary {
                operator: Unary::Reinterpret,
                input,
            }) => Some(input),
            _ => None,
        }
    }
}
