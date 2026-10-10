use super::super::{Assumption, ValueAnalysis};
use super::*;
use crate::{
    body::{BlockId, Value, ValueDefinition},
    Type,
};

fn input(table: &mut ValueTable, ty: Type) -> usize {
    table.push(Value {
        ty,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    })
}

#[test]
fn partial_shift_facts_use_masked_carrier_counts() {
    for (ty, high_zeros, counts) in [
        (Type::I32, 0xf000_0000, [4, 36, 68]),
        (Type::I64, 0xf000_0000_0000_0000, [4, 68, 132]),
    ] {
        let mut table = ValueTable::default();
        let input = input(&mut table, ty);
        let bits = Bits {
            mask: 0xff,
            value: 0xf0,
        };
        for count in counts {
            let count = Bits {
                mask: u64::MAX,
                value: count,
            };
            let left = bits.shifted(&table, input, ShiftOp::Left, count);
            assert_eq!((left.mask, left.value), (0xfff, 0xf00));
            let right = bits.shifted(&table, input, ShiftOp::RightUnsigned, count);
            assert_eq!((right.mask, right.value), (high_zeros | 15, 15));
        }
    }
}

#[test]
fn a_narrow_view_does_not_clear_upper_carrier_bits_before_a_shift() {
    let mut table = ValueTable::default();
    let source = input(&mut table, Type::I32);
    let view = table.intern(Value {
        ty: Type::I8,
        definition: ValueDefinition::Expression(crate::Expression::Convert { input: source }),
    });
    let bits = Bits {
        mask: 0xff,
        value: 0xf0,
    };
    for count in [8, 40] {
        let shifted = bits.shifted(
            &table,
            view,
            ShiftOp::RightUnsigned,
            Bits {
                mask: 31,
                value: count & 31,
            },
        );
        assert_eq!(shifted.mask & 0xff, 0);
    }
    let unshifted = bits.shifted(
        &table,
        view,
        ShiftOp::RightUnsigned,
        Bits { mask: 31, value: 0 },
    );
    assert_eq!((unshifted.mask, unshifted.value), (0xff, 0xf0));
}

#[test]
fn shift_boundaries_and_unknown_counts_keep_only_proved_bits() {
    for ty in [Type::I32, Type::I64] {
        let mut table = ValueTable::default();
        let input = input(&mut table, ty);
        let width = ty.bits();
        let top = 1_u64 << (width - 1);
        let bits = Bits { mask: 1, value: 1 };
        for count in [u64::from(width - 1), u64::from(2 * width - 1)] {
            let shifted = bits.shifted(
                &table,
                input,
                ShiftOp::Left,
                Bits {
                    mask: u64::MAX,
                    value: count,
                },
            );
            assert_eq!((shifted.mask, shifted.value), (ty.mask(), top));
        }
        let unknown = bits.shifted(&table, input, ShiftOp::Left, Bits { mask: 1, value: 1 });
        assert_eq!((unknown.mask, unknown.value), (0, 0));
    }
}

#[test]
fn constant_evaluation_cannot_invent_unknown_upper_shift_bits() {
    let mut table = ValueTable::default();
    let source = input(&mut table, Type::I32);
    let view = table.intern(Value {
        ty: Type::I8,
        definition: ValueDefinition::Expression(crate::Expression::Convert { input: source }),
    });
    let count = table.literal(Type::I32, 8);
    let shifted = table.intern(Value {
        ty: Type::I8,
        definition: ValueDefinition::Expression(crate::Expression::Shift {
            operator: ShiftOp::RightUnsigned,
            value: view,
            count,
        }),
    });
    let facts = ValueAnalysis::default().fork(
        &table,
        [Assumption::Bits {
            value: view,
            mask: 0xff,
            bits: 0,
        }],
    );
    assert_eq!(facts.constant(&table, view), Some(0));
    assert_eq!(facts.constant(&table, shifted), None);
}
