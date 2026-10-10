use super::*;
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
        definition: ValueDefinition::Expression(Expression::Compare {
            operator,
            left,
            right,
        }),
    })
}

#[test]
fn structural_inference_leaves_vector_values_opaque() {
    let mut table = ValueTable::default();
    let input = table.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let zero = table.literal(Type::V128, 0_u128);
    let high = table.literal(Type::V128, 1_u128 << 96);
    let facts = ScalarFacts::default();
    for value in [input, zero, high] {
        assert_eq!(facts.inferred_constant(&table, value), None);
    }
}

#[test]
fn restoring_partial_bits_discards_child_constants_but_preserves_snapshots() {
    let table = parameters();
    let mut facts = ScalarFacts::default();
    facts.assume_bits(0, 0xff, 0x12);
    let scope = facts.checkpoint();
    facts.assume_bits(0, 0xffff_ff00, 0x123400);
    facts.assume_bits(1, 0xffff_ffff, 9);
    assert_eq!(facts.constant(&table, 0), Some(0x123412));
    assert_eq!(facts.constant(&table, 1), Some(9));
    assert_eq!(facts.bits(&table, 0).value, 0x123412);
    assert_eq!(facts.bits(&table, 1).value, 9);
    let snapshot = facts.clone();
    facts.restore(scope);
    assert_eq!(facts.constant(&table, 0), None);
    assert_eq!(facts.constant(&table, 1), None);
    let bits = facts.bits(&table, 0);
    assert_eq!((bits.mask, bits.value), (0xff, 0x12));
    assert_eq!(facts.bits(&table, 1).mask, 0);
    assert_eq!(snapshot.constant(&table, 0), Some(0x123412));
    assert_eq!(snapshot.constant(&table, 1), Some(9));
}

#[test]
fn restoring_ranges_discards_child_comparison_inference() {
    let mut table = parameters();
    let [ten, twenty, forty] = [10, 20, 40].map(|value| table.literal(Type::I32, value));
    let below_ten = compare(&mut table, CompareOp::LtUnsigned, 0, ten);
    let below_twenty = compare(&mut table, CompareOp::LtUnsigned, 0, twenty);
    let below_forty = compare(&mut table, CompareOp::LtUnsigned, 0, forty);
    let mut facts = ScalarFacts::default();
    facts.assume(&table, below_forty, true);
    let scope = facts.checkpoint();
    facts.assume(&table, below_ten, true);
    assert_eq!(facts.constant(&table, below_twenty), Some(1));
    facts.restore(scope);
    assert_eq!(facts.constant(&table, below_forty), Some(1));
    assert_eq!(facts.constant(&table, below_twenty), None);
    let sibling = facts.checkpoint();
    facts.assume(&table, below_twenty, false);
    assert_eq!(facts.constant(&table, below_ten), Some(0));
    assert_eq!(facts.constant(&table, below_forty), Some(1));
    facts.restore(sibling);
    assert_eq!(facts.constant(&table, below_twenty), None);
}

#[test]
fn restoring_comparisons_forgets_equivalent_and_opposite_predicates() {
    let mut table = parameters();
    let unequal = compare(&mut table, CompareOp::Ne, 0, 1);
    let reversed = compare(&mut table, CompareOp::Eq, 1, 0);
    let equal = compare(&mut table, CompareOp::Eq, 0, 1);
    let mut facts = ScalarFacts::default();
    // Keep an inherited fact so subsequent queries still consult inference.
    facts.assume_bits(0, 1, 0);
    let scope = facts.checkpoint();
    facts.assume(&table, equal, true);
    assert_eq!(facts.constant(&table, unequal), Some(0));
    assert_eq!(facts.constant(&table, reversed), Some(1));
    facts.restore(scope);
    assert_eq!(facts.constant(&table, unequal), None);
    assert_eq!(facts.constant(&table, reversed), None);
    let sibling = facts.checkpoint();
    facts.assume(&table, equal, false);
    assert_eq!(facts.constant(&table, unequal), Some(1));
    assert_eq!(facts.constant(&table, reversed), Some(0));
    facts.restore(sibling);
}
