use super::super::Assumption;
use super::*;
use crate::{
    body::{BlockId, Value, ValueDefinition, ValueTable},
    integer::CompareOp,
    Expression, Type,
};

fn parameters() -> ValueTable {
    let mut table = ValueTable::default();
    for component in 0..2 {
        table.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component,
            },
        });
    }
    table
}

fn compare(table: &mut ValueTable, operator: CompareOp, left: usize, right: usize) -> usize {
    table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Compare {
            operator,
            left,
            right,
        }),
    })
}

#[test]
fn common_bits_discard_disagreements_missing_facts_and_cached_constants() {
    let table = parameters();
    let mut first = ValueAnalysis::default().fork(
        &table,
        [
            Assumption::Bits {
                value: 0,
                mask: 0xffff_ffff,
                bits: 5,
            },
            Assumption::Bits {
                value: 1,
                mask: 0xffff_ffff,
                bits: 9,
            },
        ],
    );
    assert_eq!(first.constant(&table, 0), Some(5));
    assert_eq!(first.constant(&table, 1), Some(9));
    assert_eq!(first.bits(&table, 0).value, 5);
    assert_eq!(first.bits(&table, 1).value, 9);
    let second = ValueAnalysis::default().fork(
        &table,
        [Assumption::Bits {
            value: 0,
            mask: 0xffff_ffff,
            bits: 7,
        }],
    );
    first = ValueAnalysis::join(&table, &[], &[(&first, &[]), (&second, &[])]);
    let bits = first.bits(&table, 0);
    assert_eq!((bits.mask, bits.value), (0xffff_fffd, 5));
    assert_eq!(first.constant(&table, 0), None);
    assert_eq!(first.bits(&table, 1).mask, 0);
}

#[test]
fn common_ranges_cover_both_paths_even_without_a_shared_predicate_identity() {
    let mut table = parameters();
    let ten = table.literal(Type::I32, 10);
    let twenty = table.literal(Type::I32, 20);
    let below_ten = compare(&mut table, CompareOp::LtUnsigned, 0, ten);
    let below_twenty = compare(&mut table, CompareOp::LtUnsigned, 0, twenty);
    let mut first = ValueAnalysis::default().fork(
        &table,
        [Assumption::Truth {
            condition: below_ten,
            truth: true,
        }],
    );
    let second = ValueAnalysis::default().fork(
        &table,
        [Assumption::Truth {
            condition: below_twenty,
            truth: true,
        }],
    );
    first = ValueAnalysis::join(&table, &[], &[(&first, &[]), (&second, &[])]);
    assert!(first.context.known.is_empty());
    assert_eq!(first.constant(&table, below_twenty), Some(1));
    assert_eq!(first.constant(&table, below_ten), None);
    first = ValueAnalysis::join(
        &table,
        &[],
        &[(&first, &[]), (&ValueAnalysis::default(), &[])],
    );
    assert_eq!(first.constant(&table, below_twenty), None);
}

#[test]
fn common_comparison_outcomes_survive_distinct_predicates_but_not_disagreement() {
    let mut table = parameters();
    let equal = compare(&mut table, CompareOp::Eq, 0, 1);
    let reversed = compare(&mut table, CompareOp::Eq, 1, 0);
    let mut first = ValueAnalysis::default().fork(
        &table,
        [Assumption::Truth {
            condition: equal,
            truth: true,
        }],
    );
    let second = ValueAnalysis::default().fork(
        &table,
        [Assumption::Truth {
            condition: reversed,
            truth: true,
        }],
    );
    first = ValueAnalysis::join(&table, &[], &[(&first, &[]), (&second, &[])]);
    assert!(first.context.known.is_empty());
    assert_eq!(first.constant(&table, equal), Some(1));
    let unequal = ValueAnalysis::default().fork(
        &table,
        [Assumption::Truth {
            condition: reversed,
            truth: false,
        }],
    );
    first = ValueAnalysis::join(&table, &[], &[(&first, &[]), (&unequal, &[])]);
    assert_eq!(first.constant(&table, equal), None);
}
