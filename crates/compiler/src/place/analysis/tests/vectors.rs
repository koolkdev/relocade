//! Lane observations preserve their context and scalar carrier boundaries.
use super::*;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};
use crate::{bitwise::BitwiseOp, expression::LaneBinaryOp};

fn expression(values: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    values.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

#[test]
fn packed_facts_require_complete_input_lanes_at_each_width() {
    for ty in [Type::I8, Type::I16, Type::I32, Type::I64] {
        for lane in [0, 128 / ty.bits() - 1] {
            let mut values = ValueTable::default();
            let unknown = values.push(Value {
                ty: Type::V128,
                definition: ValueDefinition::Parameter {
                    block: BlockId(0),
                    component: 0,
                },
            });
            let ones = values.literal(ty, ty.mask());
            let one = values.literal(ty, 1);
            let zero = values.literal(ty, 0);
            let mut replace = |value| {
                expression(
                    &mut values,
                    Type::V128,
                    Expression::Binary {
                        operator: Binary::ReplaceLane {
                            lane_type: ty,
                            lane,
                        },
                        left: unknown,
                        right: value,
                    },
                )
            };
            let left = replace(ones);
            let right = replace(one);
            let zero_lane = replace(zero);
            let input_lane = expression(
                &mut values,
                ty,
                Expression::Unary {
                    operator: Unary::ExtractLane { lane },
                    input: unknown,
                },
            );
            let facts = ValueAnalysis::default().with_assumptions(
                &values,
                [Assumption::Bits {
                    value: input_lane,
                    mask: ty.mask() >> 1,
                    bits: 0,
                }],
            );
            // All but one input bit is known. That still leaves this packed
            // lane unknown unless the replacement supplies the complete lane.
            for (operator, expected) in [
                (LaneBinaryOp::Add, 0),
                (LaneBinaryOp::Sub, ty.mask() - 1),
                (LaneBinaryOp::Eq, 0),
                (LaneBinaryOp::GtSigned, 0),
            ] {
                let result = expression(
                    &mut values,
                    Type::V128,
                    Expression::Binary {
                        operator: Binary::Vector {
                            operator,
                            lane_type: ty,
                        },
                        left,
                        right,
                    },
                );
                let shift = lane * ty.bits();
                assert_eq!(
                    facts.vector_bits(&values, result),
                    Bits {
                        mask: u128::from(ty.mask()) << shift,
                        value: u128::from(expected) << shift,
                    }
                );
                assert_eq!(facts.vector_constant(&values, result), None);
                let partial = expression(
                    &mut values,
                    Type::V128,
                    Expression::Binary {
                        operator: Binary::Vector {
                            operator,
                            lane_type: ty,
                        },
                        left: zero_lane,
                        right: unknown,
                    },
                );
                assert_eq!(facts.vector_bits(&values, partial), Bits::default());
            }
            let greater = expression(
                &mut values,
                Type::V128,
                Expression::Binary {
                    operator: Binary::Vector {
                        operator: LaneBinaryOp::GtSigned,
                        lane_type: ty,
                    },
                    left: right,
                    right: left,
                },
            );
            let mask = u128::from(ty.mask()) << (lane * ty.bits());
            assert_eq!(
                facts.vector_bits(&values, greater),
                Bits { mask, value: mask }
            );
        }
    }
}

#[test]
fn scalar_observation_proves_only_its_vector_lane_and_leaves_with_its_scope() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let low = expression(
        &mut values,
        Type::I64,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 0 },
            input,
        },
    );
    let high = expression(
        &mut values,
        Type::I64,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 1 },
            input,
        },
    );
    let scalar_zero = values.literal(Type::I64, 0);
    let condition = compare(&mut values, CompareOp::Eq, high, scalar_zero);
    let zero = values.literal(Type::V128, 0_u128);
    let restored = expression(
        &mut values,
        Type::V128,
        Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I64,
                lane: 0,
            },
            left: zero,
            right: low,
        },
    );
    let mut facts = ValueAnalysis::default();
    let round_trip = expression(
        &mut values,
        Type::V128,
        Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I64,
                lane: 0,
            },
            left: input,
            right: low,
        },
    );
    assert_eq!(facts.identity(&values, round_trip), Some(input));
    assert!(facts.derived.borrow().vectors.is_none());
    assert_eq!(facts.identity(&values, restored), None);
    let scope = facts.enter(
        &values,
        [Assumption::Truth {
            condition,
            truth: true,
        }],
    );
    assert_eq!(
        facts.vector_bits(&values, input),
        Bits {
            mask: u128::from(u64::MAX) << 64,
            value: 0
        }
    );
    assert_eq!(facts.identity(&values, restored), Some(input));
    assert_eq!(facts.vector_constant(&values, input), None);
    let snapshot = facts.clone();
    facts.leave(scope);
    assert_eq!(facts.identity(&values, restored), None);
    assert_eq!(snapshot.identity(&values, restored), Some(input));
}

#[test]
fn replacement_observes_logical_lane_bits_and_extraction_crosses_domains() {
    let mut values = ValueTable::default();
    values.push(Value {
        ty: Type::I64,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let narrow = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input: 0,
        },
    );
    let zero = values.literal(Type::V128, 0_u128);
    let vector = expression(
        &mut values,
        Type::V128,
        Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I32,
                lane: 3,
            },
            left: zero,
            right: narrow,
        },
    );
    let lane = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 3 },
            input: vector,
        },
    );
    let low = expression(
        &mut values,
        Type::I64,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 0 },
            input: vector,
        },
    );
    let facts = ValueAnalysis::default().with_assumptions(
        &values,
        [Assumption::Bits {
            value: 0,
            mask: u64::MAX,
            bits: 0x12345678_90abcdef,
        }],
    );
    assert_eq!(
        facts.vector_constant(&values, vector),
        Some(0x90abcdef_u128 << 96)
    );
    assert_eq!(facts.constant(&values, lane), Some(0x90abcdef));
    assert_eq!(facts.constant(&values, low), Some(0));
}

#[test]
fn scalar_inference_uses_compact_storage_without_allocating_vector_tables() {
    let mut table = parameters();
    let mask = table.literal(Type::I32, 0xff);
    let low = expression(
        &mut table,
        Type::I32,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: 0,
            right: mask,
        },
    );
    let facts = ValueAnalysis::default();
    assert_eq!(
        facts.bits(&table, low),
        Bits {
            mask: 0xffff_ff00,
            value: 0
        }
    );
    assert_eq!(std::mem::size_of::<Bits>(), 16);
    assert!(facts.context.vector_known.is_none());
    assert!(facts.derived.borrow().vectors.is_none());
}

#[test]
fn nested_lane_observations_restore_parent_facts_and_cross_domain_answers() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let low = expression(
        &mut values,
        Type::I64,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 0 },
            input,
        },
    );
    let high = expression(
        &mut values,
        Type::I64,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 1 },
            input,
        },
    );
    let seven = values.literal(Type::I64, 7);
    let high_seven = compare(&mut values, CompareOp::Eq, high, seven);
    let mut facts = ValueAnalysis::default().with_assumptions(
        &values,
        [Assumption::Truth {
            condition: high_seven,
            truth: true,
        }],
    );
    assert_eq!(facts.constant(&values, high), Some(7));
    assert_eq!(facts.constant(&values, low), None);
    assert_eq!(facts.vector_constant(&values, input), None);
    let mut snapshots = Vec::new();
    for number in [9_u64, 11] {
        let literal = values.literal(Type::I64, number);
        let condition = compare(&mut values, CompareOp::Eq, low, literal);
        let scope = facts.enter(
            &values,
            [Assumption::Truth {
                condition,
                truth: true,
            }],
        );
        assert_eq!(facts.constant(&values, low), Some(number));
        assert_eq!(
            facts.vector_constant(&values, input),
            Some((7_u128 << 64) | u128::from(number))
        );
        snapshots.push(facts.clone());
        facts.leave(scope);
        assert_eq!(facts.constant(&values, low), None);
        assert_eq!(facts.constant(&values, high), Some(7));
        assert_eq!(facts.vector_constant(&values, input), None);
    }
    assert_eq!(snapshots[0].constant(&values, low), Some(9));
    assert_eq!(snapshots[1].constant(&values, low), Some(11));
    let joined = ValueAnalysis::join(&values, &[], &[(&snapshots[0], &[]), (&snapshots[1], &[])]);
    assert_eq!(
        joined.vector_bits(&values, input),
        Bits {
            mask: u128::MAX - 2,
            value: (7_u128 << 64) | 9
        }
    );
}

#[test]
fn an_observed_replaced_lane_does_not_constrain_the_scalars_other_bits() {
    let mut values = ValueTable::default();
    let input = values.push(Value {
        ty: Type::I64,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let narrow = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::Convert,
            input,
        },
    );
    let zero = values.literal(Type::V128, 0_u128);
    let vector = expression(
        &mut values,
        Type::V128,
        Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I32,
                lane: 3,
            },
            left: zero,
            right: narrow,
        },
    );
    let lane = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 3 },
            input: vector,
        },
    );
    let literal = values.literal(Type::I32, 0x90abcdef_u64);
    let condition = compare(&mut values, CompareOp::Eq, lane, literal);
    let facts = ValueAnalysis::default().with_assumptions(
        &values,
        [Assumption::Truth {
            condition,
            truth: true,
        }],
    );
    assert_eq!(facts.constant(&values, narrow), Some(0x90abcdef));
    assert_eq!(facts.constant(&values, input), None);
    assert_eq!(
        facts.bits(&values, input),
        Bits {
            mask: u64::from(u32::MAX),
            value: 0x90abcdef
        }
    );
}

#[test]
fn width_changing_products_do_not_reuse_the_input_lane_mask() {
    for (operator, replaced, output_shift) in [
        (LaneBinaryOp::WideningMulLowSigned, 4, 64),
        (LaneBinaryOp::WideningMulHighSigned, 0, 0),
        (LaneBinaryOp::DotI16Signed, 0, 0),
    ] {
        let mut values = ValueTable::default();
        let unknown = values.push(Value {
            ty: Type::V128,
            definition: ValueDefinition::Parameter {
                block: BlockId(0),
                component: 0,
            },
        });
        let one = values.literal(Type::I16, 1);
        let left = expression(
            &mut values,
            Type::V128,
            Expression::Binary {
                operator: Binary::ReplaceLane {
                    lane_type: Type::I16,
                    lane: replaced,
                },
                left: unknown,
                right: one,
            },
        );
        let right = values.literal(Type::V128, 0x0001_0001_0001_0001_0001_0001_0001_0001_u128);
        let result = expression(
            &mut values,
            Type::V128,
            Expression::Binary {
                operator: Binary::Vector {
                    operator,
                    lane_type: Type::I16,
                },
                left,
                right,
            },
        );
        // This output still depends on an arbitrary input word. Every low
        // result bit can vary even though another input word is completely known.
        let facts = ValueAnalysis::default().vector_bits(&values, result);
        assert_eq!(facts.mask & (0xffff_u128 << output_shift), 0);
    }
}

#[test]
fn replacement_width_survives_a_wider_scalar_alias() {
    let mut values = ValueTable::default();
    let vector = values.literal(Type::V128, 0_u128);
    let scalar = values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let replaced = expression(
        &mut values,
        Type::V128,
        Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I8,
                lane: 3,
            },
            left: vector,
            right: scalar,
        },
    );
    let lane = expression(
        &mut values,
        Type::I8,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 3 },
            input: replaced,
        },
    );
    let literal = values.literal(Type::I8, 0xab);
    let condition = compare(&mut values, CompareOp::Eq, lane, literal);
    let facts = ValueAnalysis::default().with_assumptions(
        &values,
        [Assumption::Truth {
            condition,
            truth: true,
        }],
    );
    assert_eq!(
        facts.bits(&values, scalar),
        Bits {
            mask: 0xff,
            value: 0xab
        }
    );
    assert_eq!(
        facts.vector_constant(&values, replaced),
        Some(0xab_u128 << 24)
    );

    let word = expression(
        &mut values,
        Type::I32,
        Expression::Unary {
            operator: Unary::ExtractLane { lane: 3 },
            input: replaced,
        },
    );
    let different_width = expression(
        &mut values,
        Type::V128,
        Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I8,
                lane: 3,
            },
            left: replaced,
            right: word,
        },
    );
    // Equal lane indices with different widths name different bit intervals.
    assert_eq!(facts.identity(&values, different_width), None);
}
