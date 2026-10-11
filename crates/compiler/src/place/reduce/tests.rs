use super::*;
use crate::bitwise::BitwiseOp;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};

fn branch(reducer: &Reducer, values: &ValueTable, condition: usize, truth: bool) -> Reducer {
    Reducer::from_analysis(
        reducer
            .analysis()
            .clone()
            .with_assumptions(values, [Assumption::Truth { condition, truth }]),
    )
}

fn conditional_value() -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    let condition = graph.values.push_with_bounds(
        Value {
            ty: crate::Type::I1,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component: 0,
            },
        },
        crate::body::BitBounds {
            unsigned: 1,
            signed: 2,
        },
    );
    graph.blocks[0].parameters.push(condition);
    let when_true = graph.values.literal(crate::Type::I32, 7);
    let when_false = graph.values.literal(crate::Type::I32, 11);
    let choice = graph.values.intern(Value {
        ty: crate::Type::I32,
        definition: ValueDefinition::Expression(Expression::Select {
            condition,
            when_true,
            when_false,
        }),
    });
    graph.blocks[0].exit = Exit::Return(vec![choice]);
    graph
}

fn returned(graph: &FunctionGraph) -> usize {
    let Exit::Return(values) = &graph.blocks[0].exit else {
        unreachable!()
    };
    values[0]
}

#[test]
fn sibling_contexts_have_independent_facts_and_residuals() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut live = Reducer::default();
    assert_eq!(live.reduce(&mut graph.values, choice), choice);
    for (truth, expected) in [(false, 11), (true, 7)] {
        let mut sibling = branch(&live, &graph.values, condition, truth);
        let result = sibling.reduce(&mut graph.values, choice);
        assert!(matches!(graph.values[result].scalar_literal(), Some(bits) if bits == expected));
    }
    assert_eq!(live.reduce(&mut graph.values, choice), choice);
    assert!(graph.blocks[0].items.is_empty());
}

#[test]
fn refolding_finishes_reduction_of_a_new_recipe_before_returning() {
    use crate::Type;

    let mut graph = conditional_value();
    let input = graph.values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 1,
        },
    });
    let condition = graph.blocks[0].parameters[0];
    let mut masked = |mask| {
        let mask = graph.values.literal(Type::I32, mask);
        graph.values.intern(Value {
            ty: Type::I32,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::And),
                left: input,
                right: mask,
            }),
        })
    };
    let low = masked(0xff);
    let high = masked(0xff0000);
    let choice = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Select {
            condition,
            when_true: low,
            when_false: input,
        }),
    });
    let root = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::Or),
            left: high,
            right: choice,
        }),
    });
    let facts = ValueAnalysis::default().with_assumptions(
        &graph.values,
        [
            Assumption::Truth {
                condition,
                truth: true,
            },
            Assumption::Bits {
                value: input,
                mask: 0xff00ff00,
                bits: 0,
            },
        ],
    );
    let mut reducer = Reducer::from_analysis(facts);
    // The chosen arm first refolds to x & 0x00ff00ff. Only then can path
    // knowledge absorb the combined mask and return x's entire carrier.
    assert_eq!(reducer.reduce(&mut graph.values, root), input);
    assert_eq!(reducer.reduce(&mut graph.values, root), input);
}

#[test]
fn semantic_equivalences_belong_to_their_fixed_context() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut parent = Reducer::default();
    let mut yes = branch(&parent, &graph.values, condition, true);
    let mut no = branch(&parent, &graph.values, condition, false);
    let seven = yes.reduce(&mut graph.values, choice);
    let eleven = no.reduce(&mut graph.values, choice);
    assert_eq!(graph.values[seven].scalar_literal(), Some(7));
    assert_eq!(graph.values[eleven].scalar_literal(), Some(11));
    assert!(yes.recipes(seven).any(|recipe| recipe == choice));
    assert!(!no.recipes(seven).any(|recipe| recipe == choice));
    assert!(no.recipes(eleven).any(|recipe| recipe == choice));
    assert_eq!(parent.reduce(&mut graph.values, choice), choice);
    assert_eq!(yes.reduce(&mut graph.values, choice), seven);
    assert_eq!(no.reduce(&mut graph.values, choice), eleven);
}

#[test]
fn equal_logical_choices_keep_distinct_physical_carriers() {
    let mut graph = conditional_value();
    let condition = graph.blocks[0].parameters[0];
    let when_true = graph.values.carrier_literal(crate::Type::I8, 0x100);
    let when_false = graph.values.carrier_literal(crate::Type::I8, 0x200);
    let choice = graph.values.intern(Value {
        ty: crate::Type::I8,
        definition: ValueDefinition::Expression(Expression::Select {
            condition,
            when_true,
            when_false,
        }),
    });
    let carrier = graph.values.intern(Value {
        ty: crate::Type::I32,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::Convert,
            input: choice,
        }),
    });
    let mut reducer = Reducer::default();
    let result = reducer.reduce(&mut graph.values, carrier);
    assert!(graph.values[result].scalar_literal().is_none());
    for (truth, expected) in [(true, 0x100), (false, 0x200)] {
        let mut branch = branch(&reducer, &graph.values, condition, truth);
        let result = branch.reduce(&mut graph.values, carrier);
        assert_eq!(graph.values[result].scalar_literal(), Some(expected));
    }
}

#[test]
fn exact_carrier_constants_survive_specialization_and_fact_inference() {
    let mut graph = conditional_value();
    let condition = graph.blocks[0].parameters[0];
    let literal = graph.values.carrier_literal(crate::Type::I8, 256);
    let view = graph.values.intern(Value {
        ty: crate::Type::I32,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::Convert,
            input: literal,
        }),
    });
    let mut reducer = branch(&Reducer::default(), &graph.values, condition, true);
    assert_eq!(reducer.reduce(&mut graph.values, literal), literal);
    let result = reducer.reduce(&mut graph.values, view);
    assert!(matches!(graph.values[result].scalar_literal(), Some(256)));
}

#[test]
fn bitwise_absorption_cannot_discard_unknown_carrier_bits() {
    use crate::Type;

    for operator in [BitwiseOp::Or, BitwiseOp::And] {
        for reversed in [false, true] {
            let mut graph = FunctionGraph::new();
            let inputs: Vec<_> = (0..2)
                .map(|component| {
                    let wide = graph.values.push(Value {
                        ty: Type::I32,
                        definition: ValueDefinition::Parameter {
                            block: graph.entry,
                            component,
                        },
                    });
                    graph.values.intern(Value {
                        ty: Type::I8,
                        definition: ValueDefinition::Expression(Expression::Unary {
                            operator: Unary::Convert,
                            input: wide,
                        }),
                    })
                })
                .collect();
            let (left, right) = if reversed {
                (inputs[1], inputs[0])
            } else {
                (inputs[0], inputs[1])
            };
            let result = graph.values.intern(Value {
                ty: Type::I8,
                definition: ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::Bitwise(operator),
                    left,
                    right,
                }),
            });
            // The low bytes alone would permit absorption. Both narrow views
            // still have unknown upper i32 bits, which the operation must keep.
            let mut reducer = Reducer {
                analysis: ValueAnalysis::default().with_assumptions(
                    &graph.values,
                    [
                        Assumption::Bits {
                            value: inputs[0],
                            mask: 3,
                            bits: 3,
                        },
                        Assumption::Bits {
                            value: inputs[1],
                            mask: 0xfc,
                            bits: 0,
                        },
                    ],
                ),
                ..Reducer::default()
            };
            assert_eq!(graph.values.bounds[left].unsigned, 32);
            assert_eq!(graph.values.bounds[right].unsigned, 32);
            assert_eq!(reducer.reduce(&mut graph.values, result), result);
        }
    }
}

#[test]
fn absorbed_operands_are_specialized_and_aliases_stay_on_the_proven_path() {
    use crate::Type;

    let mut graph = FunctionGraph::new();
    let inputs: Vec<_> = (0..3)
        .map(|component| {
            graph.values.push(Value {
                ty: Type::I32,
                definition: ValueDefinition::Parameter {
                    block: graph.entry,
                    component,
                },
            })
        })
        .collect();
    let bit = graph.values.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::LowBits { bits: 1 },
            input: inputs[0],
        }),
    });
    let choice = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Select {
            condition: bit,
            when_true: inputs[1],
            when_false: inputs[2],
        }),
    });
    let update = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::LowBits { bits: 1 },
            input: inputs[0],
        }),
    });
    let result = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::Or),
            left: choice,
            right: update,
        }),
    });
    let mut live = Reducer::default();
    let mut branch = branch(&live, &graph.values, bit, true);
    branch.analysis = branch.analysis.clone().with_assumptions(
        &graph.values,
        [Assumption::Bits {
            value: inputs[1],
            mask: 1,
            bits: 1,
        }],
    );
    let absorbed = branch.reduce(&mut graph.values, result);
    assert_eq!(absorbed, inputs[1]);
    assert!(branch.recipes(absorbed).any(|recipe| recipe == choice));
    assert!(!live.recipes(absorbed).any(|recipe| recipe == choice));
    let unproven = live.reduce(&mut graph.values, result);
    assert!(matches!(
        graph.values[unproven].definition,
        ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::Or),
            ..
        })
    ));
}
