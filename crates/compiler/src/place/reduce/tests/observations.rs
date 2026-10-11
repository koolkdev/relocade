//! Partial consumers cannot establish equivalence of complete values.
use super::*;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};
use crate::{integer::BinaryOp, Type};

fn parameter(values: &mut ValueTable, ty: Type, component: usize) -> usize {
    values.push(Value {
        ty,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component,
        },
    })
}

fn expression(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    values.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

#[test]
fn partial_reassembly_never_answers_a_complete_observation_in_either_query_order() {
    for full_first in [false, true] {
        let mut values = ValueTable::default();
        let input = parameter(&mut values, Type::V128, 0);
        let low = expression(
            &mut values,
            Type::I64,
            Expression::Unary {
                operator: Unary::ExtractLane { lane: 0 },
                input,
            },
        );
        let zero = values.literal(Type::V128, 0_u128);
        let restored = expression(
            &mut values,
            Type::V128,
            Expression::Binary {
                operator: Binary::ReplaceLane {
                    lane_type: Type::I64,
                    lane: 0,
                },
                left: zero,
                right: low,
            },
        );
        let extracted = expression(
            &mut values,
            Type::I64,
            Expression::Unary {
                operator: Unary::ExtractLane { lane: 0 },
                input: restored,
            },
        );
        let mut reducer = Reducer::default();
        if full_first {
            assert_eq!(reducer.reduce(&mut values, restored), restored);
        }
        assert_eq!(reducer.reduce(&mut values, extracted), low);
        assert!(!reducer.recipes(input).any(|recipe| recipe == restored));
        assert_eq!(reducer.reduce(&mut values, restored), restored);
        assert_eq!(reducer.reduce(&mut values, extracted), low);
    }
}

#[test]
fn a_narrow_result_removes_input_masks_without_changing_a_full_use() {
    let mut values = ValueTable::default();
    let left = parameter(&mut values, Type::I32, 0);
    let right = parameter(&mut values, Type::I32, 1);
    let mask = values.literal(Type::I32, 255);
    let masked = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left,
            right: mask,
        },
    );
    let sum = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: masked,
            right,
        },
    );
    let narrow = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::LowBits { bits: 8 },
            input: sum,
        },
    );
    let unmasked_sum = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left,
            right,
        },
    );
    let expected = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::LowBits { bits: 8 },
            input: unmasked_sum,
        },
    );
    let mut reducer = Reducer::default();
    assert_eq!(reducer.reduce(&mut values, narrow), expected);
    assert_eq!(reducer.reduce(&mut values, sum), sum);
    assert!(!reducer.recipes(left).any(|recipe| recipe == masked));
    assert!(reducer.recipes(expected).any(|recipe| recipe == narrow));
}

#[test]
fn narrowing_a_carrier_can_discard_high_input_bits_but_a_type_view_cannot() {
    let mut values = ValueTable::default();
    let input = parameter(&mut values, Type::I64, 0);
    let mask = values.literal(Type::I64, 0xffff_ffff_u64);
    let masked = expression(
        &mut values,
        Type::I64,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: input,
            right: mask,
        },
    );
    let narrow = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input: masked,
        },
    );
    let expected = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input,
        },
    );
    let mut reducer = Reducer::default();
    assert_eq!(reducer.reduce(&mut values, narrow), expected);
    assert_eq!(reducer.reduce(&mut values, masked), masked);

    let input = parameter(&mut values, Type::I32, 1);
    let mask = values.literal(Type::I32, 255);
    let masked = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: input,
            right: mask,
        },
    );
    let byte = expression(
        &mut values,
        Type::I8,
        Expression::Unary {
            operator: Unary::Convert,
            input: masked,
        },
    );
    assert_eq!(reducer.reduce(&mut values, byte), byte);
}

#[test]
fn a_known_low_byte_does_not_replace_its_dirty_carrier() {
    let mut graph = conditional_value();
    let condition = graph.blocks[0].parameters[0];
    let yes = graph.values.carrier_literal(Type::I8, 0x100);
    let no = graph.values.carrier_literal(Type::I8, 0x200);
    let choice = expression(
        &mut graph.values,
        Type::I8,
        Expression::Select {
            condition,
            when_true: yes,
            when_false: no,
        },
    );
    let byte = expression(
        &mut graph.values,
        Type::I8,
        Expression::Unary {
            operator: Unary::LowBits { bits: 8 },
            input: choice,
        },
    );
    let carrier = expression(
        &mut graph.values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input: choice,
        },
    );
    let mut reducer = Reducer::default();
    let low = reducer.reduce(&mut graph.values, byte);
    assert_eq!(graph.values[low].scalar_literal(), Some(0));
    assert_eq!(reducer.reduce(&mut graph.values, carrier), carrier);
    assert!(!reducer.recipes(low).any(|recipe| recipe == choice));
    for (truth, expected) in [(true, 0x100), (false, 0x200)] {
        let mut path = branch(&reducer, &graph.values, condition, truth);
        let result = path.reduce(&mut graph.values, carrier);
        assert_eq!(graph.values[result].scalar_literal(), Some(expected));
    }
}

#[test]
fn refolded_partial_recipes_finish_before_the_full_parent_is_published() {
    let mut values = ValueTable::default();
    let input = parameter(&mut values, Type::I32, 0);
    let mask = values.literal(Type::I32, 255);
    let masked = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: input,
            right: mask,
        },
    );
    let one = values.literal(Type::I32, 1);
    let carry = values.literal(Type::I32, 256);
    let sum = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: masked,
            right: one,
        },
    );
    let shifted = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: sum,
            right: carry,
        },
    );
    let root = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::LowBits { bits: 8 },
            input: shifted,
        },
    );
    let sum = expression(
        &mut values,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: input,
            right: one,
        },
    );
    let expected = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::LowBits { bits: 8 },
            input: sum,
        },
    );
    let mut reducer = Reducer::default();
    assert_eq!(reducer.reduce(&mut values, root), expected);
    assert_eq!(reducer.reduce(&mut values, root), expected);
    assert!(reducer.recipes(expected).any(|recipe| recipe == root));
}
