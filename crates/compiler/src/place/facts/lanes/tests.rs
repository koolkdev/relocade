use super::*;
use crate::body::{BlockId, Value};
use crate::place::facts::Assumption;

fn parameter(table: &mut ValueTable, ty: Type, component: usize) -> usize {
    table.push(Value {
        ty,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component,
        },
    })
}

fn extract(table: &mut ValueTable, ty: Type, lane: u8, input: usize) -> usize {
    table.intern(Value {
        ty,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::ExtractLane { lane },
            input,
        }),
    })
}

#[test]
fn overlapping_observations_invalidate_earlier_queries_and_stay_path_local() {
    let mut table = ValueTable::default();
    let vector = parameter(&mut table, Type::V128, 0);
    let earlier = extract(&mut table, Type::I16, 1, vector);
    let word = extract(&mut table, Type::I32, 0, vector);
    let high = extract(&mut table, Type::I64, 1, vector);
    let high_byte = extract(&mut table, Type::I8, 15, vector);
    let parent = ScalarFacts::default();
    assert_eq!(parent.bits(&table, earlier).mask, 0);
    let mut child = parent.clone();
    assert_eq!(child.bits(&table, earlier).mask, 0);
    child = child.with_assumptions(
        &table,
        [
            Assumption::Bits {
                value: word,
                mask: u32::MAX.into(),
                bits: 0x12345678,
            },
            Assumption::Bits {
                value: high,
                mask: u64::MAX,
                bits: 0xdead_beef_1234_5678,
            },
        ],
    );
    assert_eq!(child.constant(&table, earlier), Some(0x1234));
    assert_eq!(child.constant(&table, high_byte), Some(0xde));
    assert_eq!(parent.constant(&table, earlier), None);
    assert!(
        child.known.is_empty(),
        "lane observations have one stored owner"
    );
    let sibling = parent.with_assumptions(
        &table,
        [Assumption::Bits {
            value: earlier,
            mask: 0xffff,
            bits: 0x9876,
        }],
    );
    assert!(child.conflicts_with(&table, &sibling));
    assert!(sibling.conflicts_with(&table, &child));
    let joined = ScalarFacts::join(&table, &[], &[(&child, &[]), (&sibling, &[])]);
    assert_eq!(joined.constant(&table, earlier), None);
    let unknown = ScalarFacts::default();
    let joined = ScalarFacts::join(&table, &[], &[(&child, &[]), (&unknown, &[])]);
    assert!(joined.lanes.is_none());
}

#[test]
fn partial_bits_and_scalar_views_do_not_assert_adjacent_vector_bits() {
    let mut table = ValueTable::default();
    let vector = parameter(&mut table, Type::V128, 0);
    let word = extract(&mut table, Type::I32, 0, vector);
    let view = table.intern(Value {
        ty: Type::I16,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::Convert,
            input: word,
        }),
    });
    let low = extract(&mut table, Type::I8, 0, vector);
    let next = extract(&mut table, Type::I8, 1, vector);
    let upper = extract(&mut table, Type::I16, 1, vector);
    let facts = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Bits {
            value: view,
            mask: 0xff0f,
            bits: 0xab05,
        }],
    );
    assert_eq!(facts.constant(&table, next), Some(0xab));
    assert_eq!(facts.constant(&table, low), None);
    let bits = facts.bits(&table, low);
    assert_eq!((bits.mask, bits.value), (0x0f, 5));
    assert_eq!(facts.bits(&table, upper).mask, 0);
}

#[test]
fn raw_bits_of_a_vector_view_share_only_the_observed_low_word() {
    let mut table = ValueTable::default();
    let vector = parameter(&mut table, Type::V128, 0);
    let view = table.intern(Value {
        ty: Type::V64,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::Convert,
            input: vector,
        }),
    });
    let raw = table.intern(Value {
        ty: Type::I64,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::Reinterpret,
            input: view,
        }),
    });
    let low = extract(&mut table, Type::I32, 0, vector);
    let next = extract(&mut table, Type::I32, 1, vector);
    let high = extract(&mut table, Type::I64, 1, vector);
    let facts = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Bits {
            value: raw,
            mask: u64::MAX,
            bits: 0x11223344_aabbccdd,
        }],
    );
    assert_eq!(facts.constant(&table, low), Some(0xaabbccdd));
    assert_eq!(facts.constant(&table, next), Some(0x11223344));
    assert_eq!(facts.constant(&table, high), None);
    let conflict = facts.with_assumptions(
        &table,
        [Assumption::Bits {
            value: next,
            mask: u32::MAX.into(),
            bits: 0xdeadbeef,
        }],
    );
    assert_eq!(conflict.constant(&table, raw), Some(0x11223344_aabbccdd));
    let reverse = ScalarFacts::default().with_assumptions(
        &table,
        [
            Assumption::Bits {
                value: low,
                mask: u32::MAX.into(),
                bits: 0xaabbccdd,
            },
            Assumption::Bits {
                value: next,
                mask: u32::MAX.into(),
                bits: 0x11223344,
            },
        ],
    );
    assert_eq!(reverse.constant(&table, raw), Some(0x11223344_aabbccdd));
    assert_eq!(reverse.constant(&table, high), None);
}

#[test]
fn vector_parameters_keep_common_words_at_their_logical_width() {
    for ty in [Type::V64, Type::V128] {
        let mut table = ValueTable::default();
        let a = parameter(&mut table, Type::V128, 0);
        let b = parameter(&mut table, Type::V128, 1);
        let parameter = parameter(&mut table, ty, 2);
        let a_low = extract(&mut table, Type::I32, 0, a);
        let b_low = extract(&mut table, Type::I32, 0, b);
        let a_high = extract(&mut table, Type::I64, 1, a);
        let b_high = extract(&mut table, Type::I64, 1, b);
        let low = extract(&mut table, Type::I16, 1, parameter);
        // Raw carrier query witnesses that V64's upper word is not inferred.
        let high = extract(&mut table, Type::I64, 1, parameter);
        let first = ScalarFacts::default().with_assumptions(
            &table,
            [
                Assumption::Bits {
                    value: a_low,
                    mask: u32::MAX.into(),
                    bits: 0x12345678,
                },
                Assumption::Bits {
                    value: a_high,
                    mask: u64::MAX,
                    bits: 9,
                },
            ],
        );
        let second = ScalarFacts::default().with_assumptions(
            &table,
            [
                Assumption::Bits {
                    value: b_low,
                    mask: u32::MAX.into(),
                    bits: 0x12345679,
                },
                Assumption::Bits {
                    value: b_high,
                    mask: u64::MAX,
                    bits: 9,
                },
            ],
        );
        let arguments = [a, b].map(|input| {
            table.intern(Value {
                ty,
                definition: ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::Convert,
                    input,
                }),
            })
        });
        let joined = ScalarFacts::join(
            &table,
            &[parameter],
            &[(&first, &arguments[..1]), (&second, &arguments[1..])],
        );
        assert_eq!(joined.constant(&table, low), Some(0x1234));
        assert_eq!(
            joined.constant(&table, high),
            if ty == Type::V128 { Some(9) } else { None }
        );
        assert_eq!(joined.constant(&table, parameter), None);
    }
}
