//! Known logical bits; facts about a truncated flag do not erase its other bits.

use std::collections::HashMap;

use super::super::ValueArena;
use crate::{
    integer::{BinaryOp, CompareOp},
    Type, ValueKind,
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
    fn bits(&self, arena: &ValueArena, id: usize) -> Bits {
        let value = arena.values[id];
        match value.kind {
            ValueKind::Constant(bits) => Bits {
                mask: value.ty.mask(),
                value: bits,
            },
            _ => self.0.get(&id).copied().unwrap_or_default(),
        }
    }

    pub(super) fn constant(&self, arena: &ValueArena, id: usize) -> Option<u64> {
        let bits = self.bits(arena, id);
        let mask = arena.values[id].ty.mask();
        (bits.mask & mask == mask).then_some(bits.value & mask)
    }

    pub(super) fn assume(&mut self, arena: &ValueArena, condition: usize, truth: bool) {
        let mut pending = vec![(
            condition,
            Bits {
                mask: 1,
                value: u64::from(truth),
            },
        )];
        while let Some((id, bits)) = pending.pop() {
            let value = arena.values[id];
            let bits = bits.restrict(value.ty.mask());
            let previous = self.bits(arena, id);
            // Contradictory facts describe an unreachable arm. Keeping the old
            // facts is sufficient: its constant controlling branch is folded away.
            if previous.conflicts(bits) || bits.mask & !previous.mask == 0 {
                continue;
            }
            let bits = previous.union(bits);
            self.0.insert(id, bits);
            match value.kind {
                ValueKind::Normalize(input) | ValueKind::Convert(input) => {
                    pending.push((input, bits))
                }
                ValueKind::Binary(BinaryOp::Or, a, b) => {
                    let zeros = Bits {
                        mask: bits.mask & !bits.value,
                        value: 0,
                    };
                    pending.push((a, zeros));
                    pending.push((b, zeros));
                }
                ValueKind::Binary(BinaryOp::And, a, b) => {
                    let ones = Bits {
                        mask: bits.value,
                        value: bits.value,
                    };
                    pending.push((a, ones));
                    pending.push((b, ones));
                    // A known mask exposes the same bits of the other operand.
                    for (input, other) in [(a, b), (b, a)] {
                        let other = self.bits(arena, other);
                        pending.push((input, bits.restrict(other.value)));
                    }
                }
                ValueKind::ZeroTest { input, nonzero } if bits.mask & 1 != 0 => {
                    let is_zero = (bits.value & 1 != 0) != nonzero;
                    if is_zero {
                        pending.push((
                            input,
                            Bits {
                                mask: arena.values[input].ty.mask(),
                                value: 0,
                            },
                        ));
                    } else if arena.values[input].ty == Type::I1
                        || arena.bounds[input].unsigned <= 1
                    {
                        pending.push((
                            input,
                            Bits {
                                mask: arena.values[input].ty.mask(),
                                value: 1,
                            },
                        ));
                    }
                }
                ValueKind::Compare(operator @ (CompareOp::Eq | CompareOp::Ne), a, b)
                    if bits.mask & 1 != 0
                        && (bits.value & 1 != 0) == (operator == CompareOp::Eq) =>
                {
                    pending.push((a, self.bits(arena, b)));
                    pending.push((b, self.bits(arena, a)));
                }
                ValueKind::Select {
                    condition,
                    when_true,
                    when_false,
                } => {
                    let truth = self
                        .constant(arena, condition)
                        .map(|value| value != 0)
                        .or_else(|| self.bits(arena, when_true).conflicts(bits).then_some(false))
                        .or_else(|| self.bits(arena, when_false).conflicts(bits).then_some(true));
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
        arena: &ValueArena,
        original: usize,
        rebuilt: usize,
    ) -> Option<u64> {
        let value = arena.values[rebuilt];
        let bits = match value.kind {
            ValueKind::Convert(input) | ValueKind::Normalize(input) => {
                let input_bits = self.bits(arena, input);
                // Convert also represents carrier aliases introduced by signed
                // lowering. Only proved zero upper bits can cross a widening.
                let width = arena.bounds[input].unsigned;
                let source_mask = u64::MAX.checked_shr(64 - u32::from(width)).unwrap_or(0);
                input_bits.union(Bits {
                    mask: value.ty.mask() & !source_mask,
                    value: 0,
                })
            }
            ValueKind::Binary(BinaryOp::And, a, b) => {
                let a = self.bits(arena, a);
                let b = self.bits(arena, b);
                let zeros = (a.mask & !a.value) | (b.mask & !b.value);
                let ones = a.value & b.value;
                Bits {
                    mask: zeros | ones,
                    value: ones,
                }
            }
            ValueKind::Binary(BinaryOp::Or, a, b) => {
                let a = self.bits(arena, a);
                let b = self.bits(arena, b);
                let zeros = a.mask & !a.value & b.mask & !b.value;
                let ones = a.value | b.value;
                Bits {
                    mask: zeros | ones,
                    value: ones,
                }
            }
            _ => Bits::default(),
        }
        .union(self.bits(arena, original))
        .union(self.bits(arena, rebuilt))
        .restrict(value.ty.mask());
        if bits.mask != 0 {
            self.0.insert(rebuilt, bits);
        }
        self.constant(arena, rebuilt)
    }
}
