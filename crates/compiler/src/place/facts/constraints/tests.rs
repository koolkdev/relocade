use super::*;
use crate::place::facts::{
    tests::{compare, parameters},
    Assumption,
};
use crate::{
    body::{Value, ValueDefinition},
    integer::CompareOp,
    Type,
};

#[test]
fn later_nonzero_fact_revisits_an_excluded_conjunction() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let threshold = table.literal(Type::I32, 0x0080_0000);
    let nonzero = compare(&mut table, CompareOp::Ne, 0, zero);
    let below = compare(&mut table, CompareOp::LtUnsigned, 0, threshold);
    let conjunction = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Bitwise(BitwiseOp::And),
            left: nonzero,
            right: below,
        }),
    });
    for observations in [
        [(conjunction, false), (nonzero, true)],
        [(nonzero, true), (conjunction, false)],
    ] {
        let facts = ScalarFacts::default().with_assumptions(
            &table,
            observations.map(|(condition, truth)| Assumption::Truth { condition, truth }),
        );
        assert_eq!(facts.constant(&table, below), Some(0), "{observations:?}");
    }
}

#[test]
fn remembered_boolean_constraints_match_their_truth_tables() {
    for (name, operator) in [
        ("and", BitwiseOp::And),
        ("or", BitwiseOp::Or),
        ("xor", BitwiseOp::Xor),
    ] {
        for truth in [false, true] {
            for operand in [false, true] {
                let possible: Vec<_> = [false, true]
                    .into_iter()
                    .filter(|other| {
                        (match operator {
                            BitwiseOp::And => operand & *other,
                            BitwiseOp::Or => operand | *other,
                            BitwiseOp::Xor => operand ^ *other,
                        }) == truth
                    })
                    .collect();
                if possible.is_empty() {
                    continue;
                }
                let expected = if possible.len() == 1 {
                    Some(u64::from(possible[0]))
                } else {
                    None
                };
                for reverse in [false, true] {
                    let mut table = parameters();
                    let zero = table.literal(Type::I32, 0);
                    let a = compare(&mut table, CompareOp::Ne, 0, zero);
                    let b = compare(&mut table, CompareOp::Ne, 1, zero);
                    let root = table.intern(Value {
                        ty: Type::I1,
                        definition: ValueDefinition::Expression(Expression::Binary {
                            operator: BinaryOp::Bitwise(operator),
                            left: a,
                            right: b,
                        }),
                    });
                    let mut observations = [(root, truth), (a, operand)];
                    if reverse {
                        observations.reverse();
                    }
                    let facts = ScalarFacts::default().with_assumptions(
                        &table,
                        observations
                            .map(|(condition, truth)| Assumption::Truth { condition, truth }),
                    );
                    assert_eq!(
                        facts.constant(&table, b),
                        expected,
                        "{name}, {observations:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn boolean_constraints_survive_a_join_without_importing_sibling_facts() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let a = compare(&mut table, CompareOp::Ne, 0, zero);
    let b = compare(&mut table, CompareOp::Ne, 1, zero);
    let root = table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Bitwise(BitwiseOp::And),
            left: a,
            right: b,
        }),
    });
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: root,
            truth: false,
        }],
    );
    let yes = parent.clone().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: a,
            truth: true,
        }],
    );
    let no = parent.clone().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: a,
            truth: false,
        }],
    );
    assert_eq!(yes.constant(&table, b), Some(0));
    assert_eq!(no.constant(&table, b), None);
    assert_eq!(parent.constant(&table, b), None);
    for (first, second) in [(&yes, &no), (&no, &yes)] {
        let joined = ScalarFacts::join(&table, &[], &[(first, &[]), (second, &[])]);
        assert_eq!(joined.constant(&table, a), None);
        assert_eq!(joined.constant(&table, b), None);
        let later = joined.with_assumptions(
            &table,
            [Assumption::Truth {
                condition: a,
                truth: true,
            }],
        );
        assert_eq!(later.constant(&table, b), Some(0));
    }
}

fn boolean(table: &mut ValueTable, operator: BitwiseOp, left: usize, right: usize) -> usize {
    table.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Bitwise(operator),
            left,
            right,
        }),
    })
}

#[test]
fn ranges_and_opposite_comparisons_reactivate_constraints() {
    for ordered in [false, true] {
        let mut table = parameters();
        let zero = table.literal(Type::I32, 0);
        let ten = table.literal(Type::I32, 10);
        let twenty = table.literal(Type::I32, 20);
        let premise = compare(&mut table, CompareOp::LtUnsigned, 0, twenty);
        let consequence = compare(&mut table, CompareOp::Ne, 1, zero);
        let root = boolean(&mut table, BitwiseOp::And, premise, consequence);
        let later = if ordered {
            compare(&mut table, CompareOp::LtUnsigned, 0, ten)
        } else {
            compare(&mut table, CompareOp::GeUnsigned, 0, twenty)
        };
        let facts = ScalarFacts::default().with_assumptions(
            &table,
            [Assumption::Truth {
                condition: root,
                truth: false,
            }],
        );
        assert_eq!(facts.constant(&table, consequence), None);
        let facts = facts.with_assumptions(
            &table,
            [Assumption::Truth {
                condition: later,
                truth: ordered,
            }],
        );
        assert_eq!(facts.constant(&table, consequence), Some(0));
    }
}

#[test]
fn inner_bits_reactivate_constraints_without_a_new_operand_fact() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let four = table.literal(Type::I32, 4);
    let masked = table.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Bitwise(BitwiseOp::And),
            left: 0,
            right: four,
        }),
    });
    let premise = compare(&mut table, CompareOp::Ne, masked, zero);
    let consequence = compare(&mut table, CompareOp::Ne, 1, zero);
    let root = boolean(&mut table, BitwiseOp::And, premise, consequence);
    let facts = ScalarFacts::default()
        .with_assumptions(
            &table,
            [Assumption::Truth {
                condition: root,
                truth: false,
            }],
        )
        .with_assumptions(
            &table,
            [Assumption::Bits {
                value: 0,
                mask: 4,
                bits: 4,
            }],
        );
    assert_eq!(facts.constant(&table, consequence), Some(0));
}

#[test]
fn newly_deduced_operands_propagate_through_all_retained_constraints() {
    let mut table = ValueTable::default();
    let inputs: Vec<_> = (0..=MAX_CONSTRAINTS)
        .map(|component| {
            table.push(Value {
                ty: Type::I1,
                definition: ValueDefinition::Parameter {
                    block: crate::body::BlockId(0),
                    component,
                },
            })
        })
        .collect();
    let constraints: Vec<_> = inputs
        .windows(2)
        .map(|pair| {
            let condition = boolean(&mut table, BitwiseOp::Xor, pair[0], pair[1]);
            Assumption::Truth {
                condition,
                truth: true,
            }
        })
        .collect();
    let facts = ScalarFacts::default()
        .with_assumptions(&table, constraints)
        .with_assumptions(
            &table,
            [Assumption::Truth {
                condition: inputs[MAX_CONSTRAINTS],
                truth: true,
            }],
        );
    for (index, input) in inputs.into_iter().enumerate() {
        assert_eq!(
            facts.constant(&table, input),
            Some(u64::from((MAX_CONSTRAINTS - index).is_multiple_of(2)))
        );
    }
}

#[test]
fn joins_discard_constraints_missing_on_an_incoming_path() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let a = compare(&mut table, CompareOp::Ne, 0, zero);
    let b = compare(&mut table, CompareOp::Ne, 1, zero);
    let root = boolean(&mut table, BitwiseOp::And, a, b);
    let first = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: root,
            truth: false,
        }],
    );
    let second = ScalarFacts::default();
    let joined = ScalarFacts::join(&table, &[], &[(&first, &[]), (&second, &[])]);
    let facts = joined.with_assumptions(
        &table,
        [Assumption::Truth {
            condition: a,
            truth: true,
        }],
    );
    assert_eq!(facts.constant(&table, b), None);
}

#[test]
fn joined_parameter_bits_reactivate_common_constraints() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let one = table.literal(Type::I32, 1);
    let a = compare(&mut table, CompareOp::Ne, 0, zero);
    let b = compare(&mut table, CompareOp::Ne, 1, zero);
    let root = boolean(&mut table, BitwiseOp::And, a, b);
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: root,
            truth: false,
        }],
    );
    let joined = ScalarFacts::join(&table, &[0], &[(&parent, &[one]), (&parent, &[one])]);
    assert_eq!(joined.constant(&table, b), Some(0));
}

#[test]
fn joins_recover_deductions_with_different_support_on_each_path() {
    let mut table = ValueTable::default();
    let a = table.push(Value {
        ty: Type::I1,
        definition: ValueDefinition::Parameter {
            block: crate::body::BlockId(0),
            component: 0,
        },
    });
    let x = table.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: crate::body::BlockId(0),
            component: 1,
        },
    });
    let five = table.literal(Type::I32, 5);
    let b = compare(&mut table, CompareOp::LtSigned, x, five);
    let root = boolean(&mut table, BitwiseOp::And, a, b);
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: root,
            truth: false,
        }],
    );
    let first = parent.clone().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: a,
            truth: true,
        }],
    );
    let second = parent.with_assumptions(
        &table,
        [
            Assumption::Bits {
                value: x,
                mask: u32::MAX.into(),
                bits: 10,
            },
            Assumption::Truth {
                condition: a,
                truth: true,
            },
        ],
    );
    for (first, second) in [(&first, &second), (&second, &first)] {
        let joined = ScalarFacts::join(&table, &[], &[(first, &[]), (second, &[])]);
        assert_eq!(joined.constant(&table, b), Some(0));
    }
}

#[test]
fn a_deduction_can_discover_an_inner_constraint() {
    let mut table = ValueTable::default();
    let [a, b, c] = [0, 1, 2].map(|component| {
        table.push(Value {
            ty: Type::I1,
            definition: ValueDefinition::Parameter {
                block: crate::body::BlockId(0),
                component,
            },
        })
    });
    let inner = boolean(&mut table, BitwiseOp::Xor, a, b);
    let outer = boolean(&mut table, BitwiseOp::Xor, inner, c);
    let facts = ScalarFacts::default()
        .with_assumptions(
            &table,
            [
                Assumption::Truth {
                    condition: a,
                    truth: true,
                },
                Assumption::Truth {
                    condition: outer,
                    truth: true,
                },
            ],
        )
        .with_assumptions(
            &table,
            [Assumption::Truth {
                condition: c,
                truth: false,
            }],
        );
    assert_eq!(facts.constant(&table, inner), Some(1));
    assert_eq!(facts.constant(&table, b), Some(0));
}

#[test]
fn boolean_constraints_preserve_unknown_upper_carrier_bits() {
    let mut table = parameters();
    let [a, b] = [0, 1].map(|input| {
        table.intern(Value {
            ty: Type::I1,
            definition: ValueDefinition::Expression(Expression::Unary {
                operator: crate::expression::UnaryOp::Convert,
                input,
            }),
        })
    });
    let root = boolean(&mut table, BitwiseOp::And, a, b);
    for reverse in [false, true] {
        let mut observations = [
            Assumption::Truth {
                condition: root,
                truth: false,
            },
            Assumption::Bits {
                value: 0,
                mask: 1,
                bits: 1,
            },
        ];
        if reverse {
            observations.reverse();
        }
        let facts = ScalarFacts::default().with_assumptions(&table, observations);
        assert_eq!(facts.constant(&table, b), Some(0));
        let bits = facts.bits(&table, 1);
        assert_eq!((bits.mask, bits.value), (1, 0));
        assert_eq!(facts.constant(&table, 1), None);
        assert_eq!(table.carrier_bits(b, 0), None);
    }
}

#[test]
fn comparison_notifications_include_operands_created_after_a_literal() {
    let mut table = ValueTable::default();
    let zero = table.literal(Type::I32, 0);
    let five = table.literal(Type::I32, 5);
    let [left, right] = [0, 1].map(|component| {
        table.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Parameter {
                block: crate::body::BlockId(0),
                component,
            },
        })
    });
    let premise = compare(&mut table, CompareOp::LtSigned, left, five);
    let consequence = compare(&mut table, CompareOp::Ne, right, zero);
    let root = boolean(&mut table, BitwiseOp::And, premise, consequence);
    let opposite = compare(&mut table, CompareOp::GeSigned, left, five);
    let facts = ScalarFacts::default()
        .with_assumptions(
            &table,
            [Assumption::Truth {
                condition: root,
                truth: false,
            }],
        )
        .with_assumptions(
            &table,
            [Assumption::Truth {
                condition: opposite,
                truth: false,
            }],
        );
    assert_eq!(facts.constant(&table, consequence), Some(0));
}

#[test]
fn deep_operand_histories_preserve_boolean_deductions() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let one = table.literal(Type::I32, 1);
    let mut input = 0;
    for _ in 0..80 {
        input = table.intern(Value {
            ty: Type::I32,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: BinaryOp::Integer(crate::integer::BinaryOp::Add),
                left: input,
                right: one,
            }),
        });
    }
    let a = compare(&mut table, CompareOp::Ne, input, zero);
    let b = compare(&mut table, CompareOp::Ne, 1, zero);
    let root = boolean(&mut table, BitwiseOp::Xor, a, b);
    let facts = ScalarFacts::default().with_assumptions(
        &table,
        [
            Assumption::Truth {
                condition: root,
                truth: true,
            },
            Assumption::Truth {
                condition: a,
                truth: true,
            },
        ],
    );
    assert_eq!(facts.constant(&table, root), Some(1));
    assert_eq!(facts.constant(&table, b), Some(0));
}

#[test]
fn sibling_constraints_extend_their_roots_independently() {
    let mut table = ValueTable::default();
    let [a, b, c, d] = [0, 1, 2, 3].map(|component| {
        table.push(Value {
            ty: Type::I1,
            definition: ValueDefinition::Parameter {
                block: crate::body::BlockId(0),
                component,
            },
        })
    });
    let common = boolean(&mut table, BitwiseOp::Xor, a, b);
    let first = boolean(&mut table, BitwiseOp::Xor, b, c);
    let second = boolean(&mut table, BitwiseOp::Xor, b, d);
    let parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: common,
            truth: true,
        }],
    );
    let first = parent.clone().with_assumptions(
        &table,
        [
            Assumption::Truth {
                condition: first,
                truth: true,
            },
            Assumption::Truth {
                condition: a,
                truth: true,
            },
        ],
    );
    let second = parent.clone().with_assumptions(
        &table,
        [
            Assumption::Truth {
                condition: second,
                truth: true,
            },
            Assumption::Truth {
                condition: a,
                truth: true,
            },
        ],
    );
    assert_eq!(first.constant(&table, c), Some(1));
    assert_eq!(first.constant(&table, d), None);
    assert_eq!(second.constant(&table, c), None);
    assert_eq!(second.constant(&table, d), Some(1));
    assert_eq!(parent.constant(&table, b), None);
}

#[test]
fn root_capacity_exhaustion_only_forgoes_later_deductions() {
    let mut table = ValueTable::default();
    let inputs: Vec<_> = (0..2 * (MAX_CONSTRAINTS + 1))
        .map(|component| {
            table.push(Value {
                ty: Type::I1,
                definition: ValueDefinition::Parameter {
                    block: crate::body::BlockId(0),
                    component,
                },
            })
        })
        .collect();
    let observations: Vec<_> = inputs
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Assumption::Truth {
            condition: boolean(&mut table, BitwiseOp::Xor, pair[0], pair[1]),
            truth: true,
        })
        .collect();
    let facts = ScalarFacts::default()
        .with_assumptions(&table, observations)
        .with_assumptions(
            &table,
            inputs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| Assumption::Truth {
                    condition: pair[0],
                    truth: true,
                }),
        );
    for (index, pair) in inputs.as_chunks::<2>().0.iter().enumerate() {
        assert_eq!(facts.constant(&table, pair[0]), Some(1));
        assert_eq!(
            facts.constant(&table, pair[1]),
            (index < MAX_CONSTRAINTS).then_some(0)
        );
    }
}

#[test]
fn a_fork_preserves_pending_parent_deductions() {
    let mut table = parameters();
    let zero = table.literal(Type::I32, 0);
    let a = compare(&mut table, CompareOp::Ne, 0, zero);
    let b = compare(&mut table, CompareOp::Ne, 1, zero);
    let root = boolean(&mut table, BitwiseOp::Xor, a, b);
    let mut parent = ScalarFacts::default().with_assumptions(
        &table,
        [Assumption::Truth {
            condition: root,
            truth: true,
        }],
    );
    parent.assume_bits(0, u32::MAX as u64, 1);
    let child = parent.clone().with_assumptions(&table, []);
    assert_eq!(child.constant(&table, b), Some(0));
    assert_eq!(parent.constant(&table, b), None);
    parent.propagate_constraints(&table);
    assert_eq!(parent.constant(&table, b), Some(0));
}
