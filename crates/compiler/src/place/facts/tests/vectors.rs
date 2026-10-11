//! Vector bit facts obey the same scope and lane semantics as scalar observations.

use super::*;
use crate::{expression::LaneBinaryOp, Type};

fn expression(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    values.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

fn vector(values: &mut ValueTable) -> usize {
    values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    })
}

#[test]
fn scalar_observation_proves_only_its_vector_lane_and_leaves_with_its_scope() {
    let mut values = ValueTable::default();
    let input = vector(&mut values);
    let low = expression(
        &mut values,
        Type::I64,
        Expression::VectorExtract { input, lane: 0 },
    );
    let high = expression(
        &mut values,
        Type::I64,
        Expression::VectorExtract { input, lane: 1 },
    );
    let scalar_zero = values.literal(Type::I64, 0);
    let condition = expression(
        &mut values,
        Type::I1,
        Expression::Compare {
            operator: CompareOp::Eq,
            left: high,
            right: scalar_zero,
        },
    );
    let zero = values.literal(Type::V128, 0_u128);
    let restored = expression(
        &mut values,
        Type::V128,
        Expression::VectorReplace {
            vector: zero,
            value: low,
            lane: 0,
        },
    );
    let mut facts = ValueFacts::default();
    assert_eq!(facts.identity(&values, restored), None);
    let checkpoint = facts.checkpoint();
    facts.assume(&values, condition, true);
    assert_eq!(
        facts.bits(&values, input),
        KnownBits {
            mask: u128::from(u64::MAX) << 64,
            value: 0
        }
    );
    assert_eq!(facts.identity(&values, restored), Some(input));
    assert_eq!(facts.constant(&values, input), None);
    let snapshot = facts.clone();
    facts.restore(checkpoint);
    assert_eq!(facts.identity(&values, restored), None);
    assert_eq!(snapshot.identity(&values, restored), Some(input));
}

#[test]
fn packed_facts_evaluate_complete_known_lanes_with_wrapping_and_signed_comparisons() {
    let mut values = ValueTable::default();
    let input = vector(&mut values);
    let known = values.literal(Type::I64, 0xffff_8000_7fff_0001_u64);
    let left = expression(
        &mut values,
        Type::V128,
        Expression::VectorReplace {
            vector: input,
            value: known,
            lane: 1,
        },
    );
    let right = values.literal(Type::V128, 0x0001_7fff_8000_ffff_ffff_ffff_ffff_ffff_u128);
    let facts = ValueFacts::default();
    for (operator, expected) in [
        (LaneBinaryOp::Add, 0x0000_ffff_ffff_0000_u64),
        (LaneBinaryOp::Sub, 0xfffe_0001_ffff_0002),
        (LaneBinaryOp::Eq, 0),
        (LaneBinaryOp::GtSigned, 0x0000_0000_ffff_ffff),
    ] {
        let packed = expression(
            &mut values,
            Type::V128,
            Expression::VectorBinary {
                operator,
                lane_type: Type::I16,
                left,
                right,
            },
        );
        let high = expression(
            &mut values,
            Type::I64,
            Expression::VectorExtract {
                input: packed,
                lane: 1,
            },
        );
        assert_eq!(
            facts.constant(&values, high).map(u128::from),
            Some(u128::from(expected))
        );
        assert_eq!(facts.constant(&values, packed), None);
    }
}
