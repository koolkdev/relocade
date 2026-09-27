//! Value storage, interning and shared expression canonicalization.
//!
//! Construction and simplification use the same table. Handle lifetimes and
//! lexical visibility belong to the builder, not to these numerical records.
use std::collections::HashMap;

use super::{Site, Value, ValueDefinition};
use crate::{
    integer::{self, BitBounds, BitCountOp},
    Expression, Type,
};

mod arithmetic;
mod comparisons;
mod shifts;

#[derive(Default)]
pub(crate) struct ValueTable {
    pub(crate) values: Vec<Value>,
    pub(crate) bounds: Vec<BitBounds>,
    interned: HashMap<Value, usize>,
}

impl ValueTable {
    pub(crate) fn join_result(
        &mut self,
        ty: Type,
        site: Site,
        component: usize,
        inputs: &[usize],
    ) -> usize {
        // Joining does not clear upper bits. Later observers use the largest
        // bound from the arms that actually yield a value.
        let bounds = inputs
            .iter()
            .map(|&id| self.bounds[id])
            .reduce(BitBounds::union)
            .unwrap();
        self.push_with_bounds(
            Value {
                ty,
                definition: ValueDefinition::JoinResult { site, component },
            },
            bounds,
        )
    }

    // These inputs have logical types. Existing canonical body nodes may carry
    // wider operands and must retain their own rebuilding rules.
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
            Expression::Normalize { input } => self.normalize(input),
        }
    }

    /// Rebuild a stored expression after replacing its operands. Its existing
    /// carrier conversions are already present and must not be inserted again.
    pub(crate) fn rebuild(&mut self, ty: Type, expression: Expression<usize>) -> usize {
        let preserve_carrier = match expression {
            Expression::Binary { left, right, .. } => {
                self.values[left].ty != ty || self.values[right].ty != ty
            }
            Expression::Shift { .. } | Expression::Convert { .. } => true,
            _ => false,
        };
        if preserve_carrier {
            self.intern(Value {
                ty,
                definition: ValueDefinition::Expression(expression),
            })
        } else {
            self.expression(ty, expression)
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

    fn convert(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        if let ValueDefinition::Constant(bits) = source.definition {
            return self.constant(target, bits);
        }
        let input = if source.ty.bits() < target.bits() {
            self.normalize(input)
        } else {
            input
        };
        self.intern(Value {
            ty: target,
            definition: ValueDefinition::Expression(Expression::Convert { input }),
        })
    }

    fn sign_extend(&mut self, input: usize, target: Type) -> usize {
        let source = self.values[input];
        if source.ty == target {
            return input;
        }
        if let ValueDefinition::Constant(bits) = source.definition {
            return self.constant(target, integer::signed_value(source.ty, bits) as u64);
        }
        let canonical = self.bounds[input].signed <= source.ty.bits();
        if canonical && target != Type::I64 {
            // Preserve the existing signed representation and its sharing when
            // only the logical type widens; unsigned convert() would mask it.
            return self.intern(Value {
                ty: target,
                definition: ValueDefinition::Expression(Expression::Convert { input }),
            });
        }
        if canonical {
            let alias = if source.ty == Type::I32 {
                input
            } else {
                self.intern(Value {
                    ty: Type::I32,
                    definition: ValueDefinition::Expression(Expression::Convert { input }),
                })
            };
            // Crossing into i64 still needs the signed carrier extension.
            return self.intern(Value {
                ty: target,
                definition: ValueDefinition::Expression(Expression::SignExtend { input: alias }),
            });
        }
        self.intern(Value {
            ty: target,
            definition: ValueDefinition::Expression(Expression::SignExtend { input }),
        })
    }

    fn sign_extend_carrier(&mut self, input: usize) -> usize {
        // Interpret the logical sign before a signed carrier operation; narrow
        // arithmetic can leave upper bits that do not belong to the value.
        let carrier = if self.values[input].ty == Type::I64 {
            Type::I64
        } else {
            Type::I32
        };
        self.sign_extend(input, carrier)
    }

    pub(crate) fn normalize(&mut self, input: usize) -> usize {
        let value = self.values[input];
        if self.bounds[input].unsigned <= value.ty.bits() {
            return input;
        }
        // Calls, returns and unsigned observations share the masked result.
        // Arithmetic and stores keep the original value.
        self.intern(Value {
            ty: value.ty,
            definition: ValueDefinition::Expression(Expression::Normalize { input }),
        })
    }

    pub(crate) fn push(&mut self, value: Value) -> usize {
        let bounds = BitBounds::for_value(value, &self.values, &self.bounds);
        self.push_with_bounds(value, bounds)
    }

    fn push_with_bounds(&mut self, value: Value, bounds: BitBounds) -> usize {
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
