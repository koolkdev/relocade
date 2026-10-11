use super::Expression;

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
            definition: ValueDefinition::Expression(Expression::VectorReplace {
                vector,
                value,
                lane: 0,
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
        let value = values.carrier_literal(ty, carrier);
        let input = values.intern(Value {
            ty: Type::V128,
            definition: ValueDefinition::Expression(Expression::VectorReplace {
                vector,
                value,
                lane,
            }),
        });
        let expression = values.intern(Value {
            ty,
            definition: ValueDefinition::Expression(Expression::VectorExtract { input, lane }),
        });
        let folded = super::refold(&mut values, expression);
        assert_eq!(values[folded].scalar_literal(), Some(expected));
    }
}
