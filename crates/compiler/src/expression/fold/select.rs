//! Value selection keeps its condition's truth distinct from its numeric bits.

use super::Folder;
use crate::expression::{BinaryOp as Binary, TernaryOp as Ternary, UnaryOp as Unary};
use crate::{bitwise::BitwiseOp, body::ValueDefinition};
use crate::{Expression, Type};

impl Folder<'_> {
    /// A selection disappears when both arms agree on every observed bit.
    pub(super) fn fold_masked_select(
        &mut self,
        ty: Type,
        input: usize,
        mask: u64,
    ) -> Option<usize> {
        let ValueDefinition::Expression(Expression::Ternary {
            operator: Ternary::Select,
            first,
            second,
            ..
        }) = self.values[self.values.representation(input)].definition
        else {
            return None;
        };
        let mask = self.values.carrier_literal(ty, mask);
        let mut masked = |input| {
            self.fold(
                ty,
                Expression::Binary {
                    operator: Binary::Bitwise(BitwiseOp::And),
                    left: input,
                    right: mask,
                },
            )
        };
        let first = masked(first);
        let second = masked(second);
        (self.values.representation(first) == self.values.representation(second)).then_some(first)
    }

    pub(super) fn select(
        &mut self,
        condition: usize,
        when_true: usize,
        when_false: usize,
    ) -> usize {
        let condition = self.normalize(condition);
        self.fold(
            self.values[when_true].ty,
            Expression::Ternary {
                operator: Ternary::Select,
                first: when_true,
                second: when_false,
                third: condition,
            },
        )
    }

    pub(super) fn fold_select(
        &mut self,
        condition: usize,
        when_true: usize,
        when_false: usize,
    ) -> Option<usize> {
        match self.values[condition].scalar_literal() {
            Some(0) => return Some(when_false),
            Some(_) => return Some(when_true),
            _ => {}
        }
        if self.values.representation(when_true) == self.values.representation(when_false) {
            return Some(when_true);
        }
        if !self.values[when_true].ty.is_integer() {
            return None;
        }
        let nonzero = match (
            self.values[when_true].scalar_literal(),
            self.values[when_false].scalar_literal(),
        ) {
            (Some(1), Some(0)) => true,
            (Some(0), Some(1)) => false,
            _ => return None,
        };
        // A truth consumer accepts any nonzero carrier. Numeric selection must
        // still produce exactly zero or one, including during operand refolding.
        Some(self.fold(
            Type::I1,
            Expression::Unary {
                operator: Unary::ZeroTest { nonzero },
                input: condition,
            },
        ))
    }
}
