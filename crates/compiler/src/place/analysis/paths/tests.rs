use super::*;
use crate::{
    body::{BlockId, Value, ValueDefinition},
    integer::CompareOp,
    place::analysis::Assumption,
    Expression, Type,
};

fn parameter() -> ValueTable {
    let mut table = ValueTable::default();
    table.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    table
}

fn low_bit(bits: u64) -> Assumption {
    Assumption::Bits {
        value: 0,
        mask: 1,
        bits,
    }
}

fn snapshot(table: &ValueTable, block: usize, bits: u64) -> PathSnapshot {
    ValueAnalysis::default()
        .fork(table, [low_bit(bits)])
        .snapshot(block)
}

#[test]
fn exclusion_proofs_and_misses_follow_nested_and_sibling_contexts() {
    let table = parameter();
    let zero = snapshot(&table, 1, 0);
    let one = snapshot(&table, 2, 1);
    let high_zero = ValueAnalysis::default()
        .fork(
            &table,
            [Assumption::Bits {
                value: 0,
                mask: 2,
                bits: 0,
            }],
        )
        .snapshot(3);
    let mut active = ValueAnalysis::default();
    assert!(!active.excludes(&table, &zero));
    assert!(!active.excludes(&table, &one));

    for bit in [1, 0] {
        let scope = active.enter(&table, None, [low_bit(bit)]);
        assert_eq!(active.excludes(&table, &zero), bit == 1);
        assert_eq!(active.excludes(&table, &one), bit == 0);
        assert!(!active.excludes(&table, &high_zero));
        let nested = active.enter(
            &table,
            None,
            [Assumption::Bits {
                value: 0,
                mask: 2,
                bits: 2,
            }],
        );
        assert_eq!(active.excludes(&table, &zero), bit == 1);
        assert!(active.excludes(&table, &high_zero));
        active.leave(nested);
        assert!(!active.excludes(&table, &high_zero));
        active.leave(scope);
        assert!(!active.excludes(&table, &zero));
        assert!(!active.excludes(&table, &one));
    }
}

#[test]
fn a_replaced_context_and_its_snapshot_keep_distinct_observations() {
    let table = parameter();
    let zero = snapshot(&table, 1, 0);
    let mut active = ValueAnalysis::default().fork(&table, [low_bit(1)]);
    let original = active.snapshot(2);
    assert!(active.excludes(&table, &zero));
    let replacement = ValueAnalysis::default().fork(&table, [low_bit(0)]);
    let scope = active.enter(&table, Some(replacement), []);
    assert!(!active.excludes(&table, &zero));
    assert!(active.excludes(&table, &original));
    active.leave(scope);
    assert!(active.excludes(&table, &zero));
    assert!(!active.excludes(&table, &original));
}

#[test]
fn forks_and_joins_reconsider_relations_under_their_own_observations() {
    let table = parameter();
    let zero = snapshot(&table, 1, 0);
    let base = ValueAnalysis::default();
    assert!(!base.excludes(&table, &zero));
    let first = base.fork(&table, [low_bit(1)]);
    assert!(first.excludes(&table, &zero));
    let second = base.fork(&table, [low_bit(0)]);
    assert!(!second.excludes(&table, &zero));
    let joined = ValueAnalysis::join(&table, &[], &[(&first, &[]), (&second, &[])]);
    assert!(!joined.excludes(&table, &zero));
    assert!(!base.excludes(&table, &zero));
}

#[test]
fn exclusions_can_be_derived_from_distinct_range_predicates() {
    let mut table = parameter();
    let ten = table.literal(Type::I32, 10);
    let twenty = table.literal(Type::I32, 20);
    let mut compare = |operator, right| {
        table.intern(Value {
            ty: Type::I1,
            definition: ValueDefinition::Expression(Expression::Compare {
                operator,
                left: 0,
                right,
            }),
        })
    };
    let below_ten = compare(CompareOp::LtUnsigned, ten);
    let at_least_twenty = compare(CompareOp::GeUnsigned, twenty);
    let source = ValueAnalysis::default()
        .fork(
            &table,
            [Assumption::Truth {
                condition: below_ten,
                truth: true,
            }],
        )
        .snapshot(1);
    let mut active = ValueAnalysis::default();
    assert!(!active.excludes(&table, &source));
    let scope = active.enter(
        &table,
        None,
        [Assumption::Truth {
            condition: at_least_twenty,
            truth: true,
        }],
    );
    assert!(active.excludes(&table, &source));
    assert!(active.excludes(&table, &source));
    active.leave(scope);
    assert!(!active.excludes(&table, &source));
}
