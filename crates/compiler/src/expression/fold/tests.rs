//! Logical construction and operand replacement observe different bit widths.

mod vectors;

use super::*;
use crate::expression::{
    BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, TernaryOp as Ternary,
    UnaryOp as Unary,
};
use crate::{
    bitwise::BitwiseOp,
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
    let Some(bits) = values[id].scalar_literal() else {
        panic!("expected a constant");
    };
    assert_eq!(bits, expected);
}

#[test]
fn scalar_roundtrips_distinguish_logical_width_from_carrier_truncation() {
    let mut values = ValueTable::default();
    let convert = |input| Expression::Unary {
        operator: Unary::Convert,
        input,
    };
    for source in [Type::I32, Type::I64] {
        let input = values.push(Value {
            ty: source,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        for narrow in [Type::I1, Type::I8, Type::I16, Type::I32] {
            let view = build(&mut values, narrow, convert(input), 0);
            let carrier = refold(&mut values, source, convert(view));
            assert_eq!(carrier == input, source == Type::I32);

            let logical = build(&mut values, source, convert(view), 0);
            assert_eq!(logical == input, source == narrow);
            assert_eq!(values.bounds[logical].unsigned, narrow.bits());
        }
    }
}

#[test]
fn sparse_masks_preserve_upper_carrier_bits_during_refolding() {
    for ty in [Type::I1, Type::I8, Type::I16] {
        let mut values = ValueTable::default();
        let input = values.push(Value {
            ty,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        let high_bit = values.carrier_literal(ty, 1_u64 << ty.bits());
        let one = values.literal(ty, 1);
        let binary = |operator, left, right| Expression::Binary {
            operator: Binary::Bitwise(operator),
            left,
            right,
        };
        let dynamic = refold(&mut values, ty, binary(BitwiseOp::And, input, high_bit));
        for operator in [BitwiseOp::Or, BitwiseOp::Xor] {
            let combined = refold(&mut values, ty, binary(operator, dynamic, one));
            let known = refold(&mut values, ty, binary(BitwiseOp::And, combined, one));
            assert_constant(&values, known, 1);
            let high = refold(&mut values, ty, binary(BitwiseOp::And, combined, high_bit));
            assert_eq!(values[high].scalar_literal(), None);
        }
    }
}

#[test]
fn masked_selections_preserve_upper_carrier_bits_during_refolding() {
    for ty in [Type::I1, Type::I8, Type::I16] {
        let mut values = ValueTable::default();
        let condition = values.push(Value {
            ty: Type::I1,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        let high = 1_u64 << ty.bits();
        let first = values.carrier_literal(ty, high | 1);
        let second = values.carrier_literal(ty, high);
        let selection = refold(
            &mut values,
            ty,
            Expression::Ternary {
                operator: Ternary::Select,
                first,
                second,
                third: condition,
            },
        );
        let mask = values.carrier_literal(ty, high);
        let result = refold(
            &mut values,
            ty,
            Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::And),
                left: selection,
                right: mask,
            },
        );
        assert_constant(&values, result, high);
    }
}

#[test]
fn xor_mask_combination_preserves_carrier_bits_during_refolding() {
    for ty in [Type::I1, Type::I8, Type::I16] {
        let mut values = ValueTable::default();
        let input = values.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        let view = refold(
            &mut values,
            ty,
            Expression::Unary {
                operator: Unary::Convert,
                input,
            },
        );
        let high_bit = 1_u64 << ty.bits();
        let first_mask = values.carrier_literal(ty, high_bit | 1);
        let second_mask = values.literal(ty, 1);
        let xor = |left, right| Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::Xor),
            left,
            right,
        };

        // Construction interprets a constant at its logical width, so both
        // masks mean one. Refolding must retain their distinct carrier bits.
        let constructed = build(&mut values, ty, xor(view, first_mask), 0);
        assert_eq!(
            build(&mut values, ty, xor(constructed, second_mask), 0),
            view
        );
        let first = refold(&mut values, ty, xor(view, first_mask));
        let combined = refold(&mut values, ty, xor(first, second_mask));
        let ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::Xor),
            left,
            right,
        }) = values[combined].definition
        else {
            panic!("the upper carrier bit still needs toggling")
        };
        assert_eq!(values.representation(left), input);
        assert_constant(&values, right, high_bit);
        let restored = refold(&mut values, ty, xor(first, first_mask));
        assert_eq!(values.representation(restored), input);
    }
}

#[test]
fn refolded_constants_retain_carrier_bits_and_unsigned_extension() {
    let mut values = ValueTable::default();
    let byte = values.literal(Type::I8, 255);
    let one = values.literal(Type::I8, 1);
    let sum = refold(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: byte,
            right: one,
        },
    );
    assert_constant(&values, sum, 256);
    assert_eq!(values.carrier_bits(sum, 0), Some(256));
    let view = refold(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input: sum,
        },
    );
    assert_constant(&values, view, 256);
    let negative = values.carrier_literal(Type::I8, u64::MAX);
    assert_eq!(values.carrier_bits(negative, 255), Some(0xffff_ffff_u64));
    let unsigned = refold(
        &mut values,
        Type::I64,
        Expression::Unary {
            operator: Unary::Convert,
            input: negative,
        },
    );
    assert_constant(&values, unsigned, 0xffff_ffff_u64);
    let zero = values.literal(Type::I8, 0);
    let positive = refold(
        &mut values,
        Type::I1,
        Expression::Binary {
            operator: Binary::Compare(Compare::Integer(CompareOp::GeSigned)),
            left: byte,
            right: zero,
        },
    );
    assert_constant(&values, positive, 1);
}

#[test]
fn construction_interprets_logical_bits_of_carrier_constants() {
    let mut values = ValueTable::default();
    let input = values.carrier_literal(Type::I8, 256);
    let count = values.literal(Type::I32, 1);
    for (ty, expression, expected) in [
        (
            Type::I32,
            Expression::Unary {
                operator: Unary::Convert,
                input,
            },
            0,
        ),
        (
            Type::I32,
            Expression::Unary {
                operator: Unary::SignExtend {
                    input_type: Type::I8,
                },
                input,
            },
            0,
        ),
        (
            Type::I8,
            Expression::Shift {
                operator: Shift::Scalar(ShiftOp::RightUnsigned),
                value: input,
                count,
            },
            0,
        ),
        (
            Type::I8,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator: BitCountOp::LeadingZeros,
                    input_type: Type::I8,
                },
                input,
            },
            8,
        ),
        (
            Type::I8,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator: BitCountOp::TrailingZeros,
                    input_type: Type::I8,
                },
                input,
            },
            8,
        ),
        (
            Type::I8,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator: BitCountOp::Ones,
                    input_type: Type::I8,
                },
                input,
            },
            0,
        ),
        (
            Type::I1,
            Expression::Unary {
                operator: Unary::ZeroTest { nonzero: true },
                input,
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
    let offset = values.literal(Type::I8, 255);
    let one = values.literal(Type::I8, 1);
    let first = build(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: input,
            right: offset,
        },
        0,
    );
    let expression = Expression::Binary {
        operator: Binary::Integer(BinaryOp::Add),
        left: first,
        right: one,
    };
    assert_eq!(build(&mut values, Type::I8, expression, 0), input);
    let carried = refold(&mut values, Type::I8, expression);
    let ValueDefinition::Expression(Expression::Binary {
        operator: Binary::Integer(_),
        left,
        right,
        ..
    }) = values[carried].definition
    else {
        panic!("carrier addition must retain the carry");
    };
    assert_eq!(left, input);
    assert_constant(&values, right, 256);
    let mask = values.carrier_literal(Type::I8, 511);
    let masked = refold(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: carried,
            right: mask,
        },
    );
    assert!(matches!(
        values[masked].definition,
        ValueDefinition::Expression(Expression::Unary {
            operator: Unary::LowBits { bits: 9 },
            ..
        })
    ));
}

#[test]
fn constant_folding_keeps_division_traps_and_narrow_signed_results() {
    let mut values = ValueTable::default();
    for (ty, minimum) in [
        (Type::I32, 0x8000_0000_u64),
        (Type::I64, 0x8000_0000_0000_0000_u64),
    ] {
        let left = values.literal(ty, minimum);
        let minus_one = values.literal(ty, u64::MAX);
        let zero = values.literal(ty, 0);
        for (operator, right) in [
            (BinaryOp::DivSigned, minus_one),
            (BinaryOp::DivUnsigned, zero),
            (BinaryOp::RemSigned, zero),
        ] {
            let result = refold(
                &mut values,
                ty,
                Expression::Binary {
                    operator: Binary::Integer(operator),
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
    let left = values.carrier_literal(Type::I8, 0xffff_ff80_u64);
    let right = values.carrier_literal(Type::I8, 0xffff_ffff_u64);
    let result = refold(
        &mut values,
        Type::I8,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::DivSigned),
            left,
            right,
        },
    );
    assert_constant(&values, result, 128);
}

#[test]
fn recovered_choice_conditions_normalize_dirty_carriers() {
    let mut values = ValueTable::default();
    let condition = values.push(Value {
        ty: Type::I1,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let first = values.carrier_literal(Type::I8, 0x102);
    let second = values.literal(Type::I8, 3);
    let choice = refold(
        &mut values,
        Type::I8,
        Expression::Ternary {
            operator: Ternary::Select,
            first,
            second,
            third: condition,
        },
    );
    for (right, expected_nonzero) in [(first, true), (second, false)] {
        let result = refold(
            &mut values,
            Type::I1,
            Expression::Binary {
                operator: Binary::Compare(Compare::Integer(CompareOp::Eq)),
                left: choice,
                right,
            },
        );
        assert_eq!(values.bounds[result].unsigned, 1);
        assert!(matches!(
            values[result].definition,
            ValueDefinition::Expression(Expression::Unary {
                operator: Unary::ZeroTest { nonzero },
                input,
            }) if nonzero == expected_nonzero && input == condition
        ));
    }
}

#[test]
fn boolean_folds_do_not_discard_upper_carrier_bits() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::I1,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let zero_test = refold(
        &mut values,
        Type::I1,
        Expression::Unary {
            operator: Unary::ZeroTest { nonzero: false },
            input,
        },
    );
    for (when_true, when_false) in [(1, 0), (0, 1)] {
        let when_true = values.literal(Type::I64, when_true);
        let when_false = values.literal(Type::I64, when_false);
        let numeric_bit = refold(
            &mut values,
            Type::I64,
            Expression::Ternary {
                operator: Ternary::Select,
                first: when_true,
                second: when_false,
                third: input,
            },
        );
        // A nonzero carrier such as 2 must still yield a canonical numeric bit.
        assert_eq!(values[numeric_bit].ty, Type::I64);
        assert_eq!(values.bounds[numeric_bit].unsigned, 1);
    }
    for operator in [BitwiseOp::Or, BitwiseOp::Xor] {
        let result = refold(
            &mut values,
            Type::I1,
            Expression::Binary {
                operator: Binary::Bitwise(operator),
                left: input,
                right: zero_test,
            },
        );
        // For carrier input 2 these produce 2, despite the logical I1 type.
        assert!(!matches!(
            values[result].definition,
            ValueDefinition::Literal(_)
        ));
        assert_eq!(values.bounds[result].unsigned, 32);
    }
    let one_with_upper_bits = values.carrier_literal(Type::I8, 0x101);
    let zero = values.literal(Type::I8, 0);
    let byte_choice = refold(
        &mut values,
        Type::I8,
        Expression::Ternary {
            operator: Ternary::Select,
            first: one_with_upper_bits,
            second: zero,
            third: zero_test,
        },
    );
    assert_eq!(values.bounds[byte_choice].unsigned, 9);
    assert!(matches!(
        values[byte_choice].definition,
        ValueDefinition::Expression(Expression::Ternary {
            operator: Ternary::Select,
            ..
        })
    ));
}
