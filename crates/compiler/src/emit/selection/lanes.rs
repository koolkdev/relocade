//! Discard replacements that cannot affect a sole scalar observer.
//!
//! These aliases describe the final consumers, not value equivalence. Masks use
//! physical byte positions, including through logical V64 views of a carrier.

use super::Selection;
use crate::{
    body::{FunctionGraph, ValueDefinition},
    expression::{BinaryOp as Binary, LaneBinaryOp, ShiftOp, UnaryOp as Unary},
    Expression,
};

fn lane_bytes(width: u8, lane: u8) -> u16 {
    ((1_u16 << width) - 1) << (lane * width)
}

fn input_bytes(bytes: u16, width: u8, prefix: bool) -> u16 {
    let mut needed = 0;
    for lane in 0..16 / width {
        let mask = lane_bytes(width, lane);
        let observed = bytes & mask;
        if observed != 0 {
            // Modular arithmetic needs the lower bytes for carries/borrows.
            // Comparisons and saturation depend on every byte of their lane.
            needed |= if prefix {
                (u16::MAX >> observed.leading_zeros()) & mask
            } else {
                mask
            };
        }
    }
    needed
}

impl Selection {
    pub(super) fn discard_unobserved_lanes(
        &mut self,
        graph: &FunctionGraph,
        order: &[usize],
    ) -> bool {
        let mut changed = false;
        for &value in order {
            let ValueDefinition::Expression(Expression::Unary {
                operator: Unary::ExtractLane { lane },
                input,
            }) = graph.values[value].definition
            else {
                continue;
            };
            let bits = graph.values[value].ty.bits();
            // Whole V64 observations are the ordinary packed-chain boundary.
            if bits < graph.values[input].ty.bits() {
                let mut budget = 32;
                changed |= self.discard_replacements(
                    graph,
                    input,
                    lane_bytes(bits / 8, lane),
                    &mut budget,
                );
            }
        }
        changed
    }

    fn discard_replacements(
        &mut self,
        graph: &FunctionGraph,
        value: usize,
        bytes: u16,
        budget: &mut u8,
    ) -> bool {
        let value = self.resolve(value);
        if bytes == 0 || *budget == 0 || self.uses[value] != 1 {
            return false;
        }
        *budget -= 1;
        let ValueDefinition::Expression(expression) = graph.values[value].definition else {
            return false;
        };
        match expression {
            Expression::Binary {
                operator: Binary::ReplaceLane { lane_type, lane },
                left,
                ..
            } => {
                let replaced = lane_bytes(lane_type.bits() / 8, lane);
                let removed = bytes & replaced == 0;
                if removed {
                    self.bypass_expression(graph, value, left);
                }
                self.discard_replacements(graph, left, bytes & !replaced, budget) | removed
            }
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                let (a, b) = match operator {
                    Binary::Bitwise(_) => (bytes, bytes),
                    Binary::Vector {
                        operator,
                        lane_type,
                    } => {
                        let prefix = match operator {
                            LaneBinaryOp::Add | LaneBinaryOp::Sub | LaneBinaryOp::Mul => true,
                            LaneBinaryOp::Eq
                            | LaneBinaryOp::GtSigned
                            | LaneBinaryOp::AddSatSigned
                            | LaneBinaryOp::AddSatUnsigned
                            | LaneBinaryOp::SubSatSigned
                            | LaneBinaryOp::SubSatUnsigned => false,
                            // Width-changing lane dependencies remain opaque.
                            _ => return false,
                        };
                        let needed = input_bytes(bytes, lane_type.bits() / 8, prefix);
                        (needed, needed)
                    }
                    Binary::ShuffleImmediate(mask) => {
                        let mut inputs = [0_u16; 2];
                        for (byte, source) in mask.indices().into_iter().enumerate() {
                            if bytes & (1 << byte) != 0 {
                                inputs[usize::from(source / 16)] |= 1 << (source % 16);
                            }
                        }
                        (inputs[0], inputs[1])
                    }
                    _ => return false,
                };
                // One budget covers both operands; a branching DAG cannot
                // multiply the amount of selection work at each recursive call.
                let a = self.discard_replacements(graph, left, a, budget);
                self.discard_replacements(graph, right, b, budget) | a
            }
            Expression::Shift {
                operator: ShiftOp::Lanes { lane_type, .. },
                value,
                ..
            } => self.discard_replacements(
                graph,
                value,
                input_bytes(bytes, lane_type.bits() / 8, false),
                budget,
            ),
            _ => false,
        }
    }
}
