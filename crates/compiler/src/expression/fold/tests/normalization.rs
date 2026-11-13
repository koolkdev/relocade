use super::*;

#[test]
fn normalized_zero_refolding_preserves_dirty_carrier_aliases() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let byte = refold(
        &mut values,
        Type::I8,
        Expression::Unary {
            operator: Unary::Convert,
            input,
        },
    );
    assert_eq!(values.bounds[byte].unsigned, 32);
    for input_type in [Type::I32, Type::I8] {
        let count = refold(
            &mut values,
            Type::I32,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator: BitCountOp::LeadingZeros,
                    input_type,
                },
                input: byte,
            },
        );
        let normalized = refold(
            &mut values,
            Type::I32,
            Expression::Shift {
                operator: Shift::Scalar(ShiftOp::Left),
                value: input,
                count,
            },
        );
        for nonzero in [false, true] {
            let test = |input| Expression::Unary {
                operator: Unary::ZeroTest { nonzero },
                input,
            };
            let expected = refold(&mut values, Type::I1, test(input));
            let actual = refold(&mut values, Type::I1, test(normalized));
            // Full-carrier CLZ sees upper bits in the byte view. A logical byte
            // count does not establish the same normalization proof.
            assert_eq!(actual == expected, input_type == Type::I32);
        }
    }
}
