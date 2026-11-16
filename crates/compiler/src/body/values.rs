//! Stored values, deduplication and physical representation facts.
use crate::expression::UnaryOp as Unary;
use crate::literal::Literal;
use hashbrown::HashTable;
use rustc_hash::FxBuildHasher;
use std::hash::BuildHasher;

use super::{BlockId, BlockItem, Value, ValueDefinition};
use crate::{Expression, Type};

mod bits;
mod bounds;
mod definitions;
pub(crate) use bits::{infer_bits, initial_bits, Bits};
pub(crate) use bounds::BitBounds;
pub(crate) use definitions::Definitions;
#[cfg(test)]
mod tests;

/// Construction shares immutable recipes; SSA gives each calculation its own definition.
pub(crate) trait Phase: Default {
    const INTERN_EXPRESSIONS: bool;
    fn record(&mut self, _index: usize, _value: Value) {}
    fn compact(&mut self, _retained: &[Option<usize>]) {}
}

#[derive(Default)]
pub(crate) struct Recipes {
    // Allocated only when construction introduces the first explicit fact.
    refined: Vec<bool>,
}
impl Phase for Recipes {
    const INTERN_EXPRESSIONS: bool = true;

    fn record(&mut self, index: usize, value: Value) {
        if self.refined.is_empty() {
            if !matches!(
                value.definition,
                ValueDefinition::RefinedAlias {
                    assumption: Some(_),
                    ..
                }
            ) {
                return;
            }
            self.refined.resize(index, false);
        }
        let refined = match value.definition {
            ValueDefinition::RefinedAlias {
                source, assumption, ..
            } => assumption.is_some() || self.refined[source],
            ValueDefinition::Expression(expression) => {
                expression.inputs().any(|&input| self.refined[input])
            }
            ValueDefinition::Result {
                producer: BlockItem::Evaluate(producer),
                ..
            } => self.refined[producer],
            _ => false,
        };
        self.refined.push(refined);
    }

    fn compact(&mut self, _retained: &[Option<usize>]) {
        self.refined = Vec::new();
    }
}

#[derive(Default)]
pub(crate) struct ValueTable<P: Phase = Recipes> {
    pub(crate) values: Vec<Value>,
    pub(crate) bounds: Vec<BitBounds>,
    assumptions: Vec<Bits>,
    interned: HashTable<usize>,
    hash_builder: FxBuildHasher,
    phase: P,
}

/// One component of a pure expression, whether stored inline or as a projection.
#[derive(Clone, Copy)]
pub(crate) struct ExpressionResult {
    pub(crate) producer: usize,
    pub(crate) expression: Expression<usize>,
    pub(crate) component: usize,
}

impl<P: Phase> ValueTable<P> {
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
                ValueDefinition::RefinedAlias {
                    source: input,
                    block,
                    assumption,
                } => ValueDefinition::RefinedAlias {
                    source: remapping.value(input),
                    block,
                    assumption,
                },
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
        self.phase.compact(&remapping.values);
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

    /// Store proof bits separately so ordinary values keep their compact layout.
    pub(crate) fn refined_alias(&mut self, source: usize, block: BlockId, known: Bits) -> usize {
        let assumption = if known == Bits::default() {
            None
        } else {
            self.assumptions.push(known);
            std::num::NonZeroUsize::new(self.assumptions.len())
        };
        self.push(Value {
            ty: self.values[source].ty,
            definition: ValueDefinition::RefinedAlias {
                source,
                block,
                assumption,
            },
        })
    }

    pub(crate) fn refinement_bits(&self, value: usize) -> Bits {
        match self.values[value].definition {
            ValueDefinition::RefinedAlias {
                assumption: Some(index),
                ..
            } => self.assumptions[index.get() - 1],
            _ => Bits::default(),
        }
    }

    /// A calculated value or proof alias is placed where its consumers need it.
    pub(crate) fn recipe(&self, id: usize) -> Option<usize> {
        if matches!(
            self.values[id].definition,
            ValueDefinition::RefinedAlias { .. }
        ) {
            Some(id)
        } else {
            self.expression(id).map(|result| result.producer)
        }
    }

    pub(crate) fn recipe_inputs(&self, id: usize) -> impl DoubleEndedIterator<Item = usize> {
        let mut inputs = [None; 3];
        if let ValueDefinition::RefinedAlias { source, .. } = self.values[id].definition {
            inputs[0] = Some(source);
        } else if let Some(result) = self.expression(id) {
            for (slot, &input) in inputs.iter_mut().zip(result.expression.inputs()) {
                *slot = Some(input);
            }
        }
        inputs.into_iter().flatten()
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

    /// Transfer an unconditional bound across a proven full-carrier equivalence.
    /// Definitions stay fixed; learning tighter bounds cannot invalidate a proof
    /// already made with the more conservative bounds.
    pub(crate) fn retain_bounds(&mut self, result: usize, original: usize) {
        debug_assert_eq!(
            self.values[result].ty.carrier(),
            self.values[original].ty.carrier()
        );
        let known = self.bounds[original];
        self.bounds[result].unsigned = self.bounds[result].unsigned.min(known.unsigned);
        self.bounds[result].signed = self.bounds[result].signed.min(known.signed);
    }

    /// Conversions within a Wasm carrier preserve all physical bits.
    pub(crate) fn representation(&self, mut id: usize) -> usize {
        loop {
            match self.values[id].definition {
                ValueDefinition::RefinedAlias { source: input, .. } => id = input,
                ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::Convert,
                    input,
                }) if self.values[id].ty.carrier() == self.values[input].ty.carrier() => id = input,
                _ => break,
            }
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
        self.phase.record(index, value);
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
        if !P::INTERN_EXPRESSIONS && !matches!(value.definition, ValueDefinition::Literal(_)) {
            return self.push(value);
        }
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

impl ValueTable<Recipes> {
    pub(crate) fn depends_on_assumption(&self, value: usize) -> bool {
        self.phase.refined.get(value).copied().unwrap_or(false)
    }
}

impl<P: Phase> std::ops::Deref for ValueTable<P> {
    type Target = [Value];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
