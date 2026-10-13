//! Packed identities at construction and during carrier-preserving refolding.

use super::Folder;
use crate::{
    expression::{BinaryOp, LaneBinaryOp},
    Expression, Type,
};

fn lane_ones(lane_type: Type) -> u128 {
    match lane_type {
        Type::I16 => 0x0001_0001_0001_0001_0001_0001_0001_0001,
        Type::I32 => 0x0000_0001_0000_0001_0000_0001_0000_0001,
        Type::I64 => 0x0000_0000_0000_0001_0000_0000_0000_0001,
        _ => unreachable!("packed multiplication uses word or wider lanes"),
    }
}

impl Folder<'_> {
    pub(super) fn vector(
        &mut self,
        ty: Type,
        operator: LaneBinaryOp,
        lane_type: Type,
        left: usize,
        right: usize,
    ) -> usize {
        // Construction may ignore V64's upper lanes; refolding must retain them.
        if ty == Type::V64 && operator == LaneBinaryOp::Mul {
            let ones = u128::from(lane_ones(lane_type) as u64);
            for (constant, input) in [(left, right), (right, left)] {
                if self.vector_literal(constant) == Some(ones) {
                    return input;
                }
            }
        }
        self.fold(
            ty,
            Expression::Binary {
                operator: BinaryOp::Vector {
                    operator,
                    lane_type,
                },
                left,
                right,
            },
        )
    }

    pub(super) fn fold_vector(
        &mut self,
        ty: Type,
        operator: LaneBinaryOp,
        lane_type: Type,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        use LaneBinaryOp::*;
        let a = self.values.representation(left);
        let b = self.values.representation(right);
        if a == b {
            match operator {
                Sub | SubSatSigned | SubSatUnsigned | GtSigned => {
                    return Some(self.values.literal(ty, 0));
                }
                // Even V64 equality sets the unused upper carrier lanes.
                Eq => return Some(self.values.carrier_literal(ty, u128::MAX)),
                MinSigned | MinUnsigned | MaxSigned | MaxUnsigned | AverageUnsigned => {
                    return Some(left);
                }
                _ => {}
            }
        }
        let x = self.vector_literal(a);
        let y = self.vector_literal(b);
        match operator {
            Add | AddSatSigned | AddSatUnsigned if x == Some(0) => Some(right),
            Add | AddSatSigned | AddSatUnsigned | Sub | SubSatSigned | SubSatUnsigned
                if y == Some(0) =>
            {
                Some(left)
            }
            Mul | WideningMulLowSigned | WideningMulHighSigned | DotI16Signed
                if x == Some(0) || y == Some(0) =>
            {
                Some(self.values.literal(ty, 0))
            }
            Mul if x.is_some() || y.is_some() => {
                let ones = lane_ones(lane_type);
                if x == Some(ones) {
                    Some(right)
                } else if y == Some(ones) {
                    Some(left)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub(super) fn fold_lane_shift(
        &self,
        value: usize,
        count: usize,
        lane_type: Type,
    ) -> Option<usize> {
        let input = self.values.representation(value);
        let count = self.values[self.values.representation(count)].scalar_literal();
        if self.vector_literal(input) == Some(0)
            || count.is_some_and(|count| count % u64::from(lane_type.bits()) == 0)
        {
            Some(value)
        } else {
            None
        }
    }
}
