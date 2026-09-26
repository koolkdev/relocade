//! Rebuild calculations at their uses; snapshots and effect results keep identity.

use super::{super::ValueArena, Path};
use crate::{Value, ValueKind};

impl Path {
    pub(super) fn value(&mut self, arena: &mut ValueArena, root: &mut usize) {
        let mut pending = vec![(*root, false)];
        while let Some((id, ready)) = pending.pop() {
            if self.rewritten.contains_key(&id) {
                continue;
            }
            let value = arena.values[id];
            if let Some(bits) = self.facts.constant(arena, id) {
                let result = constant(arena, id, bits);
                self.rewritten.insert(id, result);
                continue;
            }
            if let Some(&shared) = self.shared.get(&id) {
                // A known selection can use an input or snapshot directly.
                // Calculated arms retain the group's rewrite: exposing one
                // separately could make placement repeat it on the same path.
                let result = match arena.values[shared].kind {
                    ValueKind::Select {
                        condition,
                        when_true,
                        when_false,
                    } => match self.facts.constant(arena, condition) {
                        Some(0) => when_false,
                        Some(_) => when_true,
                        None => shared,
                    },
                    _ => shared,
                };
                self.rewritten.insert(
                    id,
                    if arena.values[result].kind.is_calculation() {
                        shared
                    } else {
                        result
                    },
                );
                continue;
            }
            if !value.kind.is_calculation() {
                self.rewritten.insert(id, id);
                continue;
            }
            if !ready {
                pending.push((id, true));
                for input in value.kind.inputs() {
                    if !self.rewritten.contains_key(&input) {
                        pending.push((input, false));
                    }
                }
                continue;
            }
            let input = |id| self.rewritten[&id];
            let rebuilt = match value.kind {
                ValueKind::Binary(op, a, b) => {
                    let (a, b) = (input(a), input(b));
                    if arena.values[a].ty == value.ty && arena.values[b].ty == value.ty {
                        arena.binary(op, a, b)
                    } else {
                        arena.intern(Value {
                            ty: value.ty,
                            kind: ValueKind::Binary(op, a, b),
                        })
                    }
                }
                ValueKind::Compare(op, a, b) => arena.compare(op, input(a), input(b)),
                ValueKind::Shift {
                    operator,
                    value: shifted,
                    count,
                } => arena.intern(Value {
                    ty: value.ty,
                    kind: ValueKind::Shift {
                        operator,
                        value: input(shifted),
                        count: input(count),
                    },
                }),
                ValueKind::Rotate {
                    operator,
                    value,
                    count,
                } => arena.rotate(operator, input(value), input(count)),
                ValueKind::Select {
                    condition,
                    when_true,
                    when_false,
                } => arena.select(input(condition), input(when_true), input(when_false)),
                ValueKind::Normalize(i) => arena.normalize(input(i)),
                ValueKind::Convert(i) => arena.intern(Value {
                    ty: value.ty,
                    kind: ValueKind::Convert(input(i)),
                }),
                ValueKind::SignExtend(i) => arena.sign_extend(input(i), value.ty),
                ValueKind::BitCount(op, i) => arena.bit_count(op, input(i)),
                ValueKind::ZeroTest { input: i, nonzero } => arena.zero_test(input(i), nonzero),
                _ => unreachable!("only calculations have inputs to rebuild"),
            };
            let result = match self.facts.refine(arena, id, rebuilt) {
                Some(bits) => constant(arena, id, bits),
                None => rebuilt,
            };
            // Existing joins retain bounds computed during construction. A rewrite
            // must honor those carrier promises as well as the logical result.
            let original = arena.bounds[id];
            let replacement = arena.bounds[result];
            self.rewritten.insert(
                id,
                if replacement.unsigned <= original.unsigned
                    && replacement.signed <= original.signed
                {
                    result
                } else {
                    id
                },
            );
        }
        *root = self.rewritten[root];
    }
}

fn constant(arena: &mut ValueArena, original: usize, bits: u64) -> usize {
    let ty = arena.values[original].ty;
    let constant = arena.constant(ty, bits);
    if ty.bits() < 32
        && arena.bounds[original].signed <= ty.bits()
        && bits & (1 << (ty.bits() - 1)) != 0
    {
        // A narrow signed carrier may contain ones above its logical width.
        // Preserve that representation for consumers whose lowering relies on it.
        arena.intern(Value {
            ty,
            kind: ValueKind::SignExtend(constant),
        })
    } else {
        constant
    }
}
