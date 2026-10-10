use super::*;
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
            definition: ValueDefinition::Expression(Expression::Compare {
                operator,
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
