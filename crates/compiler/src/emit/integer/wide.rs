//! Portable 64-by-64 multiplication with two 64-bit results.
use super::Writer;
use wasm_encoder::{Instruction::*, ValType};

impl Writer<'_> {
    pub(super) fn multiply_wide(&mut self, signed: bool) {
        let left = self.encoder.temporary(ValType::I64);
        let right = self.encoder.temporary(ValType::I64);
        let low_product = self.encoder.temporary(ValType::I64);
        let cross_left = self.encoder.temporary(ValType::I64);
        let cross_right = self.encoder.temporary(ValType::I64);
        self.emit(LocalSet(right));
        self.emit(LocalSet(left));

        // With 32-bit limbs, both cross sums fit in 64 bits:
        // t = a1*b0 + (a0*b0 >> 32), u = a0*b1 + (t & mask).
        self.emit(LocalGet(left));
        self.emit(I64Const(0xffff_ffff));
        self.emit(I64And);
        self.emit(LocalGet(right));
        self.emit(I64Const(0xffff_ffff));
        self.emit(I64And);
        self.emit(I64Mul);
        self.emit(LocalSet(low_product));

        self.emit(LocalGet(left));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(LocalGet(right));
        self.emit(I64Const(0xffff_ffff));
        self.emit(I64And);
        self.emit(I64Mul);
        self.emit(LocalGet(low_product));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(I64Add);
        self.emit(LocalSet(cross_left));

        self.emit(LocalGet(left));
        self.emit(I64Const(0xffff_ffff));
        self.emit(I64And);
        self.emit(LocalGet(right));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(I64Mul);
        self.emit(LocalGet(cross_left));
        self.emit(I64Const(0xffff_ffff));
        self.emit(I64And);
        self.emit(I64Add);
        self.emit(LocalTee(cross_right));

        // Leave the low half below the high half in declared result order.
        self.emit(I64Const(32));
        self.emit(I64Shl);
        self.emit(LocalGet(low_product));
        self.emit(I64Const(0xffff_ffff));
        self.emit(I64And);
        self.emit(I64Or);

        self.emit(LocalGet(left));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(LocalGet(right));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(I64Mul);
        self.emit(LocalGet(cross_left));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(I64Add);
        self.emit(LocalGet(cross_right));
        self.emit(I64Const(32));
        self.emit(I64ShrU);
        self.emit(I64Add);

        if signed {
            // Signed inputs differ from their unsigned bit patterns by 2^64.
            // Their correction therefore changes only the high product half.
            for (sign, other) in [(left, right), (right, left)] {
                self.emit(LocalGet(sign));
                self.emit(I64Const(63));
                self.emit(I64ShrS);
                self.emit(LocalGet(other));
                self.emit(I64And);
                self.emit(I64Sub);
            }
        }
    }
}
