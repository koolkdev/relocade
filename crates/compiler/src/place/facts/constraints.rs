//! Propagate retained Boolean constraints when facts about their inputs change.

use super::{Bits, ScalarFacts};
use crate::{bitwise::BitwiseOp, body::ValueTable, expression::BinaryOp, Expression, Type};
use rustc_hash::FxHashMap;
use std::rc::Rc;

#[cfg(test)]
mod tests;

const MAX_CANDIDATES: usize = 4;
const MAX_CONSTRAINTS: usize = u64::BITS as usize;
const MAX_DEPENDENCIES: usize = 64;
const MAX_PROPAGATIONS: usize = 256;

#[derive(Clone, Default)]
pub(super) struct Constraints {
    retained: Option<Rc<RetainedConstraints>>,
    active: u64,
    pending: u64,
}

#[derive(Clone, Default)]
struct RetainedConstraints {
    roots: Vec<usize>,
    // A few unresolved candidates need no dependency graph. Index them before
    // deriving a fact, or when broad notifications would scan too many roots.
    dependents: Option<FxHashMap<usize, u64>>,
}

impl Constraints {
    fn indexed(&self) -> bool {
        self.retained
            .as_ref()
            .is_some_and(|retained| retained.dependents.is_some())
    }

    fn track(&mut self, table: &ValueTable, id: usize, ready: bool) {
        let retained = self.retained.as_deref();
        let existing =
            retained.and_then(|retained| retained.roots.iter().position(|&root| root == id));
        let index = if let Some(index) = existing {
            index
        } else {
            if retained.is_some_and(|retained| {
                retained.dependents.is_none() && retained.roots.len() == MAX_CANDIDATES
            }) {
                self.index(table);
            }
            if self
                .retained
                .as_ref()
                .is_some_and(|retained| retained.roots.len() == MAX_CONSTRAINTS)
            {
                return;
            }
            let inputs = if self.indexed() {
                let Some(inputs) = operand_dependencies(table, id) else {
                    return;
                };
                Some(inputs)
            } else {
                None
            };
            let retained = Rc::make_mut(self.retained.get_or_insert_default());
            let index = retained.roots.len();
            retained.roots.push(id);
            if let Some(inputs) = inputs {
                let dependents = retained.dependents.as_mut().unwrap();
                for input in inputs {
                    *dependents.entry(input).or_default() |= 1 << index;
                }
            }
            index
        };
        self.active |= 1 << index;
        if ready {
            self.pending |= 1 << index;
        }
    }

    /// Validate deferred candidates against the same discovery limit as indexed
    /// roots. No candidate can contribute a deduction before this check.
    fn index(&mut self, table: &ValueTable) {
        let Some(retained) = &self.retained else {
            return;
        };
        let mut indexed = RetainedConstraints::default();
        let mut dependents = FxHashMap::<usize, u64>::default();
        let mut active = 0;
        for (previous, &id) in retained.roots.iter().enumerate() {
            let Some(inputs) = operand_dependencies(table, id) else {
                continue;
            };
            let index = indexed.roots.len();
            indexed.roots.push(id);
            for input in inputs {
                *dependents.entry(input).or_default() |= 1 << index;
            }
            if self.active & (1 << previous) != 0 {
                active |= 1 << index;
            }
        }
        indexed.dependents = Some(dependents);
        self.retained = Some(Rc::new(indexed));
        self.active = active;
        // This also requeues the candidate whose deduction requested indexing.
        self.pending = active;
    }

    pub(super) fn changed(&mut self, id: usize) {
        if self.active == 0 {
            return;
        }
        let retained = self.retained.as_ref().unwrap();
        if let Some(dependents) = &retained.dependents {
            if let Some(&dependents) = dependents.get(&id) {
                self.pending |= dependents & self.active;
            }
        } else {
            let mut remaining = self.active;
            while remaining != 0 {
                let index = remaining.trailing_zeros() as usize;
                remaining &= remaining - 1;
                // Expressions only refer to earlier values. Changes to a later
                // value cannot establish either premise of this candidate.
                if id < retained.roots[index] {
                    self.pending |= 1 << index;
                }
            }
        }
    }

    pub(super) fn retain_known(&mut self, known: &FxHashMap<usize, Bits>) {
        let Some(retained) = &self.retained else {
            return;
        };
        // A join can forget the operand that settled a constraint on either path.
        self.active = retained
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
        Some((index, self.retained.as_ref().unwrap().roots[index]))
    }
}

/// Premises can become known through ranges, opposite comparisons or inner bits.
/// Discovery is bounded; exceeding the budget only forgoes propagation.
fn operand_dependencies(table: &ValueTable, root: usize) -> Option<Vec<usize>> {
    let Expression::Binary { left, right, .. } = table.expression(root).unwrap().expression else {
        unreachable!()
    };
    let mut pending = vec![left, right];
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
    pub(super) fn track_constraint(&mut self, table: &ValueTable, id: usize, bits: Bits) {
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
        if matches!(operator, BitwiseOp::And) && bits.value & 1 != 0
            || matches!(operator, BitwiseOp::Or) && bits.value & 1 == 0
        {
            return;
        }
        // Inference of this guard already visited its operands. Recording the
        // result invalidates later values, so their inferred bits remain available.
        let ready = {
            let computed = self.computed.borrow();
            [left, right].into_iter().any(|input| {
                computed
                    .get(&input)
                    .is_some_and(|bits| bits.mask == table[input].ty.mask())
            })
        };
        self.constraints.track(table, id, ready);
    }

    pub(super) fn propagate_constraints(&mut self, table: &ValueTable) {
        let mut remaining = MAX_PROPAGATIONS;
        while remaining != 0 {
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
                remaining -= 1;
                continue;
            }
            let premise = match (a, b) {
                (Some(known), None) => Some((right, known)),
                (None, Some(known)) => Some((left, known)),
                _ => None,
            };
            let deduction = premise.and_then(|(input, known)| {
                let inferred = match operator {
                    BitwiseOp::And if known => Some(truth),
                    BitwiseOp::Or if !known => Some(truth),
                    BitwiseOp::Xor => Some(truth ^ known),
                    _ => None,
                };
                inferred.map(|truth| (input, truth))
            });
            if deduction.is_some() && !self.constraints.indexed() {
                self.constraints.index(table);
                continue;
            }
            remaining -= 1;
            // Facts only strengthen before a join. A settled constraint cannot
            // contribute another deduction until its premises weaken at a join.
            self.constraints.active &= !(1 << index);
            if let Some((input, truth)) = deduction {
                self.assume(table, input, truth);
            }
        }
    }
}
