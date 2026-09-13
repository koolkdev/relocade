//! Binary folding and a shared representation for modular constant offsets.

use super::ValueArena;
use crate::{
    integer::{self, BinaryOp},
    Value, ValueKind,
};

impl ValueArena {
    pub(super) fn binary(&mut self, operator: BinaryOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let (ValueKind::Constant(a), ValueKind::Constant(b)) = (a.kind, b.kind) {
            if let Some(bits) = integer::binary(self.values[left].ty, operator, a, b) {
                return self.constant(self.values[left].ty, bits);
            }
        }
        match (operator, a.kind, b.kind) {
            (BinaryOp::Add, _, ValueKind::Constant(offset)) => self.add_constant(left, offset),
            (BinaryOp::Add, ValueKind::Constant(offset), _) => self.add_constant(right, offset),
            (BinaryOp::Sub, _, ValueKind::Constant(offset)) => {
                self.add_constant(left, a.ty.normalize(0u64.wrapping_sub(offset)))
            }
            (BinaryOp::Or | BinaryOp::Xor, _, ValueKind::Constant(0)) => left,
            (BinaryOp::Sub | BinaryOp::Xor, _, _) if left == right => self.constant(a.ty, 0),
            (BinaryOp::Or | BinaryOp::Xor, ValueKind::Constant(0), _) => right,
            (BinaryOp::Mul, _, ValueKind::Constant(1)) => left,
            (BinaryOp::Mul, ValueKind::Constant(1), _) => right,
            (BinaryOp::And | BinaryOp::Or, _, _) if left == right => left,
            (BinaryOp::And, _, ValueKind::Constant(bits)) if bits == a.ty.mask() => left,
            (BinaryOp::And, ValueKind::Constant(bits), _) if bits == a.ty.mask() => right,
            (BinaryOp::And | BinaryOp::Mul, _, ValueKind::Constant(0))
            | (BinaryOp::And | BinaryOp::Mul, ValueKind::Constant(0), _) => self.constant(a.ty, 0),
            (BinaryOp::Or, _, ValueKind::Constant(bits)) if bits == a.ty.mask() => right,
            (BinaryOp::Or, ValueKind::Constant(bits), _) if bits == a.ty.mask() => left,
            _ => {
                let (left, right) = match operator {
                    BinaryOp::DivUnsigned | BinaryOp::RemUnsigned => {
                        (self.normalize(left), self.normalize(right))
                    }
                    BinaryOp::DivSigned | BinaryOp::RemSigned => (
                        self.sign_extend_carrier(left),
                        self.sign_extend_carrier(right),
                    ),
                    _ => (left, right),
                };
                self.intern(Value {
                    ty: a.ty,
                    kind: ValueKind::Binary(operator, left, right),
                })
            }
        }
    }

    fn add_constant(&mut self, mut input: usize, mut offset: u64) -> usize {
        let ty = self.values[input].ty;
        // Combine only consecutive offsets. Conversions and other operations
        // remain boundaries, and offsets wrap at the logical integer width.
        while let ValueKind::Binary(BinaryOp::Add, base, constant) = self.values[input].kind {
            let ValueKind::Constant(previous) = self.values[constant].kind else {
                break;
            };
            input = base;
            offset = ty.normalize(offset.wrapping_add(previous));
        }
        if offset == 0 {
            return input;
        }
        let constant = self.constant(ty, offset);
        self.intern(Value {
            ty,
            kind: ValueKind::Binary(BinaryOp::Add, input, constant),
        })
    }
}
