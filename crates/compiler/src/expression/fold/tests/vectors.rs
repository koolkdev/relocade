use super::*;

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
