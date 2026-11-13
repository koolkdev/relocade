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

fn assert_refinement(values: &mut ValueTable, root: usize, observation: Assumption, expected: u64) {
    let mut reducer = Reducer::default();
    let parent = reducer.scope(ScalarFacts::default());
    assert_eq!(reducer.reduce(values, parent, root), root);
    let child = reducer.scope(
        reducer
            .facts(parent)
            .clone()
            .with_assumptions(values, [observation]),
    );
    let result = reducer.reduce(values, child, root);
    assert_eq!(values[result].scalar_literal(), Some(expected));
    assert_eq!(reducer.reduce(values, parent, root), root);
}

#[test]
fn range_and_comparison_changes_revisit_reductions_that_skipped_inference() {
    use crate::{expression::CompareOp, integer::CompareOp as Integer};
    for (root_op, guard_op, root_limit, guard_limit, expected) in [
        (Integer::LtUnsigned, Integer::LtUnsigned, 20, 10, 1),
        (Integer::Eq, Integer::Ne, 20, 20, 0),
    ] {
        let mut values = ValueTable::default();
        let input = values.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        let [root, guard] =
            [(root_op, root_limit), (guard_op, guard_limit)].map(|(operator, limit)| {
                let limit = values.literal(Type::I32, limit as u64);
                values.intern(Value {
                    ty: Type::I1,
                    definition: ValueDefinition::Expression(Expression::Binary {
                        operator: Binary::Compare(CompareOp::Integer(operator)),
                        left: input,
                        right: limit,
                    }),
                })
            });
        assert_refinement(
            &mut values,
            root,
            Assumption::Truth {
                condition: guard,
                truth: true,
            },
            expected,
        );
    }
}

#[test]
fn overlapping_lane_changes_revisit_reductions_that_skipped_inference() {
    let mut values = ValueTable::default();
    let vector = values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let [root, word] = [(Type::I16, 1), (Type::I32, 0)].map(|(ty, lane)| {
        values.intern(Value {
            ty,
            definition: ValueDefinition::Expression(Expression::Unary {
                operator: Unary::ExtractLane { lane },
                input: vector,
            }),
        })
    });
    assert_refinement(
        &mut values,
        root,
        Assumption::Bits {
            value: word,
            mask: u32::MAX.into(),
            bits: 0x12345678,
        },
        0x1234,
    );
}
