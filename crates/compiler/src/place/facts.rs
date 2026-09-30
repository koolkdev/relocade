//! Known logical bits; facts about a truncated flag do not erase its other bits.

use std::{cell::RefCell, collections::HashMap};

use crate::{
    body::{ValueDefinition, ValueTable},
    expression::Constant,
    integer::{low_mask, BinaryOp, CompareOp},
    Expression, Type,
};

#[derive(Clone, Copy, Default)]
struct Bits {
    mask: u64,
    value: u64,
}

impl Bits {
    fn conflicts(self, other: Self) -> bool {
        (self.value ^ other.value) & self.mask & other.mask != 0
    }

    fn union(self, other: Self) -> Self {
        Self {
            mask: self.mask | other.mask,
            value: self.value | other.value,
        }
    }

    fn restrict(self, mask: u64) -> Self {
        Self {
            mask: self.mask & mask,
            value: self.value & mask,
        }
    }
}

#[derive(Default)]
pub(super) struct Facts {
    known: HashMap<usize, Bits>,
    computed: RefCell<HashMap<usize, Bits>>,
}

impl Clone for Facts {
    fn clone(&self) -> Self {
        Self {
            known: self.known.clone(),
            computed: RefCell::default(),
        }
    }
}

impl Facts {
    fn bits(&self, table: &ValueTable, root: usize) -> Bits {
        let mut cache = self.computed.borrow_mut();
        if let Some(&bits) = cache.get(&root) {
            return bits;
        }
        let mut pending = vec![(root, false)];
        while let Some((id, ready)) = pending.pop() {
            if cache.contains_key(&id) {
                continue;
            }
            let value = table[id];
            let known = self.known.get(&id).copied().unwrap_or_default();
            if let ValueDefinition::Constant(bits) = value.definition {
                cache.insert(
                    id,
                    Bits {
                        mask: value.ty.mask(),
                        value: value.ty.normalize(bits),
                    },
                );
                continue;
            }
            if known.mask == value.ty.mask() {
                cache.insert(id, known);
                continue;
            }
            let ValueDefinition::Expression(expression) = value.definition else {
                cache.insert(id, known);
                continue;
            };
            // Resolve the selector first so known paths do not walk discarded
            // state histories merely to rediscover the chosen value.
            if let Expression::Select {
                condition,
                when_true,
                when_false,
            } = expression
            {
                let Some(condition_bits) = cache.get(&condition) else {
                    pending.push((id, false));
                    pending.push((condition, false));
                    continue;
                };
                if condition_bits.mask & 1 != 0 {
                    let input = if condition_bits.value & 1 != 0 {
                        when_true
                    } else {
                        when_false
                    };
                    if let Some(&bits) = cache.get(&input) {
                        cache.insert(id, bits.union(known).restrict(value.ty.mask()));
                    } else {
                        pending.push((id, false));
                        pending.push((input, false));
                    }
                    continue;
                }
            }
            if !ready {
                pending.push((id, true));
                pending.extend(expression.inputs().map(|&input| (input, false)));
                continue;
            }
            let inputs = expression.map(|&input| cache[&input]);
            let mut bits = match (expression, inputs) {
                (Expression::Convert { input }, Expression::Convert { input: bits }) => {
                    let width = table.bounds[input].unsigned;
                    let mask = low_mask(width);
                    bits.union(Bits {
                        mask: value.ty.mask() & !mask,
                        value: 0,
                    })
                }
                (
                    Expression::LowBits { bits: width, .. },
                    Expression::LowBits { input: bits, .. },
                ) => {
                    let mask = low_mask(width);
                    bits.restrict(mask).union(Bits {
                        mask: value.ty.mask() & !mask,
                        value: 0,
                    })
                }
                (
                    _,
                    Expression::Binary {
                        operator: BinaryOp::And,
                        left: a,
                        right: b,
                    },
                ) => {
                    let zeros = (a.mask & !a.value) | (b.mask & !b.value);
                    let ones = a.value & b.value;
                    Bits {
                        mask: zeros | ones,
                        value: ones,
                    }
                }
                (
                    _,
                    Expression::Binary {
                        operator: BinaryOp::Or,
                        left: a,
                        right: b,
                    },
                ) => {
                    let ones = a.value | b.value;
                    Bits {
                        mask: (a.mask & !a.value & b.mask & !b.value) | ones,
                        value: ones,
                    }
                }
                (
                    _,
                    Expression::Binary {
                        operator: BinaryOp::Xor,
                        left: a,
                        right: b,
                    },
                ) => {
                    let mask = a.mask & b.mask;
                    Bits {
                        mask,
                        value: (a.value ^ b.value) & mask,
                    }
                }
                (_, Expression::ZeroTest { input, nonzero }) if input.value != 0 => Bits {
                    mask: 1,
                    value: u64::from(nonzero),
                },
                _ => Bits::default(),
            };
            if let Ok(constants) = expression.try_map(|&input| {
                let bits = cache[&input];
                let ty = table[input].ty;
                if bits.mask & ty.mask() != ty.mask() {
                    return Err(());
                }
                Ok(Constant {
                    ty,
                    bits: table.carrier_bits(input, bits.value),
                })
            }) {
                if let Some(result) = constants.constant_result(value.ty) {
                    bits = Bits {
                        mask: value.ty.mask(),
                        value: result,
                    };
                }
            }
            cache.insert(id, bits.union(known).restrict(value.ty.mask()));
        }
        cache[&root]
    }

    pub(super) fn equal(&mut self, table: &ValueTable, id: usize, value: u64) {
        self.record(
            id,
            Bits {
                mask: table[id].ty.mask(),
                value,
            },
        );
    }

    fn record(&mut self, id: usize, bits: Bits) {
        self.known.insert(id, bits);
        // Calculations refer only to earlier values. Their cached inputs remain
        // valid when learning a fact about this value and its possible users.
        self.computed.get_mut().retain(|&input, _| input < id);
    }

    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
        if let ValueDefinition::Constant(bits) = table[id].definition {
            return Some(bits);
        }
        // Construction already folded path-independent constants.
        if self.known.is_empty() {
            return None;
        }
        let bits = self.bits(table, id);
        let mask = table.values[id].ty.mask();
        (bits.mask & mask == mask).then_some(bits.value & mask)
    }

    pub(super) fn assume(&mut self, table: &ValueTable, condition: usize, truth: bool) {
        let mut pending = vec![(
            condition,
            Bits {
                mask: 1,
                value: u64::from(truth),
            },
        )];
        while let Some((id, bits)) = pending.pop() {
            let value = table.values[id];
            let bits = bits.restrict(value.ty.mask());
            if bits.mask == 0 {
                continue;
            }
            let previous = self.bits(table, id);
            // Contradictory facts describe an unreachable arm. Keeping the old
            // facts is sufficient: its constant controlling branch is folded away.
            if previous.conflicts(bits) || bits.mask & !previous.mask == 0 {
                continue;
            }
            let bits = previous.union(bits);
            self.record(id, bits);
            let ValueDefinition::Expression(expression) = value.definition else {
                continue;
            };
            match expression {
                Expression::Convert { input } => pending.push((input, bits)),
                Expression::LowBits { input, bits: width } => {
                    pending.push((input, bits.restrict(low_mask(width))))
                }
                Expression::Binary {
                    operator: BinaryOp::Or,
                    left: a,
                    right: b,
                } => {
                    let zeros = Bits {
                        mask: bits.mask & !bits.value,
                        value: 0,
                    };
                    pending.push((a, zeros));
                    pending.push((b, zeros));
                }
                Expression::Binary {
                    operator: BinaryOp::And,
                    left: a,
                    right: b,
                } => {
                    let ones = Bits {
                        mask: bits.value,
                        value: bits.value,
                    };
                    pending.push((a, ones));
                    pending.push((b, ones));
                    // A known mask exposes the same bits of the other operand.
                    for (input, other) in [(a, b), (b, a)] {
                        let other = self.bits(table, other);
                        pending.push((input, bits.restrict(other.value)));
                    }
                }
                Expression::ZeroTest { input, nonzero } if bits.mask & 1 != 0 => {
                    let is_zero = (bits.value & 1 != 0) != nonzero;
                    if is_zero {
                        pending.push((
                            input,
                            Bits {
                                mask: table.values[input].ty.mask(),
                                value: 0,
                            },
                        ));
                    } else if table.values[input].ty == Type::I1
                        || table.bounds[input].unsigned <= 1
                    {
                        pending.push((
                            input,
                            Bits {
                                mask: table.values[input].ty.mask(),
                                value: 1,
                            },
                        ));
                    }
                }
                Expression::Compare {
                    operator: operator @ (CompareOp::Eq | CompareOp::Ne),
                    left: a,
                    right: b,
                } if bits.mask & 1 != 0 && (bits.value & 1 != 0) == (operator == CompareOp::Eq) => {
                    pending.push((a, self.bits(table, b)));
                    pending.push((b, self.bits(table, a)));
                }
                Expression::Select {
                    condition,
                    when_true,
                    when_false,
                } => {
                    let truth = self
                        .constant(table, condition)
                        .map(|value| value != 0)
                        .or_else(|| self.bits(table, when_true).conflicts(bits).then_some(false))
                        .or_else(|| self.bits(table, when_false).conflicts(bits).then_some(true));
                    if let Some(truth) = truth {
                        pending.push((
                            condition,
                            Bits {
                                mask: 1,
                                value: u64::from(truth),
                            },
                        ));
                        pending.push((if truth { when_true } else { when_false }, bits));
                    }
                }
                _ => {}
            }
        }
    }
}
