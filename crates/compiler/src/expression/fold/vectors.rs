//! Exact packed identities preserve every result bit, including narrow carriers.

use super::Folder;
use crate::expression::BinaryOp as Binary;
use crate::{body::ValueDefinition, expression::LaneBinaryOp, Expression, Type};

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
                _ => {}
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
        input: usize,
        lane: u8,
    ) -> Option<usize> {
        match self.values[input].definition {
            ValueDefinition::Expression(Expression::Binary {
                operator:
                    Binary::ReplaceLane {
                        lane_type,
                        lane: replaced,
                    },
                right: value,
                ..
            }) if lane_type == ty && replaced == lane => {
                // Extraction zero-extends a narrow lane; the inserted scalar
                // may still carry overflow or sign bits above its logical type.
                Some(self.fold_low_bits(ty, value, ty.bits()))
            }
            _ => None,
        }
    }
}
