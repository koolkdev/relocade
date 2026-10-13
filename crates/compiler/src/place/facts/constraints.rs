//! Propagate retained Boolean constraints when facts about their inputs change.

use super::{Bits, ScalarFacts};
use crate::{bitwise::BitwiseOp, body::ValueTable, expression::BinaryOp, Expression, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;

#[cfg(test)]
mod tests;

const MAX_CONSTRAINTS: usize = u64::BITS as usize;
const MAX_DEPENDENCIES: usize = 64;
const MAX_PROPAGATIONS: usize = 256;

#[derive(Clone, Default)]
pub(super) struct Constraints {
    // Forks share the index; only adding a new root copies it. Empty contexts
    // allocate nothing, and each context owns its propagation state.
    dependencies: Option<Rc<Dependencies>>,
    active: u64,
    pending: u64,
}

#[derive(Clone, Default)]
struct Dependencies {
    roots: Vec<usize>,
    // One bit per retained constraint, indexed by values its operands depend on.
    dependents: FxHashMap<usize, u64>,
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
            left,
            right,
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
        let dependencies = self.dependencies.as_deref();
        let index = if let Some(index) =
            dependencies.and_then(|d| d.roots.iter().position(|&root| root == id))
        {
            index
        } else {
            if dependencies.is_some_and(|d| d.roots.len() == MAX_CONSTRAINTS) {
                return;
            }
            let Some(inputs) = operand_dependencies(table, [left, right]) else {
                return;
            };
            let dependencies = Rc::make_mut(self.dependencies.get_or_insert_default());
            let index = dependencies.roots.len();
            dependencies.roots.push(id);
            for input in inputs {
                *dependencies.dependents.entry(input).or_default() |= 1 << index;
            }
            index
        };
        self.active |= 1 << index;
        self.pending |= 1 << index;
    }

    pub(super) fn changed(&mut self, id: usize) {
        if self.active == 0 {
            return;
        }
        if let Some(&dependents) = self.dependencies.as_ref().unwrap().dependents.get(&id) {
            self.pending |= dependents & self.active;
        }
    }

    pub(super) fn retain_known(&mut self, known: &FxHashMap<usize, Bits>) {
        let Some(dependencies) = &self.dependencies else {
            return;
        };
        // A join can forget the operand that settled a constraint on either path.
        self.active = dependencies
            .roots
            .iter()
            .enumerate()
            .filter(|(_, id)| known.contains_key(id))
            .fold(0, |mask, (index, _)| mask | (1 << index));
        self.pending = self.active;
    }

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.pending == 0 {
            return None;
        }
        let index = self.pending.trailing_zeros() as usize;
        self.pending &= self.pending - 1;
        Some((index, self.dependencies.as_ref().unwrap().roots[index]))
    }
}

/// Premises can become known through ranges, opposite comparisons or inner bits.
/// Watch their expression inputs as well as their result IDs. Discovery is bounded;
/// exceeding the budget only forgoes propagation for this constraint.
fn operand_dependencies(table: &ValueTable, roots: [usize; 2]) -> Option<Vec<usize>> {
    let mut pending = Vec::from(roots);
    let mut visited = Vec::new();
    while let Some(id) = pending.pop() {
        if table[id].scalar_literal().is_some() || visited.contains(&id) {
            continue;
        }
        if visited.len() == MAX_DEPENDENCIES {
            return None;
        }
        visited.push(id);
        if let Some(result) = table.expression(id) {
            pending.extend(result.expression.inputs());
        }
    }
    Some(visited)
}

impl ScalarFacts {
    pub(super) fn propagate_constraints(&mut self, table: &ValueTable) {
        for _ in 0..MAX_PROPAGATIONS {
            let Some((index, id)) = self.constraints.next() else {
                break;
            };
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
            // Facts only strengthen before a join. Once either input is known,
            // this constraint cannot contribute any further deductions here.
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
    }
}
