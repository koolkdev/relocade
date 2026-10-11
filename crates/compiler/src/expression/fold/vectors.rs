//! Packed identities and lane observations without changing other vector users.

use super::Folder;
use crate::{body::ValueDefinition, expression::LaneBinaryOp, Expression, Type};

#[derive(Clone, Copy, Eq, PartialEq)]
struct LaneBits {
    start: u8,
    end: u8,
}

impl LaneBits {
    fn new(ty: Type, lane: u8) -> Self {
        let start = ty.bits() * lane;
        Self {
            start,
            end: start + ty.bits(),
        }
    }

    fn contains(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    fn disjoint(self, other: Self) -> bool {
        self.end <= other.start || other.end <= self.start
    }
}

impl Folder<'_> {
    pub(super) fn fold_vector_binary(
        &mut self,
        operator: LaneBinaryOp,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        if left == right {
            match operator {
                LaneBinaryOp::Sub | LaneBinaryOp::GtSigned => {
                    return Some(self.values.literal(Type::V128, 0_u128));
                }
                LaneBinaryOp::Eq => {
                    return Some(self.values.literal(Type::V128, u128::MAX));
                }
                LaneBinaryOp::Add => {}
            }
        }
        let zero = |id: usize| matches!(self.values[id].definition, ValueDefinition::Literal(bits) if u128::from(bits) == 0);
        match operator {
            LaneBinaryOp::Add | LaneBinaryOp::Sub if zero(right) => Some(left),
            LaneBinaryOp::Add if zero(left) => Some(right),
            _ => None,
        }
    }

    pub(super) fn fold_vector_extract(
        &mut self,
        ty: Type,
        original: usize,
        lane: u8,
    ) -> Option<usize> {
        let observed = LaneBits::new(ty, lane);
        let input = self.vector_input(original, observed);
        if let ValueDefinition::Expression(Expression::VectorReplace {
            value,
            lane: replaced,
            ..
        }) = self.values[input].definition
        {
            if self.values[value].ty == ty && replaced == lane {
                // Wasm extracts zero-extend narrow lanes. The inserted scalar
                // can still carry overflow or sign bits in its i32 carrier.
                return Some(self.normalize(value));
            }
        }
        (input != original).then(|| self.fold(ty, Expression::VectorExtract { input, lane }))
    }

    /// Bypass insertions only where the observed bits are unchanged. A scalar
    /// round trip must restore the extracted lane to its original bit positions.
    fn vector_input(&self, mut input: usize, observed: LaneBits) -> usize {
        while let ValueDefinition::Expression(Expression::VectorReplace {
            vector,
            value,
            lane,
        }) = self.values[input].definition
        {
            let replaced = LaneBits::new(self.values[value].ty, lane);
            if replaced.disjoint(observed) {
                input = vector;
            } else if let ValueDefinition::Expression(Expression::VectorExtract {
                input: source,
                lane: extracted,
            }) = self.values[value].definition
            {
                if !replaced.contains(observed)
                    || replaced != LaneBits::new(self.values[value].ty, extracted)
                {
                    break;
                }
                input = source;
            } else {
                break;
            }
        }
        input
    }

    pub(super) fn fold_vector_replace(
        &mut self,
        vector: usize,
        value: usize,
        lane: u8,
    ) -> Option<usize> {
        if let ValueDefinition::Expression(Expression::VectorExtract {
            input,
            lane: extracted,
        }) = self.values[value].definition
        {
            if input == vector && extracted == lane {
                return Some(vector);
            }
        }
        if let ValueDefinition::Expression(Expression::VectorReplace {
            vector: original,
            value: previous,
            lane: replaced,
        }) = self.values[vector].definition
        {
            if LaneBits::new(self.values[value].ty, lane)
                .contains(LaneBits::new(self.values[previous].ty, replaced))
            {
                return Some(self.fold(
                    Type::V128,
                    Expression::VectorReplace {
                        vector: original,
                        value,
                        lane,
                    },
                ));
            }
        }
        None
    }
}
