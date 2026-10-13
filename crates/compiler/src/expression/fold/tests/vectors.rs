use super::*;

#[test]
fn packed_identities_preserve_the_complete_carrier_during_refolding() {
    use crate::expression::LaneBinaryOp;
    for ty in [Type::V64, Type::V128] {
        let mut values = ValueTable::default();
        let input = values.push(Value {
            ty,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        let zero = values.literal(ty, 0);
        let upper = values.carrier_literal(ty, 1_u128 << 100);
        let packed = |operator, right| Expression::Binary {
            operator: Binary::Vector {
                operator,
                lane_type: Type::I16,
            },
            left: input,
            right,
        };
        for op in [
            LaneBinaryOp::Add,
            LaneBinaryOp::AddSatSigned,
            LaneBinaryOp::SubSatUnsigned,
        ] {
            assert_eq!(refold(&mut values, ty, packed(op, zero)), input);
            assert_ne!(refold(&mut values, ty, packed(op, upper)), input);
        }
        for op in [
            LaneBinaryOp::Sub,
            LaneBinaryOp::SubSatSigned,
            LaneBinaryOp::GtSigned,
        ] {
            assert_eq!(refold(&mut values, ty, packed(op, input)), zero);
        }
        for op in [
            LaneBinaryOp::MinSigned,
            LaneBinaryOp::MinUnsigned,
            LaneBinaryOp::MaxSigned,
            LaneBinaryOp::MaxUnsigned,
        ] {
            assert_eq!(refold(&mut values, ty, packed(op, input)), input);
        }
        let equal = refold(&mut values, ty, packed(LaneBinaryOp::Eq, input));
        let ValueDefinition::Literal(bits) = values[equal].definition else {
            panic!("equality folds")
        };
        assert_eq!(u128::from(bits), u128::MAX);

        let low_ones = values.carrier_literal(ty, 0x0001_0001_0001_0001_u64);
        assert_ne!(
            refold(&mut values, ty, packed(LaneBinaryOp::Mul, low_ones)),
            input
        );
        let all_ones = values.carrier_literal(ty, 0x0001_0001_0001_0001_0001_0001_0001_0001_u128);
        assert_eq!(
            refold(&mut values, ty, packed(LaneBinaryOp::Mul, all_ones)),
            input
        );
    }
}

#[test]
fn vector_roundtrips_distinguish_logical_construction_from_carrier_refolding() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::V64,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let reinterpret = |input| Expression::Unary {
        operator: Unary::Reinterpret,
        input,
    };
    let bits = build(&mut values, Type::I64, reinterpret(input), 0);
    assert_eq!(build(&mut values, Type::V64, reinterpret(bits), 0), input);
    let carrier = refold(&mut values, Type::V64, reinterpret(bits));
    assert_ne!(carrier, input);
    assert_eq!(values.bounds[carrier].unsigned, 128);
    assert_eq!(refold(&mut values, Type::I64, reinterpret(carrier)), bits);

    let literal = values.literal(Type::I64, 7_u64);
    let logical = build(&mut values, Type::V64, reinterpret(literal), 0);
    let carrier = refold(&mut values, Type::V64, reinterpret(literal));
    let physical = |id: usize| match values[id].definition {
        ValueDefinition::Literal(bits) => u128::from(bits),
        _ => panic!("literal reinterpretation folds"),
    };
    assert_eq!(physical(logical), 7);
    assert_eq!(physical(carrier), 7 | (7 << 64));
}

#[test]
fn normalizing_a_vector_view_preserves_its_full_width_source_and_is_idempotent() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let view = build(
        &mut values,
        Type::V64,
        Expression::Unary {
            operator: Unary::Convert,
            input,
        },
        0,
    );
    assert_eq!(values.representation(view), input);
    let normalized = super::super::normalize(&mut values, view);
    assert_ne!(values.representation(normalized), input);
    assert_eq!(values.bounds[normalized].unsigned, 64);
    assert_eq!(super::super::normalize(&mut values, normalized), normalized);
    assert_eq!(values.bounds[input].unsigned, 128);
}

#[test]
fn scalar_backed_vector_logic_is_a_construction_choice() {
    let mut values = ValueTable::default();
    let bits = values.push(Value {
        ty: Type::I64,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let vector = build(
        &mut values,
        Type::V64,
        Expression::Unary {
            operator: Unary::Reinterpret,
            input: bits,
        },
        0,
    );
    let mask = values.literal(Type::V64, u64::MAX);
    let expression = Expression::Binary {
        operator: Binary::Bitwise(BitwiseOp::And),
        left: vector,
        right: mask,
    };
    assert_eq!(build(&mut values, Type::V64, expression, 0), vector);
    let carrier = refold(&mut values, Type::V64, expression);
    assert_ne!(carrier, vector);
    assert_eq!(values.bounds[vector].unsigned, 128);
    assert_eq!(values.bounds[carrier].unsigned, 64);
}

#[test]
fn bitwise_reinterpretation_reuses_integer_operations() {
    let mut values = ValueTable::default();
    let inputs = [0, 1].map(|component| {
        values.push(Value {
            ty: Type::I64,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component,
            },
        })
    });
    let vectors = inputs.map(|input| {
        build(
            &mut values,
            Type::V64,
            Expression::Unary {
                operator: Unary::Reinterpret,
                input,
            },
            0,
        )
    });
    let mask = values.literal(Type::V64, 0x1234_5678_9abc_def0_u64);
    let scalar_mask = values.literal(Type::I64, 0x1234_5678_9abc_def0_u64);
    for operator in [BitwiseOp::And, BitwiseOp::Or, BitwiseOp::Xor] {
        for (operands, sources) in [
            (vectors, inputs),
            ([mask, vectors[0]], [scalar_mask, inputs[0]]),
            ([vectors[0], mask], [inputs[0], scalar_mask]),
        ] {
            let binary = |[left, right]: [usize; 2]| Expression::Binary {
                operator: Binary::Bitwise(operator),
                left,
                right,
            };
            let result = build(&mut values, Type::V64, binary(operands), 0);
            let ValueDefinition::Expression(Expression::Unary {
                operator: Unary::Reinterpret,
                input,
            }) = values[result].definition
            else {
                panic!("the integer result has a vector view");
            };
            assert_eq!(values[input].ty, Type::I64);
            assert!(values[input].definition == ValueDefinition::Expression(binary(sources)));
        }
    }
}

#[test]
fn incompatible_bitwise_views_do_not_materialize_integer_literals() {
    for ty in [Type::V64, Type::V128] {
        let mut values = ValueTable::default();
        let input = values.push(Value {
            ty: if ty == Type::V64 { ty } else { Type::I64 },
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        // V64 has no existing integer encoding here. The V128 splat has one,
        // but it cannot represent the mask's different low and high words.
        let vector = if ty == Type::V64 {
            input
        } else {
            build(
                &mut values,
                ty,
                Expression::Unary {
                    operator: Unary::Reinterpret,
                    input,
                },
                0,
            )
        };
        let mask = values.literal(ty, 0x0123_4567_89ab_cdef_fedc_ba98_7654_3210_u128);
        for [left, right] in [[mask, vector], [vector, mask]] {
            let expression = Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::And),
                left,
                right,
            };
            let expected = values.intern(Value {
                ty,
                definition: ValueDefinition::Expression(expression),
            });
            let count = values.len();
            assert_eq!(build(&mut values, ty, expression, 0), expected);
            assert_eq!(
                values.len(),
                count,
                "a failed match creates no unused values"
            );
        }
    }
}
