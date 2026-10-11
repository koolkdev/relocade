//! Lane boundaries bridge compact scalar facts and lazily allocated vector facts.

use super::{Bits, Inference};
use crate::expression::BinaryOp as Binary;
use crate::{
    body::ValueDefinition,
    expression::{LaneBinaryOp, TypedLiteral},
    Expression, Type,
};

impl Inference<'_> {
    pub(super) fn vector(&mut self, id: usize, ready: bool) {
        let table = self.table;
        let derived = &mut self.derived;
        let pending = &mut self.pending;
        if derived
            .vectors
            .as_ref()
            .is_some_and(|bits| bits.contains_key(&id))
        {
            return;
        }
        let known = self
            .analysis
            .context
            .vector_known
            .as_ref()
            .and_then(|known| known.get(&id))
            .copied()
            .unwrap_or_default();
        let value = table[id];
        let bits = if let ValueDefinition::Literal(literal) = value.definition {
            Bits {
                mask: u128::MAX,
                value: u128::from(literal),
            }
        } else if known.mask == u128::MAX {
            known
        } else if let Some(result) = table.expression(id) {
            let expression = result.expression;
            if let Expression::Select {
                condition,
                when_true,
                when_false,
            } = expression
            {
                let Some(condition) = derived.bits.get(&condition) else {
                    pending.push((id, false));
                    pending.push((condition, false));
                    return;
                };
                if condition.mask & 1 != 0 {
                    let input = if condition.value & 1 != 0 {
                        when_true
                    } else {
                        when_false
                    };
                    if let Some(&bits) = derived.vectors.as_ref().and_then(|bits| bits.get(&input))
                    {
                        derived
                            .vectors
                            .get_or_insert_with(Box::default)
                            .insert(id, bits.union(known));
                    } else {
                        pending.push((id, false));
                        pending.push((input, false));
                    }
                    return;
                }
            }
            if !ready {
                pending.push((id, true));
                pending.extend(expression.inputs().map(|&input| (input, false)));
                return;
            }
            match expression {
                Expression::Binary {
                    operator:
                        Binary::Vector {
                            operator:
                                LaneBinaryOp::Add
                                | LaneBinaryOp::Sub
                                | LaneBinaryOp::Mul
                                | LaneBinaryOp::Eq
                                | LaneBinaryOp::GtSigned
                                | LaneBinaryOp::AddSatSigned
                                | LaneBinaryOp::AddSatUnsigned
                                | LaneBinaryOp::SubSatSigned
                                | LaneBinaryOp::SubSatUnsigned,
                            lane_type,
                        },
                    left,
                    right,
                } => {
                    let vectors = derived.vectors.as_ref().unwrap();
                    let known = vectors[&left].mask & vectors[&right].mask;
                    let lane_mask = u128::from(lane_type.mask());
                    let mut mask = 0;
                    for shift in (0..128).step_by(lane_type.bits().into()) {
                        let lane = lane_mask << shift;
                        if known & lane == lane {
                            mask |= lane;
                        }
                    }
                    if mask == 0 {
                        Bits::default()
                    } else {
                        // Reuse literal semantics only for complete known lanes.
                        // Arbitrary bits in every other lane are masked away.
                        let literals = expression.map(|&input| TypedLiteral {
                            ty: Type::V128,
                            value: vectors[&input].value.into(),
                        });
                        Bits {
                            mask,
                            value: u128::from(literals.constant_result(Type::V128, 0).unwrap())
                                & mask,
                        }
                    }
                }
                Expression::Binary {
                    operator: Binary::Bitwise(operator),
                    left,
                    right,
                } => {
                    let vectors = derived.vectors.as_ref().unwrap();
                    vectors[&left].bitwise(operator, vectors[&right])
                }
                Expression::Binary {
                    operator: Binary::ReplaceLane { lane_type, lane },
                    left: vector,
                    right: value,
                } => {
                    let shift = lane * lane_type.bits();
                    let mask = u128::from(lane_type.mask()) << shift;
                    let value = derived.bits[&value];
                    derived.vectors.as_ref().unwrap()[&vector]
                        .restrict(!mask)
                        .union(
                            Bits {
                                mask: u128::from(value.mask) << shift,
                                value: u128::from(value.value) << shift,
                            }
                            .restrict(mask),
                        )
                }
                Expression::Select {
                    when_true,
                    when_false,
                    ..
                } => {
                    let vectors = derived.vectors.as_ref().unwrap();
                    vectors[&when_true].common(vectors[&when_false])
                }
                _ => Bits::default(),
            }
            .union(known)
        } else {
            known
        };
        derived
            .vectors
            .get_or_insert_with(Box::default)
            .insert(id, bits);
    }
}
