//! Revisit a bounded set of Boolean constraints after a batch of changed facts.

use super::storage::Storage;
use super::{Bits, ScalarFacts};
use crate::{bitwise::BitwiseOp, body::ValueTable, expression::BinaryOp, Expression, Type};
use std::{cell::Cell, rc::Rc};

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
    pending: Cell<u64>,
}

impl Default for Constraints {
    fn default() -> Self {
        Self {
            roots: None,
            active: 0,
            pending: Cell::new(0),
        }
    }
}

impl Constraints {
    pub(super) fn track(&mut self, table: &ValueTable, id: usize, bits: Bits) -> bool {
        if table[id].ty != Type::I1 || bits.mask & 1 == 0 {
            return false;
        }
        let Some(result) = table.expression(id) else {
            return false;
        };
        let Expression::Binary {
            operator: BinaryOp::Bitwise(operator),
            ..
        } = result.expression
        else {
            return false;
        };
        // True conjunctions and false disjunctions already constrain both inputs.
        if matches!(operator, BitwiseOp::And) && bits.value & 1 != 0
            || matches!(operator, BitwiseOp::Or) && bits.value & 1 == 0
        {
            return false;
        }
        let roots = self.roots.as_deref();
        let index = if let Some(index) = roots.and_then(|r| r.iter().position(|&root| root == id)) {
            index
        } else {
            if roots.is_some_and(|r| r.len() == MAX_CONSTRAINTS) {
                return false;
            }
            let roots = Rc::make_mut(self.roots.get_or_insert_default());
            let index = roots.len();
            roots.push(id);
            index
        };
        self.active |= 1 << index;
        self.pending.set(self.pending.get() | (1 << index));
        true
    }

    pub(super) fn changed(&self, id: usize) {
        if let Some(index) = self
            .roots
            .as_ref()
            .and_then(|roots| roots.iter().position(|&root| root == id))
        {
            self.pending
                .set(self.pending.get() | (self.active & (1 << index)));
        }
    }

    pub(super) fn retain_known(&mut self, known: &Storage<Bits>) {
        let Some(roots) = &self.roots else { return };
        // A join can forget the operand that settled a constraint on either path.
        self.active = roots
            .iter()
            .enumerate()
            .filter(|(_, id)| known.contains_key(id))
            .fold(0, |mask, (index, _)| mask | (1 << index));
        self.pending.set(self.active);
    }
}

impl ScalarFacts {
    pub(super) fn propagate_constraints(&mut self, table: &ValueTable) {
        let mut remaining = MAX_CONSTRAINT_CHECKS;
        while remaining != 0 {
            self.refresh_bits(table);
            let mut pending = self.constraints.pending.replace(0) & self.constraints.active;
            if pending == 0 {
                break;
            }
            while pending != 0 && remaining != 0 {
                let index = pending.trailing_zeros() as usize;
                pending &= pending - 1;
                remaining -= 1;
                let id = self.constraints.roots.as_ref().unwrap()[index];
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
                self.constraints
                    .pending
                    .set(self.constraints.pending.get() | pending);
            }
        }
    }
}
