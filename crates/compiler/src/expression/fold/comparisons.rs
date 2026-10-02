//! Comparison folding and logical-width normalization.

use super::Folder;
use crate::{
    body::ValueDefinition,
    integer::{self, BinaryOp, CompareOp},
    Expression, Type,
};

struct ConstantComparison {
    operator: CompareOp,
    ty: Type,
    constant: u64,
    constant_first: bool,
}

impl ConstantComparison {
    fn evaluate(&self, bits: u64) -> bool {
        let bits = self.ty.normalize(bits);
        let constant = self.ty.normalize(self.constant);
        let (left, right) = if self.constant_first {
            (constant, bits)
        } else {
            (bits, constant)
        };
        integer::compare(self.ty, self.operator, left, right)
    }
}

impl Folder<'_> {
    pub(super) fn complementary_predicates(&self, left: usize, right: usize) -> bool {
        let left = self.values.representation(left);
        let right = self.values.representation(right);
        for (predicate, other) in [(left, right), (right, left)] {
            let ValueDefinition::Expression(Expression::ZeroTest { input, nonzero }) =
                self.values[predicate].definition
            else {
                continue;
            };
            let input = self.values.representation(input);
            if let ValueDefinition::Expression(Expression::ZeroTest {
                input: other_input,
                nonzero: other_nonzero,
            }) = self.values[other].definition
            {
                if input == self.values.representation(other_input) && nonzero != other_nonzero {
                    return true;
                }
            }
            // Logical I1 alone is insufficient: a type view can carry upper bits.
            if !nonzero && input == other && self.values.bounds[other].unsigned <= 1 {
                return true;
            }
        }
        false
    }

    pub(super) fn compare(&mut self, operator: CompareOp, left: usize, right: usize) -> usize {
        let a = self.values[left];
        let b = self.values[right];
        debug_assert_eq!(a.ty, b.ty);
        if let Some(result) = self.compare_constant_choices(operator, left, right, a.ty) {
            return self.values.constant(Type::I1, u64::from(result));
        }
        // Equality can use the signed carriers when both already repeat their
        // logical sign. Mixed signed/unsigned representations still need masks.
        let signed_operands = matches!(operator, CompareOp::LtSigned | CompareOp::GeSigned)
            || (matches!(operator, CompareOp::Eq | CompareOp::Ne)
                && self.values.bounds[left].signed <= a.ty.bits()
                && self.values.bounds[right].signed <= b.ty.bits());
        if matches!(operator, CompareOp::Eq | CompareOp::Ne)
            && !signed_operands
            && self.values.bounds[left].unsigned > a.ty.bits()
            && self.values.bounds[right].unsigned > a.ty.bits()
        {
            // Compare the low-bit difference once instead of masking both operands.
            let difference = self.binary(BinaryOp::Xor, left, right);
            return self.zero_test(difference, operator == CompareOp::Ne);
        }
        let (left, right) = if signed_operands {
            (
                self.sign_extend_carrier(left),
                self.sign_extend_carrier(right),
            )
        } else {
            (self.normalize(left), self.normalize(right))
        };
        self.fold(
            Type::I1,
            Expression::Compare {
                operator,
                left,
                right,
            },
        )
    }

    pub(super) fn fold_compare(
        &mut self,
        operator: CompareOp,
        left: usize,
        right: usize,
    ) -> Option<usize> {
        let a = self.values.representation(left);
        let b = self.values.representation(right);
        if let Some(result) =
            self.compare_constant_choices(operator, a, b, self.values[a].ty.carrier())
        {
            return Some(self.values.constant(Type::I1, u64::from(result)));
        }
        if a == b {
            return Some(self.values.constant(
                Type::I1,
                u64::from(matches!(
                    operator,
                    CompareOp::Eq | CompareOp::GeUnsigned | CompareOp::GeSigned
                )),
            ));
        }
        if matches!(operator, CompareOp::LtUnsigned | CompareOp::GeUnsigned) {
            if self.values[b].definition == ValueDefinition::Constant(0) {
                return Some(
                    self.values
                        .constant(Type::I1, u64::from(operator == CompareOp::GeUnsigned)),
                );
            }
            if self.values[a].definition == ValueDefinition::Constant(0) {
                return Some(self.fold(
                    Type::I1,
                    Expression::ZeroTest {
                        input: right,
                        nonzero: operator == CompareOp::LtUnsigned,
                    },
                ));
            }
        }
        if !matches!(operator, CompareOp::Eq | CompareOp::Ne) {
            return None;
        }
        let (input, mask) = match (self.values[a].definition, self.values[b].definition) {
            (_, ValueDefinition::Constant(mask)) => (left, mask),
            (ValueDefinition::Constant(mask), _) => (right, mask),
            _ => return None,
        };
        let nonzero = if mask == 0 {
            operator == CompareOp::Ne
        } else {
            let one_bit = mask == 1 && self.values.bounds[input].unsigned <= 1;
            let masked_bit = match self.values[self.values.representation(input)].definition {
                ValueDefinition::Expression(Expression::Binary {
                    operator: BinaryOp::And,
                    left,
                    right,
                }) => {
                    mask.is_power_of_two()
                        && (self.values[left].definition == ValueDefinition::Constant(mask)
                            || self.values[right].definition == ValueDefinition::Constant(mask))
                }
                _ => false,
            };
            if !one_bit && !masked_bit {
                return None;
            }
            operator == CompareOp::Eq
        };
        Some(self.fold(Type::I1, Expression::ZeroTest { input, nonzero }))
    }

    fn compare_constant_choices(
        &self,
        operator: CompareOp,
        left: usize,
        right: usize,
        ty: Type,
    ) -> Option<bool> {
        let (input, constant, constant_first) =
            match (self.values[left].definition, self.values[right].definition) {
                (_, ValueDefinition::Constant(constant)) => (left, constant, false),
                (ValueDefinition::Constant(constant), _) => (right, constant, true),
                _ => return None,
            };
        let comparison = ConstantComparison {
            operator,
            ty,
            constant,
            constant_first,
        };
        // Bound work even when selections share subtrees. Unknown or mixed
        // alternatives keep the original comparison instead of expanding it.
        self.compare_choices(input, &comparison, &mut 32)
    }

    fn compare_choices(
        &self,
        input: usize,
        comparison: &ConstantComparison,
        remaining: &mut usize,
    ) -> Option<bool> {
        *remaining = remaining.checked_sub(1)?;
        let value = self.values[input];
        match value.definition {
            ValueDefinition::Constant(bits) => Some(comparison.evaluate(bits)),
            ValueDefinition::Expression(Expression::Select {
                when_true,
                when_false,
                ..
            }) => {
                let when_true = self.compare_choices(when_true, comparison, remaining)?;
                let when_false = self.compare_choices(when_false, comparison, remaining)?;
                (when_true == when_false).then_some(when_true)
            }
            _ if self.values.bounds[input].unsigned <= 1 => {
                // Canonical predicates retain the same choices after a 0/1
                // selection has folded away, including through type views.
                let zero = comparison.evaluate(0);
                (zero == comparison.evaluate(1)).then_some(zero)
            }
            _ => None,
        }
    }

    pub(super) fn zero_test(&mut self, input: usize, nonzero: bool) -> usize {
        let input = if self.values.bounds[input].signed <= self.values[input].ty.bits() {
            input
        } else {
            self.normalize(input)
        };
        self.fold(Type::I1, Expression::ZeroTest { input, nonzero })
    }

    pub(super) fn fold_zero_test(&mut self, input: usize, nonzero: bool) -> Option<usize> {
        if nonzero && self.values.bounds[input].unsigned <= 1 {
            return Some(input);
        }
        if let ValueDefinition::Expression(Expression::ZeroTest {
            input: source,
            nonzero: inner_nonzero,
        }) = self.values[input].definition
        {
            return Some(self.fold(
                Type::I1,
                Expression::ZeroTest {
                    input: source,
                    nonzero: if nonzero {
                        inner_nonzero
                    } else {
                        !inner_nonzero
                    },
                },
            ));
        }
        if let ValueDefinition::Expression(Expression::Convert { input: source }) =
            self.values[input].definition
        {
            // A carrier conversion that discards no set bits preserves zero.
            // Logical narrowing still keeps its explicit normalization mask.
            if self.values.bounds[source].unsigned <= self.values[input].ty.carrier().bits() {
                return Some(self.fold(
                    Type::I1,
                    Expression::ZeroTest {
                        input: source,
                        nonzero,
                    },
                ));
            }
        }
        None
    }
}
