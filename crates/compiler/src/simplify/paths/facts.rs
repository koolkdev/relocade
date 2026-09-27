//! Known logical bits; facts about a truncated flag do not erase its other bits.

use std::collections::HashMap;

use crate::{
    body::{ValueDefinition, ValueTable},
    integer::{BinaryOp, CompareOp},
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

#[derive(Clone, Default)]
pub(super) struct Facts(HashMap<usize, Bits>);

impl Facts {
    fn bits(&self, table: &ValueTable, id: usize) -> Bits {
        let value = table.values[id];
        match value.definition {
            ValueDefinition::Constant(bits) => Bits {
                mask: value.ty.mask(),
                value: bits,
            },
            _ => self.0.get(&id).copied().unwrap_or_default(),
        }
    }

    pub(super) fn constant(&self, table: &ValueTable, id: usize) -> Option<u64> {
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
            let previous = self.bits(table, id);
            // Contradictory facts describe an unreachable arm. Keeping the old
            // facts is sufficient: its constant controlling branch is folded away.
            if previous.conflicts(bits) || bits.mask & !previous.mask == 0 {
                continue;
            }
            let bits = previous.union(bits);
            self.0.insert(id, bits);
            let ValueDefinition::Expression(expression) = value.definition else {
                continue;
            };
            match expression {
                Expression::Normalize { input } | Expression::Convert { input } => {
                    pending.push((input, bits))
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

    /// Rebuilding can expose a projection of bits already known on this path.
    /// Remember partial knowledge too, so a later mask or truncation can use it.
    pub(super) fn refine(
        &mut self,
        table: &ValueTable,
        original: usize,
        rebuilt: usize,
    ) -> Option<u64> {
        let value = table.values[rebuilt];
        let bits = match value.definition {
            ValueDefinition::Expression(expression) => match expression {
                Expression::Convert { input } | Expression::Normalize { input } => {
                    let input_bits = self.bits(table, input);
                    // Convert also represents carrier aliases introduced by signed
                    // lowering. Only proved zero upper bits can cross a widening.
                    let width = table.bounds[input].unsigned;
                    let source_mask = u64::MAX.checked_shr(64 - u32::from(width)).unwrap_or(0);
                    input_bits.union(Bits {
                        mask: value.ty.mask() & !source_mask,
                        value: 0,
                    })
                }
                Expression::Binary {
                    operator: BinaryOp::And,
                    left: a,
                    right: b,
                } => {
                    let a = self.bits(table, a);
                    let b = self.bits(table, b);
                    let zeros = (a.mask & !a.value) | (b.mask & !b.value);
                    let ones = a.value & b.value;
                    Bits {
                        mask: zeros | ones,
                        value: ones,
                    }
                }
                Expression::Binary {
                    operator: BinaryOp::Or,
                    left: a,
                    right: b,
                } => {
                    let a = self.bits(table, a);
                    let b = self.bits(table, b);
                    let zeros = a.mask & !a.value & b.mask & !b.value;
                    let ones = a.value | b.value;
                    Bits {
                        mask: zeros | ones,
                        value: ones,
                    }
                }
                _ => Bits::default(),
            },
            _ => Bits::default(),
        }
        .union(self.bits(table, original))
        .union(self.bits(table, rebuilt))
        .restrict(value.ty.mask());
        if bits.mask != 0 {
            self.0.insert(rebuilt, bits);
        }
        self.constant(table, rebuilt)
    }
}
