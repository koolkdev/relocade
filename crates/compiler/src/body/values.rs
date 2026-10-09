//! Stored values, deduplication and physical representation facts.
use crate::literal::Literal;
use hashbrown::HashTable;
use std::{collections::hash_map::RandomState, hash::BuildHasher};

use super::{BlockItem, Value, ValueDefinition};
use crate::{integer, Expression, Type};

mod bounds;
pub(crate) use bounds::BitBounds;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct ValueTable {
    pub(crate) values: Vec<Value>,
    pub(crate) bounds: Vec<BitBounds>,
    // Canonical keys live in `values` and stay unchanged until compaction.
    interned: HashTable<usize>,
    hash_builder: RandomState,
}

/// One component of a pure expression, whether stored inline or as a projection.
#[derive(Clone, Copy)]
pub(crate) struct ExpressionResult {
    pub(crate) producer: usize,
    pub(crate) expression: Expression<usize>,
    pub(crate) component: usize,
}

impl ValueTable {
    pub(super) fn compact(&mut self, remapping: &super::prune::Remapping) {
        // No more construction or folding follows finalization. Release the
        // lookup table before changing the values and IDs that supply its keys.
        self.interned = HashTable::new();
        let mut retained = remapping.values.iter();
        self.values.retain_mut(|value| {
            if retained.next().unwrap().is_none() {
                return false;
            }
            value.definition = match value.definition {
                ValueDefinition::Expression(expression) => {
                    ValueDefinition::Expression(expression.map(|&input| remapping.value(input)))
                }
                ValueDefinition::Result {
                    producer,
                    component,
                } => ValueDefinition::Result {
                    producer: remapping.producer(producer),
                    component,
                },
                definition => definition,
            };
            true
        });
        let mut retained = remapping.values.iter();
        self.bounds.retain(|_| retained.next().unwrap().is_some());
        self.values.shrink_to_fit();
        self.bounds.shrink_to_fit();
    }

    pub(crate) fn expression(&self, id: usize) -> Option<ExpressionResult> {
        let (producer, component) = match self.values[id].definition {
            ValueDefinition::Expression(_) => (id, 0),
            ValueDefinition::Result {
                producer: BlockItem::Evaluate(producer),
                component,
            } => (producer, component),
            _ => return None,
        };
        let ValueDefinition::Expression(expression) = self.values[producer].definition else {
            panic!("a pure result names its expression producer")
        };
        Some(ExpressionResult {
            producer,
            expression,
            component,
        })
    }

    /// Pure results are allocated together, with the expression in the first slot.
    pub(crate) fn expression_results(&self, producer: usize) -> std::ops::Range<usize> {
        let ValueDefinition::Expression(expression) = self.values[producer].definition else {
            panic!("a pure producer stores its expression")
        };
        producer..producer + expression.result_types(self.values[producer].ty).len()
    }

    pub(crate) fn expression_result(&self, producer: usize, component: usize) -> usize {
        self.expression_results(producer)
            .nth(component)
            .expect("the expression declares this result")
    }

    /// Restore the carrier promised by construction after learning logical bits.
    /// Literal values already contain the exact carrier and keep those bits.
    /// Logical bits alone cannot reconstruct an unknown wider physical value.
    pub(crate) fn carrier_bits(&self, id: usize, logical_bits: u64) -> Option<u64> {
        let ty = self.values[id].ty;
        if let Some(bits) = self.values[id].scalar_literal() {
            return Some(ty.carrier().normalize(bits));
        }
        if self.bounds[id].unsigned > ty.bits() && self.bounds[id].signed > ty.bits() {
            return None;
        }
        let bits = logical_bits & ty.mask();
        let bits = if ty.bits() < 32
            && self.bounds[id].signed <= ty.bits()
            && bits & (1 << (ty.bits() - 1)) != 0
        {
            integer::signed_value(ty, bits) as u64
        } else {
            bits
        };
        Some(ty.carrier().normalize(bits))
    }

    /// Conversions within a Wasm carrier preserve all physical bits.
    pub(crate) fn representation(&self, mut id: usize) -> usize {
        while let ValueDefinition::Expression(Expression::Convert { input }) =
            self.values[id].definition
        {
            if self.values[id].ty.carrier() != self.values[input].ty.carrier() {
                break;
            }
            id = input;
        }
        id
    }

    pub(crate) fn literal(&mut self, ty: Type, bits: impl Into<Literal>) -> usize {
        self.carrier_literal(ty, bits.into().normalize(ty))
    }

    /// Store a lowered result without discarding bits above its logical width.
    pub(crate) fn carrier_literal(&mut self, ty: Type, bits: impl Into<Literal>) -> usize {
        self.intern(Value {
            ty,
            definition: ValueDefinition::Literal(bits.into().normalize(ty.carrier())),
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
        if let ValueDefinition::Expression(expression) = value.definition {
            for (component, ty) in expression.result_types(value.ty).enumerate().skip(1) {
                self.push(Value {
                    ty,
                    definition: ValueDefinition::Result {
                        producer: BlockItem::Evaluate(index),
                        component,
                    },
                });
            }
        }
        index
    }

    pub(crate) fn intern(&mut self, value: Value) -> usize {
        let hash = self.hash_builder.hash_one(value);
        if let Some(&index) = self
            .interned
            .find(hash, |&index| self.values[index] == value)
        {
            return index;
        }
        let index = self.push(value);
        self.interned.insert_unique(hash, index, |&index| {
            self.hash_builder.hash_one(self.values[index])
        });
        index
    }
}

impl std::ops::Deref for ValueTable {
    type Target = [Value];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
