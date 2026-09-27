//! Comparison folding and logical-width normalization.

use super::ValueArena;
use crate::{
    integer::{self, BinaryOp, CompareOp},
    Expression, Type, Value, ValueDefinition,
};

impl ValueArena {
    pub(super) fn compare(&mut self, operator: CompareOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let (ValueDefinition::Constant(a), ValueDefinition::Constant(b)) =
            (a.definition, b.definition)
        {
            let result = integer::compare(self.values[left].ty, operator, a, b);
            return self.constant(Type::I1, u64::from(result));
        }
        if left == right {
            return self.constant(
                Type::I1,
                u64::from(matches!(
                    operator,
                    CompareOp::Eq | CompareOp::GeUnsigned | CompareOp::GeSigned
                )),
            );
        }
        let constant_comparison = match (a.definition, b.definition) {
            (_, ValueDefinition::Constant(constant)) => Some((left, constant, false)),
            (ValueDefinition::Constant(constant), _) => Some((right, constant, true)),
            _ => None,
        };
        if let Some((input, constant, constant_first)) = constant_comparison {
            // Bound work even when selections share subtrees. Unknown or mixed
            // alternatives keep the original comparison instead of expanding it.
            let mut remaining = 32;
            if let Some(result) = self.compare_constant_choices(
                input,
                constant,
                operator,
                constant_first,
                &mut remaining,
            ) {
                return self.constant(Type::I1, u64::from(result));
            }
        }
        // Equality can use the signed carriers when both already repeat their
        // logical sign. Mixed signed/unsigned representations still need masks.
        let signed_operands = matches!(operator, CompareOp::LtSigned | CompareOp::GeSigned)
            || (matches!(operator, CompareOp::Eq | CompareOp::Ne)
                && self.bounds[left].signed <= a.ty.bits()
                && self.bounds[right].signed <= b.ty.bits());
        if matches!(operator, CompareOp::Eq | CompareOp::Ne) {
            let input = match (a.definition, b.definition) {
                (_, ValueDefinition::Constant(0)) => Some(left),
                (ValueDefinition::Constant(0), _) => Some(right),
                _ => None,
            };
            if let Some(input) = input {
                if operator == CompareOp::Ne && a.ty == Type::I1 {
                    return input;
                }
                return self.zero_test(input, operator == CompareOp::Ne);
            }
            let masked = match (a.definition, b.definition) {
                (_, ValueDefinition::Constant(mask)) => Some((left, mask)),
                (ValueDefinition::Constant(mask), _) => Some((right, mask)),
                _ => None,
            };
            if let Some((input, mask)) = masked {
                if let ValueDefinition::Expression(Expression::Binary {
                    operator: BinaryOp::And,
                    left: x,
                    right: y,
                }) = self.values[input].definition
                {
                    // A one-bit mask yields either zero or that mask.
                    if mask.is_power_of_two()
                        && (self.values[x].definition == ValueDefinition::Constant(mask)
                            || self.values[y].definition == ValueDefinition::Constant(mask))
                    {
                        return self.zero_test(input, operator == CompareOp::Eq);
                    }
                }
            }
            if !signed_operands
                && self.bounds[left].unsigned > a.ty.bits()
                && self.bounds[right].unsigned > a.ty.bits()
            {
                // Compare the low-bit difference once instead of masking both operands.
                let difference = self.binary(BinaryOp::Xor, left, right);
                return self.zero_test(difference, operator == CompareOp::Ne);
            }
        }
        let (left, right) = if signed_operands {
            (
                self.sign_extend_carrier(left),
                self.sign_extend_carrier(right),
            )
        } else {
            (self.normalize(left), self.normalize(right))
        };
        self.intern(Value {
            ty: Type::I1,
            definition: ValueDefinition::Expression(Expression::Compare {
                operator,
                left,
                right,
            }),
        })
    }

    fn compare_constant_choices(
        &self,
        input: usize,
        constant: u64,
        operator: CompareOp,
        constant_first: bool,
        remaining: &mut usize,
    ) -> Option<bool> {
        *remaining = remaining.checked_sub(1)?;
        let value = self.values[input];
        match value.definition {
            ValueDefinition::Constant(bits) => {
                let (left, right) = if constant_first {
                    (constant, bits)
                } else {
                    (bits, constant)
                };
                Some(integer::compare(value.ty, operator, left, right))
            }
            ValueDefinition::Expression(Expression::Select {
                when_true,
                when_false,
                ..
            }) => {
                let when_true = self.compare_constant_choices(
                    when_true,
                    constant,
                    operator,
                    constant_first,
                    remaining,
                )?;
                let when_false = self.compare_constant_choices(
                    when_false,
                    constant,
                    operator,
                    constant_first,
                    remaining,
                )?;
                (when_true == when_false).then_some(when_true)
            }
            _ => None,
        }
    }

    pub(super) fn zero_test(&mut self, input: usize, nonzero: bool) -> usize {
        let input = if self.bounds[input].signed <= self.values[input].ty.bits() {
            input
        } else {
            self.normalize(input)
        };
        // A value already restricted to zero or one is its own nonzero test.
        if nonzero && self.bounds[input].unsigned <= 1 {
            return self.convert(input, Type::I1);
        }
        self.intern(Value {
            ty: Type::I1,
            definition: ValueDefinition::Expression(Expression::ZeroTest { input, nonzero }),
        })
    }
}
