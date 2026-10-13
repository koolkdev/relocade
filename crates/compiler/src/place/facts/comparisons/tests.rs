use super::*;
use crate::expression::{BinaryOp as Binary, CompareOp as Compare};
use crate::{
    body::{BlockId, Value, ValueDefinition},
    place::facts::ScalarFacts,
    Expression, Type,
};

#[test]
fn a_later_comparison_invalidates_cached_inference_for_its_earlier_opposite() {
    let mut table = ValueTable::default();
    let [left, right] = [0, 1].map(|component| {
        table.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component,
            },
        })
    });
    let [equal, unequal] = [CompareOp::Eq, CompareOp::Ne].map(|operator| {
        table.intern(Value {
            ty: Type::I1,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: Binary::Compare(Compare::Integer(operator)),
                left,
                right,
            }),
        })
    });
    let mut facts = ScalarFacts::default();
    assert_eq!(facts.bits(&table, equal).mask, 0);
    facts.assume(&table, unequal, true);
    assert_eq!(facts.constant(&table, equal), Some(0));
}

#[test]
fn converted_comparisons_invalidate_earlier_predicates_and_their_consumers() {
    let mut table = ValueTable::default();
    let [left, right] = [0, 1].map(|component| {
        table.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component,
            },
        })
    });
    let equal = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Compare(Compare::Integer(CompareOp::Eq)),
            left,
            right,
        }),
    });
    let consumer = table.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: crate::expression::UnaryOp::Convert,
            input: equal,
        }),
    });
    let [left, right] = [left, right].map(|input| {
        table.intern(Value {
            ty: Type::I16,
            definition: ValueDefinition::Expression(Expression::Unary {
                operator: crate::expression::UnaryOp::Convert,
                input,
            }),
        })
    });
    let unequal = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Compare(Compare::Integer(CompareOp::Ne)),
            left,
            right,
        }),
    });
    let mut facts = ScalarFacts::default();
    assert_eq!(facts.bits(&table, equal).mask, 0);
    assert_eq!(facts.bits(&table, consumer).mask & 1, 0);
    facts.assume(&table, unequal, true);
    assert_eq!(facts.constant(&table, equal), Some(0));
    assert_eq!(facts.constant(&table, consumer), Some(0));
}

#[test]
fn comparison_refinement_preserves_unrelated_earlier_inference() {
    let mut table = ValueTable::default();
    let limit = table.literal(Type::I32, 10);
    let unrelated = table.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let earlier = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Compare(Compare::Integer(CompareOp::Eq)),
            left: unrelated,
            right: limit,
        }),
    });
    let input = table.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 1,
        },
    });
    let later = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Compare(Compare::Integer(CompareOp::LtUnsigned)),
            left: input,
            right: limit,
        }),
    });
    let mut facts = ScalarFacts::default();
    assert_eq!(facts.bits(&table, earlier).mask, 0);
    facts.assume(&table, later, true);
    assert!(facts.computed.borrow().contains_key(&earlier));
    assert_eq!(facts.constant(&table, earlier), None);
}
