use super::Expression;
use crate::expression::BinaryOp as Binary;
use crate::expression::UnaryOp as Unary;

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_opcodes_keep_compact_value_storage() {
    use std::mem::size_of;
    assert_eq!(size_of::<Expression<usize>>(), 32);
    assert_eq!(size_of::<crate::body::Value>(), 40);
}

#[test]
fn packed_operation_identity_includes_its_lane_width_without_view_nodes() {
    use super::LaneBinaryOp;
    use crate::{
        body::{BlockId, Value, ValueDefinition, ValueTable},
        Type,
    };

    let mut values = ValueTable::default();
    let left = values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let right = values.literal(Type::V128, 1_u128);
    let mut sums = Vec::new();
    for lane_type in [Type::I8, Type::I16, Type::I32, Type::I64] {
        let expression = Expression::Binary {
            operator: Binary::Vector {
                operator: LaneBinaryOp::Add,
                lane_type,
            },
            left,
            right,
        };
        let before = values.len();
        let sum = super::build(&mut values, Type::V128, expression, 0);
        assert_eq!(values.len(), before + 1);
        assert!(!sums.contains(&sum));
        assert_eq!(super::build(&mut values, Type::V128, expression, 0), sum);
        sums.push(sum);
    }
}

#[test]
fn select_inputs_and_mapping_follow_wasm_stack_order() {
    let expression = Expression::Select {
        condition: String::from("condition"),
        when_true: String::from("true"),
        when_false: String::from("false"),
    };
    let forward: Vec<_> = expression.inputs().map(String::as_str).collect();
    let backward: Vec<_> = expression.inputs().rev().map(String::as_str).collect();
    assert_eq!(forward, ["true", "false", "condition"]);
    assert_eq!(backward, ["condition", "false", "true"]);

    let mut visited = Vec::new();
    let borrowed = expression.map(|input| {
        visited.push(input.as_str());
        input.as_str()
    });
    assert_eq!(visited, ["true", "false", "condition"]);
    assert!(matches!(
        borrowed,
        Expression::Select {
            condition: "condition",
            when_true: "true",
            when_false: "false",
        }
    ));
}

#[test]
fn input_mapping_stops_at_the_first_error() {
    let expression = Expression::Select {
        condition: 3,
        when_true: 1,
        when_false: 2,
    };
    let mut visited = Vec::new();
    let result = expression.try_map(|&input| {
        visited.push(input);
        if input == 2 {
            Err("unavailable input")
        } else {
            Ok(input)
        }
    });
    assert_eq!(visited, [1, 2]);
    assert!(matches!(result, Err("unavailable input")));
}

#[test]
fn refolding_lane_replacement_discards_narrow_carrier_bits() {
    use crate::{
        body::{Value, ValueDefinition, ValueTable},
        Type,
    };

    for (ty, carrier, expected) in [
        (Type::I8, 0x102_u64, 2_u128),
        (Type::I16, 0x10001, 1),
        (Type::I8, 0xffff_ffff, 0xff),
        (Type::I16, 0xffff_ffff, 0xffff),
    ] {
        let mut values = ValueTable::default();
        let vector = values.literal(Type::V128, 0_u128);
        let value = values.carrier_literal(ty, carrier);
        let expression = values.intern(Value {
            ty: Type::V128,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: Binary::ReplaceLane {
                    lane_type: ty,
                    lane: 0,
                },
                left: vector,
                right: value,
            }),
        });
        let folded = super::refold(&mut values, expression);
        let ValueDefinition::Literal(bits) = values[folded].definition else {
            panic!("literal lane replacement folds");
        };
        assert_eq!(u128::from(bits), expected);
    }
}

#[test]
fn refolding_matching_lane_extraction_normalizes_the_scalar_carrier() {
    use crate::{
        body::{BlockId, Value, ValueDefinition, ValueTable},
        Type,
    };

    for (ty, lane, carrier, expected) in [
        (Type::I8, 15, 0x102_u64, 2),
        (Type::I16, 7, 0x10001, 1),
        (Type::I8, 0, 0xffff_ffff, 0xff),
        (Type::I16, 0, 0xffff_ffff, 0xffff),
    ] {
        let mut values = ValueTable::default();
        let vector = values.push(Value {
            ty: Type::V128,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        // Execution reuse may substitute an i32 carrier for a byte or word.
        let value = values.carrier_literal(ty.carrier(), carrier);
        let input = values.intern(Value {
            ty: Type::V128,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: Binary::ReplaceLane {
                    lane_type: ty,
                    lane,
                },
                left: vector,
                right: value,
            }),
        });
        let expression = values.intern(Value {
            ty,
            definition: ValueDefinition::Expression(Expression::Unary {
                operator: Unary::ExtractLane { lane },
                input,
            }),
        });
        let folded = super::refold(&mut values, expression);
        assert_eq!(values[folded].scalar_literal(), Some(expected));
    }
}
