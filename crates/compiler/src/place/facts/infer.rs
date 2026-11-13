//! Derive facts through expressions without changing the current path.

use super::{Bits, ScalarFacts};
use crate::expression::{BinaryOp as Binary, CompareOp as Compare, TernaryOp as Ternary};
use crate::{body::ValueTable, Expression};

mod expression;

#[derive(Clone, Copy)]
pub(in crate::place) enum Inference {
    Visit(usize),
    Finish(usize),
}

impl ScalarFacts {
    /// Known scalar bits; non-scalar values contribute no knowledge.
    pub(super) fn bits(&self, table: &ValueTable, root: usize) -> Bits {
        if let Some(bits) = self.cached_bits(root) {
            return bits;
        }
        let mut pending = self.pending.borrow_mut();
        pending.push(Inference::Visit(root));
        while let Some(step) = pending.pop() {
            self.infer_step(table, step, |step| pending.push(step));
        }
        self.cached_bits(root).unwrap()
    }

    fn cached_bits(&self, id: usize) -> Option<Bits> {
        self.answers
            .borrow()
            .get(&id)
            .and_then(|answer| answer.bits)
    }

    // Preserve the literal and assumption-free shortcuts before walking operands.
    pub(in crate::place) fn constant_inference(
        &self,
        table: &ValueTable,
        id: usize,
    ) -> Option<Inference> {
        (table[id].ty.is_scalar()
            && table[id].scalar_literal().is_none()
            && self.has_observations()
            && self.cached_bits(id).is_none())
        .then_some(Inference::Visit(id))
    }

    pub(in crate::place) fn identity_inference(
        &self,
        table: &ValueTable,
        id: usize,
    ) -> Option<Inference> {
        let (_, left, right) = Self::bitwise_inputs(table, id)?;
        [left, right]
            .into_iter()
            .find(|&input| self.cached_bits(input).is_none())
            .map(Inference::Visit)
    }

    /// Both fact-only queries and reduction use these dependency steps.
    pub(in crate::place) fn infer_step(
        &self,
        table: &ValueTable,
        step: Inference,
        mut schedule: impl FnMut(Inference),
    ) {
        let (id, ready) = match step {
            Inference::Visit(id) => (id, false),
            Inference::Finish(id) => (id, true),
        };
        let mut cache = self.answers.borrow_mut();
        if cache.get(&id).is_some_and(|answer| answer.bits.is_some()) {
            return;
        }
        let value = table[id];
        if !value.ty.is_scalar() {
            cache.entry(id).or_default().bits = Some(Bits::default());
            return;
        }
        let known = self.known_bits(table, id);
        if let Some(bits) = value.scalar_literal() {
            cache.entry(id).or_default().bits = Some(Bits {
                mask: value.ty.mask(),
                value: value.ty.normalize(bits),
            });
            return;
        }
        if known.mask == value.ty.mask() {
            cache.entry(id).or_default().bits = Some(known);
            return;
        }
        let Some(result) = table.expression(id) else {
            cache.entry(id).or_default().bits = Some(known);
            return;
        };
        let expression = result.expression;
        if let Expression::Binary {
            operator: Binary::Compare(Compare::Integer(operator)),
            left,
            right,
        } = expression
        {
            if let Some(truth) = self.comparisons.get(table, operator, left, right) {
                cache.entry(id).or_default().bits = Some(Bits {
                    mask: 1,
                    value: u64::from(truth),
                });
                return;
            }
        }
        // Resolve the selector first so known paths do not walk discarded
        // state histories merely to rediscover the chosen value.
        if let Expression::Ternary {
            operator: Ternary::Select,
            first: when_true,
            second: when_false,
            third: condition,
        } = expression
        {
            let Some(condition_bits) = cache.get(&condition).and_then(|answer| answer.bits) else {
                schedule(Inference::Visit(id));
                schedule(Inference::Visit(condition));
                return;
            };
            if condition_bits.mask & 1 != 0 {
                let input = if condition_bits.value & 1 != 0 {
                    when_true
                } else {
                    when_false
                };
                if let Some(bits) = cache.get(&input).and_then(|answer| answer.bits) {
                    cache.entry(id).or_default().bits =
                        Some(bits.union(known).restrict(value.ty.mask()));
                } else {
                    schedule(Inference::Visit(id));
                    schedule(Inference::Visit(input));
                }
                return;
            }
        }
        if !ready {
            schedule(Inference::Finish(id));
            for &input in expression.inputs() {
                schedule(Inference::Visit(input));
            }
            return;
        }
        let bits = self.expression_bits(table, id, |input| cache[&input].bits.unwrap());
        cache.entry(id).or_default().bits = Some(bits.union(known).restrict(value.ty.mask()));
    }
}
