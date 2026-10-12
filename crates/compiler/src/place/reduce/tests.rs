use super::*;
use crate::bitwise::BitwiseOp;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};

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
fn branch_previews_have_independent_facts_and_residuals() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut live = Reducer::default();
    assert_eq!(live.reduce(&mut graph.values, choice), choice);
    for (truth, expected) in [(false, 11), (true, 7)] {
        let mut preview = live.on_branch(&graph.values, condition, truth);
        let result = preview.reduce(&mut graph.values, choice);
        assert!(matches!(graph.values[result].scalar_literal(), Some(bits) if bits == expected));
    }
    assert_eq!(live.reduce(&mut graph.values, choice), choice);
    assert!(graph.blocks[0].items.is_empty());
}

#[test]
fn semantic_equivalences_follow_their_path_context() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut reducer = Reducer::default();
    let scope = reducer.enter(
        &graph.values,
        None,
        Some(EdgeAssumption::Truth {
            condition,
            truth: true,
        }),
    );
    let residual = reducer.reduce(&mut graph.values, choice);
    assert!(reducer.recipes(residual).any(|recipe| recipe == choice));
    let unchanged = reducer.enter(&graph.values, None, None);
    assert_eq!(reducer.reduce(&mut graph.values, choice), residual);
    reducer.leave(unchanged);
    assert!(reducer.recipes(residual).any(|recipe| recipe == choice));
    reducer.leave(scope);
    assert!(!reducer.recipes(residual).any(|recipe| recipe == choice));
    assert_eq!(reducer.reduce(&mut graph.values, choice), choice);
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
        let mut branch = reducer.on_branch(&graph.values, condition, truth);
        let result = branch.reduce(&mut graph.values, carrier);
        assert_eq!(graph.values[result].scalar_literal(), Some(expected));
    }
}

#[test]
fn entering_a_refined_context_reconsiders_previous_folds() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut reducer = Reducer::default();
    assert_eq!(reducer.reduce(&mut graph.values, choice), choice);
    reducer.enter(
        &graph.values,
        None,
        Some(EdgeAssumption::Truth {
            condition,
            truth: true,
        }),
    );
    let result = reducer.reduce(&mut graph.values, choice);
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));
}

#[test]
fn nested_blocks_restore_inherited_and_replaced_facts_and_folds() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut reducer = Reducer::default();
    let inherited = reducer.enter(
        &graph.values,
        None,
        Some(EdgeAssumption::Truth {
            condition,
            truth: true,
        }),
    );
    let result = reducer.reduce(&mut graph.values, choice);
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));

    let mut incoming = ScalarFacts::default();
    incoming.assume(&graph.values, condition, false);
    let replaced = reducer.enter(&graph.values, Some(incoming), None);
    let seven = graph.values.literal(crate::Type::I32, 7);
    assert!(!reducer.recipes(seven).any(|recipe| recipe == choice));
    let child = reducer.enter(&graph.values, None, None);
    let result = reducer.reduce(&mut graph.values, choice);
    assert!(matches!(graph.values[result].scalar_literal(), Some(11)));
    reducer.leave(child);
    assert_eq!(reducer.facts().constant(&graph.values, condition), Some(0));
    reducer.leave(replaced);
    let result = reducer.reduce(&mut graph.values, choice);
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));
    reducer.leave(inherited);
    assert_eq!(reducer.reduce(&mut graph.values, choice), choice);
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
    let mut reducer = Reducer::default().on_branch(&graph.values, condition, true);
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
            let mut facts = ScalarFacts::default();
            facts.assume_bits(inputs[0], 3, 3);
            facts.assume_bits(inputs[1], 0xfc, 0);
            let mut reducer = Reducer::from_facts(facts);
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
    let mut branch = live.on_branch(&graph.values, bit, true);
    branch.facts.assume_bits(inputs[1], 1, 1);
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
