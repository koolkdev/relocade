use super::*;
use crate::expression::{BinaryOp as Binary, CompareOp as Compare};
use crate::{
    body::{BlockId, Value},
    integer::CompareOp,
    Type,
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
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Compare(Compare::Integer(operator)),
            left,
            right,
        }),
    })
}

#[test]
fn child_bits_and_derived_constants_do_not_change_the_parent() {
    let table = parameters();
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Bits {
            value: 0,
            mask: 0xff,
            bits: 0x12,
        }],
    );
    assert_eq!(parent.constant(&table, 0), None);
    let child = parent.clone().with_assumptions(
        &table,
        [
            Assumption::Bits {
                value: 0,
                mask: 0xffff_ff00,
                bits: 0x123400,
            },
            Assumption::Bits {
                value: 1,
                mask: 0xffff_ffff,
                bits: 9,
            },
        ],
    );
    assert_eq!(child.constant(&table, 0), Some(0x123412));
    assert_eq!(child.constant(&table, 1), Some(9));
    assert_eq!(parent.constant(&table, 0), None);
    assert_eq!(parent.constant(&table, 1), None);
    let bits = parent.bits(&table, 0);
    assert_eq!((bits.mask, bits.value), (0xff, 0x12));
}

#[test]
fn sibling_ranges_have_independent_comparison_inference() {
    let mut table = parameters();
    let [ten, twenty, forty] = [10, 20, 40].map(|value| table.literal(Type::I32, value));
    let below_ten = compare(&mut table, CompareOp::LtUnsigned, 0, ten);
    let below_twenty = compare(&mut table, CompareOp::LtUnsigned, 0, twenty);
    let below_forty = compare(&mut table, CompareOp::LtUnsigned, 0, forty);
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: below_forty,
            truth: true,
        }],
    );
    assert_eq!(parent.constant(&table, below_twenty), None);
    let child = parent.clone().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: below_ten,
            truth: true,
        }],
    );
    assert_eq!(child.constant(&table, below_twenty), Some(1));
    let sibling = parent.clone().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: below_twenty,
            truth: false,
        }],
    );
    assert_eq!(sibling.constant(&table, below_ten), Some(0));
    assert_eq!(sibling.constant(&table, below_forty), Some(1));
    assert_eq!(parent.constant(&table, below_twenty), None);
}

#[test]
fn sibling_contexts_keep_equivalent_and_opposite_predicates_separate() {
    let mut table = parameters();
    let unequal = compare(&mut table, CompareOp::Ne, 0, 1);
    let reversed = compare(&mut table, CompareOp::Eq, 1, 0);
    let equal = compare(&mut table, CompareOp::Eq, 0, 1);
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Bits {
            value: 0,
            mask: 1,
            bits: 0,
        }],
    );
    for (truth, expected) in [(true, 1), (false, 0)] {
        let child = parent.clone().with_assumptions(
            &table,
            [Assumption::Truth {
                condition: equal,
                truth,
            }],
        );
        assert_eq!(child.constant(&table, unequal), Some(1 - expected));
        assert_eq!(child.constant(&table, reversed), Some(expected));
    }
    assert_eq!(parent.constant(&table, unequal), None);
    assert_eq!(parent.constant(&table, reversed), None);
}
