//! Stored values, deduplication and physical representation facts.
use std::collections::HashMap;

use super::{Value, ValueDefinition};
use crate::{
    integer::{self, BitBounds},
    Expression, Type,
};

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

    pub(crate) fn constant(&mut self, ty: Type, bits: u64) -> usize {
        self.intern(Value {
            ty,
            definition: ValueDefinition::Constant(ty.normalize(bits)),
        })
    }

    pub(crate) fn push(&mut self, value: Value) -> usize {
        let bounds = BitBounds::for_value(value, &self.values, &self.bounds);
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
