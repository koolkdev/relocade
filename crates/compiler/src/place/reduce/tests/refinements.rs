use super::*;
use crate::Type;

#[test]
fn refinements_reuse_unrelated_answers_and_stop_at_an_unchanged_mask() {
    let mut values = ValueTable::default();
    let [input, other, guard] = [0, 1, 2].map(|component| {
        values.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component,
            },
        })
    });
    let mask = values.literal(Type::I32, 0xff);
    let masked = values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: input,
            right: mask,
        }),
    });
    let root = values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Integer(crate::integer::BinaryOp::Add),
            left: masked,
            right: other,
        }),
    });
    let mut reducer = Reducer::default();
    let parent = reducer.scope(ScalarFacts::default());
    assert_eq!(reducer.reduce(&mut values, parent, root), root);
    let unrelated = reducer.scope(reducer.facts(parent).clone().with_assumptions(
        &values,
        [Assumption::Bits {
            value: guard,
            mask: 1,
            bits: 1,
        }],
    ));
    assert_eq!(reducer.facts(unrelated).reduced(root), Some(root));
    let child = reducer.scope(reducer.facts(parent).clone().with_assumptions(
        &values,
        [Assumption::Bits {
            value: input,
            mask: 1 << 16,
            bits: 1 << 16,
        }],
    ));
    reducer.facts(child).refresh_bits(&values);
    // The input changed, but its masked result did not invalidate the later answer.
    assert_eq!(reducer.facts(child).reduced(root), Some(root));
    assert_eq!(reducer.reduce(&mut values, child, root), root);
    let constant = reducer.scope(reducer.facts(parent).clone().with_assumptions(
        &values,
        [Assumption::Bits {
            value: input,
            mask: u32::MAX.into(),
            bits: 7,
        }],
    ));
    assert_ne!(reducer.reduce(&mut values, constant, root), root);
    assert_eq!(reducer.reduce(&mut values, parent, root), root);
}
