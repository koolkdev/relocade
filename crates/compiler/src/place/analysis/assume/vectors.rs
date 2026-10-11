//! Propagate observed lanes through the shared assumption batch.
use super::*;
use crate::expression::BinaryOp as Binary;

impl ValueAnalysis {
    pub(super) fn assume_vector(
        &mut self,
        table: &ValueTable,
        id: usize,
        bits: Bits<u128>,
        pending: &mut Pending,
    ) {
        if bits.mask == 0
            || self
                .context
                .vector_known
                .as_ref()
                .and_then(|known| known.get(&id))
                .is_some_and(|known| known.conflicts(bits) || bits.mask & !known.mask == 0)
        {
            return;
        }
        let previous = self.vector_bits(table, id);
        if previous.conflicts(bits) || bits.mask & !previous.mask == 0 {
            return;
        }
        let bits = previous.union(bits);
        self.record_vector(id, bits);
        let Some(result) = table.expression(id) else {
            return;
        };
        match result.expression {
            Expression::Binary {
                operator: Binary::ReplaceLane { lane_type, lane },
                left: vector,
                right: value,
            } => {
                let shift = lane * lane_type.bits();
                let mask = u128::from(lane_type.mask()) << shift;
                pending.vector.push((vector, bits.restrict(!mask)));
                pending.scalar.push((
                    value,
                    Bits {
                        mask: ((bits.mask & mask) >> shift) as u64,
                        value: ((bits.value & mask) >> shift) as u64,
                    },
                ));
            }
            Expression::Binary {
                operator: Binary::Bitwise(operator @ (BitwiseOp::And | BitwiseOp::Or)),
                left,
                right,
            } => {
                let (a, b) = if operator == BitwiseOp::And {
                    (
                        self.vector_bits(table, left),
                        self.vector_bits(table, right),
                    )
                } else {
                    (Bits::default(), Bits::default())
                };
                let [a, b] = bits.input_constraints(operator, a, b);
                pending.vector.push((left, a));
                pending.vector.push((right, b));
            }
            Expression::Select {
                condition,
                when_true,
                when_false,
            } => {
                let truth = self
                    .constant(table, condition)
                    .map(|value| value != 0)
                    .or_else(|| {
                        self.vector_bits(table, when_true)
                            .conflicts(bits)
                            .then_some(false)
                    })
                    .or_else(|| {
                        self.vector_bits(table, when_false)
                            .conflicts(bits)
                            .then_some(true)
                    });
                if let Some(truth) = truth {
                    pending.scalar.push((
                        condition,
                        Bits {
                            mask: 1,
                            value: u64::from(truth),
                        },
                    ));
                    pending
                        .vector
                        .push((if truth { when_true } else { when_false }, bits));
                }
            }
            _ => {}
        }
    }
}
