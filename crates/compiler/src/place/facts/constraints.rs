//! Revisit a bounded set of Boolean constraints after a batch of changed facts.

use super::{Bits, ScalarFacts};
use crate::{bitwise::BitwiseOp, body::ValueTable, expression::BinaryOp, Expression, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;

#[cfg(test)]
mod tests;

const MAX_CONSTRAINTS: usize = 16;
const MAX_CONSTRAINT_CHECKS: usize = 256;

#[derive(Clone)]
pub(super) struct Constraints {
    // Forks share the roots, but keep their own settled state. Empty contexts
    // allocate nothing; adding a root copies only this short list.
    roots: Option<Rc<Vec<usize>>>,
    active: u64,
    // The earliest root that may need another visit; usize::MAX means none.
    // Inputs precede their results, so later facts cannot affect earlier roots.
    revisit_from: usize,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            roots: None,
            active: 0,
            revisit_from: usize::MAX,
        }
    }
}

impl Constraints {
    pub(super) fn track(&mut self, table: &ValueTable, id: usize, bits: Bits) {
        if table[id].ty != Type::I1 || bits.mask & 1 == 0 {
            return;
        }
        let Some(result) = table.expression(id) else {
            return;
        };
        let Expression::Binary {
            operator: BinaryOp::Bitwise(operator),
            ..
        } = result.expression
        else {
            return;
        };
        // True conjunctions and false disjunctions already constrain both inputs.
        if matches!(operator, BitwiseOp::And) && bits.value & 1 != 0
            || matches!(operator, BitwiseOp::Or) && bits.value & 1 == 0
        {
            return;
        }
        let roots = self.roots.as_deref();
        let index = if let Some(index) = roots.and_then(|r| r.iter().position(|&root| root == id)) {
            index
        } else {
            if roots.is_some_and(|r| r.len() == MAX_CONSTRAINTS) {
                return;
            }
            let roots = Rc::make_mut(self.roots.get_or_insert_default());
            let index = roots.len();
            roots.push(id);
            index
        };
        self.active |= 1 << index;
        self.revisit_from = self.revisit_from.min(id);
    }

    pub(super) fn changed(&mut self, id: usize) {
        if self.active != 0 {
            self.revisit_from = self.revisit_from.min(id);
        }
    }

    pub(super) fn retain_known(&mut self, known: &FxHashMap<usize, Bits>) {
        let Some(roots) = &self.roots else { return };
        // A join can forget the operand that settled a constraint on either path.
        self.active = roots
            .iter()
            .enumerate()
            .filter(|(_, id)| known.contains_key(id))
            .fold(0, |mask, (index, _)| mask | (1 << index));
        self.revisit_from = if self.active != 0 { 0 } else { usize::MAX };
    }
}

impl ScalarFacts {
    pub(super) fn propagate_constraints(&mut self, table: &ValueTable) {
        let mut remaining = MAX_CONSTRAINT_CHECKS;
        while self.constraints.revisit_from != usize::MAX && remaining != 0 {
            let revisit_from = std::mem::replace(&mut self.constraints.revisit_from, usize::MAX);
            let mut pending = self.constraints.active;
            while pending != 0 && remaining != 0 {
                let index = pending.trailing_zeros() as usize;
                pending &= pending - 1;
                remaining -= 1;
                let id = self.constraints.roots.as_ref().unwrap()[index];
                if id < revisit_from {
                    continue;
                }
                let Expression::Binary {
                    operator: BinaryOp::Bitwise(operator),
                    left,
                    right,
                } = table.expression(id).unwrap().expression
                else {
                    unreachable!()
                };
                let truth = self.known[&id].value & 1 != 0;
                let a = self.constant(table, left).map(|v| v & 1 != 0);
                let b = self.constant(table, right).map(|v| v & 1 != 0);
                if a.is_none() && b.is_none() {
                    continue;
                }
                // Facts only strengthen before a join. A settled constraint
                // cannot contribute another deduction in this context.
                self.constraints.active &= !(1 << index);
                for (input, own, other) in [(left, a, b), (right, b, a)] {
                    if own.is_some() {
                        continue;
                    }
                    let Some(other) = other else { continue };
                    let inferred = match operator {
                        BitwiseOp::And if other => Some(truth),
                        BitwiseOp::Or if !other => Some(truth),
                        BitwiseOp::Xor => Some(truth ^ other),
                        _ => None,
                    };
                    if let Some(truth) = inferred {
                        self.assume(table, input, truth);
                    }
                }
            }
            // Exhaustion postpones any remaining deductions until a later update.
            if pending != 0 {
                self.constraints.revisit_from = 0;
            }
        }
    }
}
