//! Value storage, interning and shared expression canonicalization.
//!
//! Construction and placement share expression interning and folding.
//! Handle lifetimes and lexical visibility belong to the builder.
use std::collections::HashMap;

use super::{Value, ValueDefinition};
use crate::{
    integer::{self, BitBounds, BitCountOp},
    Expression, Type,
};

mod arithmetic;
mod bits;
mod comparisons;
mod shifts;

#[derive(Default)]
pub(crate) struct ValueTable {
    pub(crate) values: Vec<Value>,
    pub(crate) bounds: Vec<BitBounds>,
    interned: HashMap<Value, usize>,
}

impl ValueTable {
    /// Restore the carrier promised by construction after learning logical bits.
    pub(crate) fn carrier_bits(&self, id: usize, logical_bits: u64) -> u64 {
        let ty = self.values[id].ty;
        let bits = logical_bits & ty.mask();
        if ty.bits() < 32
            && self.bounds[id].signed <= ty.bits()
            && bits & (1 << (ty.bits() - 1)) != 0
        {
            integer::signed_value(ty, bits) as u64
        } else {
            bits
        }
    }

    /// Conversions within a Wasm carrier preserve all physical bits.
    pub(crate) fn representation(&self, mut id: usize) -> usize {
        while let ValueDefinition::Expression(Expression::Convert { input }) =
            self.values[id].definition
        {
            if (self.values[id].ty == Type::I64) != (self.values[input].ty == Type::I64) {
                break;
            }
            id = input;
        }
        id
    }

    // Inputs carry logical types; canonical operations may retain wider physical bits.
    pub(crate) fn expression(&mut self, ty: Type, expression: Expression<usize>) -> usize {
        match expression {
            Expression::Binary {
                operator,
                left,
                right,
            } => self.binary(operator, left, right),
            Expression::Compare {
                operator,
                left,
                right,
            } => self.compare(operator, left, right),
            Expression::Shift {
                operator,
                value,
                count,
            } => self.shift(operator, value, count),
            Expression::Rotate {
                operator,
                value,
                count,
            } => self.rotate(operator, value, count),
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => self.select(condition, when_true, when_false),
            Expression::SignExtend { input } => self.sign_extend(input, ty),
            Expression::BitCount { operator, input } => self.bit_count(operator, input),
            Expression::ZeroTest { input, nonzero } => self.zero_test(input, nonzero),
            Expression::Convert { input } => self.convert(input, ty),
            Expression::LowBits { input, bits } => self.low_bits(input, bits),
        }
    }

    fn select(&mut self, condition: usize, when_true: usize, when_false: usize) -> usize {
        let condition = self.normalize(condition);
        match self.values[condition].definition {
            ValueDefinition::Constant(0) => return when_false,
            ValueDefinition::Constant(_) => return when_true,
            _ if when_true == when_false => return when_true,
            _ => {}
        }
        self.intern(Value {
            ty: self.values[when_true].ty,
            definition: ValueDefinition::Expression(Expression::Select {
                condition,
                when_true,
                when_false,
            }),
        })
    }

    fn bit_count(&mut self, operator: BitCountOp, input: usize) -> usize {
        let value = self.values[input];
        if let ValueDefinition::Constant(bits) = value.definition {
            return self.constant(value.ty, integer::bit_count(value.ty, operator, bits));
        }
        let input = self.normalize(input);
        self.intern(Value {
            ty: value.ty,
            definition: ValueDefinition::Expression(Expression::BitCount { operator, input }),
        })
    }

    pub(crate) fn constant(&mut self, ty: Type, bits: u64) -> usize {
        self.intern(Value {
            ty,
            definition: ValueDefinition::Constant(ty.normalize(bits)),
        })
    }

    pub(crate) fn push(&mut self, value: Value) -> usize {
        let bounds = BitBounds::for_value(value, &self.values, &self.bounds);
        if bounds.unsigned == 0 && matches!(value.definition, ValueDefinition::Expression(_)) {
            return self.constant(value.ty, 0);
        }
        self.push_with_bounds(value, bounds)
    }

    pub(crate) fn push_with_bounds(&mut self, value: Value, bounds: BitBounds) -> usize {
        let index = self.values.len();
        self.bounds.push(bounds);
        self.values.push(value);
        index
    }

    pub(crate) fn intern(&mut self, value: Value) -> usize {
        if let Some(&index) = self.interned.get(&value) {
            return index;
        }
        let index = self.push(value);
        self.interned.insert(value, index);
        index
    }
}

impl std::ops::Deref for ValueTable {
    type Target = [Value];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
