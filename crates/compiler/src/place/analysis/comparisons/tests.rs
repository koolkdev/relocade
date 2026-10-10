use super::*;
use crate::{
    body::{BlockId, Value, ValueDefinition},
    place::analysis::{Assumption, ValueAnalysis},
    Expression, Type,
};

#[test]
fn later_observations_refine_comparisons_already_visited_in_the_same_batch() {
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
    let other = table.push(Value {
        ty: Type::I1,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 2,
        },
    });
    let either = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Bitwise {
            operator: crate::bitwise::BitwiseOp::Or,
            left: equal,
            right: other,
        }),
    });
    // The first observation visits `equal` but cannot decide it. Learning its
    // opposite afterwards must discard that provisional answer before publication.
    let facts = ValueAnalysis::default().fork(
        &table,
        [
            Assumption::Truth {
                condition: either,
                truth: true,
            },
            Assumption::Truth {
                condition: unequal,
                truth: true,
            },
        ],
    );
    assert_eq!(facts.constant(&table, equal), Some(0));
}
