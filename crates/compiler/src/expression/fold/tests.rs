//! Logical construction and operand replacement observe different bit widths.

use super::*;
use crate::{
    body::BlockId,
    integer::{BinaryOp, BitCountOp, CompareOp, ShiftOp},
};

fn refold(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    let id = values.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    });
    super::refold(values, id)
}

fn assert_constant(values: &ValueTable, id: usize, expected: u64) {
    let ValueDefinition::Constant(bits) = values[id].definition else {
        panic!("expected a constant");
    };
    assert_eq!(bits, expected);
}

#[test]
fn refolded_constants_retain_carrier_bits_and_unsigned_extension() {
    let mut values = ValueTable::default();
    let byte = values.constant(Type::I8, 255);
    let one = values.constant(Type::I8, 1);
    let sum = refold(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: BinaryOp::Add,
            left: byte,
            right: one,
        },
    );
    assert_constant(&values, sum, 256);
    assert_eq!(values.carrier_bits(sum, 0), 256);
    let view = refold(&mut values, Type::I32, Expression::Convert { input: sum });
    assert_constant(&values, view, 256);
    let negative = values.carrier_constant(Type::I8, u64::MAX);
    assert_eq!(values.carrier_bits(negative, 255), 0xffff_ffff);
    let unsigned = refold(
        &mut values,
        Type::I64,
        Expression::Convert { input: negative },
    );
    assert_constant(&values, unsigned, 0xffff_ffff);
    let zero = values.constant(Type::I8, 0);
    let positive = refold(
        &mut values,
        Type::I1,
        Expression::Compare {
            operator: CompareOp::GeSigned,
            left: byte,
            right: zero,
        },
    );
    assert_constant(&values, positive, 1);
}

#[test]
fn construction_interprets_logical_bits_of_carrier_constants() {
    let mut values = ValueTable::default();
    let input = values.carrier_constant(Type::I8, 256);
    let count = values.constant(Type::I32, 1);
    for (ty, expression, expected) in [
        (Type::I32, Expression::Convert { input }, 0),
        (Type::I32, Expression::SignExtend { input }, 0),
        (
            Type::I8,
            Expression::Shift {
                operator: ShiftOp::RightUnsigned,
                value: input,
                count,
            },
            0,
        ),
        (
            Type::I8,
            Expression::BitCount {
                operator: BitCountOp::LeadingZeros,
                input,
            },
            8,
        ),
        (
            Type::I8,
            Expression::BitCount {
                operator: BitCountOp::TrailingZeros,
                input,
            },
            8,
        ),
        (
            Type::I8,
            Expression::BitCount {
                operator: BitCountOp::Ones,
                input,
            },
            0,
        ),
        (
            Type::I1,
            Expression::ZeroTest {
                input,
                nonzero: true,
            },
            0,
        ),
    ] {
        let result = build(&mut values, ty, expression, 0);
        assert_constant(&values, result, expected);
    }
}

#[test]
fn offsets_and_explicit_masks_keep_their_observed_width() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::I8,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let offset = values.constant(Type::I8, 255);
    let one = values.constant(Type::I8, 1);
    let first = build(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: BinaryOp::Add,
            left: input,
            right: offset,
        },
        0,
    );
    let expression = Expression::Binary {
        operator: BinaryOp::Add,
        left: first,
        right: one,
    };
    assert_eq!(build(&mut values, Type::I8, expression, 0), input);
    let carried = refold(&mut values, Type::I8, expression);
    let ValueDefinition::Expression(Expression::Binary { left, right, .. }) =
        values[carried].definition
    else {
        panic!("carrier addition must retain the carry");
    };
    assert_eq!(left, input);
    assert_constant(&values, right, 256);
    let mask = values.carrier_constant(Type::I8, 511);
    let masked = refold(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: BinaryOp::And,
            left: carried,
            right: mask,
        },
    );
    assert!(matches!(
        values[masked].definition,
        ValueDefinition::Expression(Expression::LowBits { bits: 9, .. })
    ));
}

#[test]
fn constant_folding_keeps_division_traps_and_narrow_signed_results() {
    let mut values = ValueTable::default();
    for (ty, minimum) in [(Type::I32, 0x8000_0000), (Type::I64, 0x8000_0000_0000_0000)] {
        let left = values.constant(ty, minimum);
        let minus_one = values.constant(ty, u64::MAX);
        let zero = values.constant(ty, 0);
        for (operator, right) in [
            (BinaryOp::DivSigned, minus_one),
            (BinaryOp::DivUnsigned, zero),
            (BinaryOp::RemSigned, zero),
        ] {
            let result = refold(
                &mut values,
                ty,
                Expression::Binary {
                    operator,
                    left,
                    right,
                },
            );
            assert!(matches!(
                values[result].definition,
                ValueDefinition::Expression(_)
            ));
        }
    }
    let left = values.carrier_constant(Type::I8, 0xffff_ff80);
    let right = values.carrier_constant(Type::I8, 0xffff_ffff);
    let result = refold(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: BinaryOp::DivSigned,
            left,
            right,
        },
    );
    assert_constant(&values, result, 128);
}
