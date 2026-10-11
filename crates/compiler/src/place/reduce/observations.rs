//! Memoize reductions by the carrier positions their consumers observe.
//!
//! Complete observations use the ordinary value key. Partial masks and their
//! memo entries are allocated only when an operation narrows an observation.

use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};
use crate::{
    body::ValueTable,
    expression::LaneBinaryOp,
    integer::{low_mask, BinaryOp},
    Expression,
};
use rustc_hash::FxHashMap;

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Observation {
    pub(super) value: usize,
    // Zero denotes the complete carrier; other IDs index Partial::masks.
    mask: usize,
}

impl Observation {
    pub(super) fn full(value: usize) -> Self {
        Self { value, mask: 0 }
    }

    pub(super) fn is_full(self) -> bool {
        self.mask == 0
    }

    pub(super) fn with_value(self, value: usize) -> Self {
        Self { value, ..self }
    }
}

#[derive(Default)]
pub(super) struct Residuals {
    full: FxHashMap<usize, usize>,
    partial: Option<Box<Partial>>,
}

#[derive(Default)]
struct Partial {
    masks: Vec<u128>,
    values: FxHashMap<Observation, usize>,
}

impl Residuals {
    pub(super) fn get(&self, observed: Observation) -> Option<usize> {
        if observed.is_full() {
            self.full.get(&observed.value).copied()
        } else {
            self.partial.as_ref()?.values.get(&observed).copied()
        }
    }

    pub(super) fn record(&mut self, observed: Observation, residual: usize) {
        if observed.is_full() {
            self.full.insert(observed.value, residual);
            self.full.entry(residual).or_insert(residual);
        } else {
            self.partial
                .as_mut()
                .unwrap()
                .values
                .insert(observed, residual);
        }
    }

    pub(super) fn mask(&self, observed: Observation) -> u128 {
        if observed.is_full() {
            u128::MAX
        } else {
            self.partial.as_ref().unwrap().masks[observed.mask - 1]
        }
    }

    fn observe(&mut self, values: &ValueTable, value: usize, bits: u128) -> Observation {
        let carrier = if values[value].ty.is_scalar() {
            u128::from(values[value].ty.carrier().mask())
        } else {
            u128::MAX
        };
        let bits = bits & carrier;
        if bits == carrier {
            return Observation::full(value);
        }
        let partial = self.partial.get_or_insert_with(Box::default);
        // A context usually needs just a few widths or lane masks. Intern these
        // small immutable keys without a second hash table on the query path.
        let index = partial
            .masks
            .iter()
            .position(|&mask| mask == bits)
            .unwrap_or_else(|| {
                partial.masks.push(bits);
                partial.masks.len() - 1
            });
        Observation {
            value,
            mask: index + 1,
        }
    }

    /// Input observations preserve the requested output, including physical bits
    /// above a logical type. Unknown dependency rules keep the complete input.
    pub(super) fn inputs(
        &mut self,
        values: &ValueTable,
        observed: Observation,
    ) -> Expression<Observation> {
        let expression = values.expression(observed.value).unwrap().expression;
        // Most scalar recipes preserve complete carriers. They never construct
        // masks or touch partial memo storage.
        let narrows = match expression {
            Expression::Unary {
                operator: Unary::LowBits { .. },
                ..
            }
            | Expression::Unary {
                operator: Unary::ExtractLane { .. },
                ..
            }
            | Expression::Binary {
                operator: Binary::ReplaceLane { .. },
                ..
            } => true,
            Expression::Unary {
                operator: Unary::Convert,
                input,
            } => values[input].ty.carrier().bits() > values[observed.value].ty.carrier().bits(),
            _ => false,
        };
        if observed.is_full() && !narrows {
            return expression.map(|&input| Observation::full(input));
        }
        let bits = if observed.is_full() && values[observed.value].ty.is_scalar() {
            u128::from(values[observed.value].ty.carrier().mask())
        } else {
            self.mask(observed)
        };
        if let Expression::Binary {
            operator:
                Binary::Vector {
                    operator,
                    lane_type,
                },
            ..
        } = expression
        {
            let lane_mask = u128::from(lane_type.mask());
            let mut needed = 0;
            for shift in (0..128).step_by(lane_type.bits().into()) {
                let lane = (bits >> shift) & lane_mask;
                let input = match operator {
                    // Carries and borrows stop at the lane boundary.
                    LaneBinaryOp::Add | LaneBinaryOp::Sub => prefix(lane),
                    // Any comparison result bit depends on the entire input lane.
                    LaneBinaryOp::Eq | LaneBinaryOp::GtSigned => {
                        if lane == 0 {
                            0
                        } else {
                            lane_mask
                        }
                    }
                    // Saturation and width-changing products can observe bits
                    // outside an output prefix. Keep their complete inputs.
                    _ => return expression.map(|&input| Observation::full(input)),
                };
                needed |= input << shift;
            }
            return expression.map(|&input| self.observe(values, input, needed));
        }
        expression.map(|&input| {
            let needed = match expression {
                Expression::Binary {
                    operator: Binary::Bitwise(_),
                    ..
                }
                | Expression::Unary {
                    operator: Unary::Reinterpret,
                    ..
                }
                | Expression::Unary {
                    operator: Unary::Convert,
                    ..
                } => bits,
                Expression::Unary {
                    operator: Unary::LowBits { bits: width },
                    ..
                } => bits & u128::from(low_mask(width)),
                Expression::Binary {
                    operator: Binary::Integer(BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul),
                    ..
                } => prefix(bits),
                Expression::Select { condition, .. } => {
                    if input == condition {
                        u128::MAX
                    } else {
                        bits
                    }
                }
                Expression::Unary {
                    operator: Unary::ExtractLane { lane },
                    ..
                } => {
                    let ty = values[observed.value].ty;
                    (bits & u128::from(ty.mask())) << (lane * ty.bits())
                }
                Expression::Binary {
                    operator: Binary::ReplaceLane { lane_type, lane },
                    right: value,
                    ..
                } => {
                    let shift = lane * lane_type.bits();
                    let lane_mask = u128::from(lane_type.mask());
                    if input == value {
                        (bits >> shift) & lane_mask
                    } else {
                        bits & !(lane_mask << shift)
                    }
                }
                _ => return Observation::full(input),
            };
            self.observe(values, input, needed)
        })
    }
}

fn prefix(mask: u128) -> u128 {
    u128::MAX.checked_shr(mask.leading_zeros()).unwrap_or(0)
}
