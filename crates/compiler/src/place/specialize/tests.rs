use super::*;
use crate::bitwise::BitwiseOp;

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
    let mut live = Specializer::default();
    assert_eq!(
        live.specialize(&mut graph, choice, |_, _, _| None).value,
        choice
    );
    for (truth, expected) in [(false, 11), (true, 7)] {
        let mut preview = live.on_branch(&graph.values, &Availability::default(), condition, truth);
        let result = preview.specialize(&mut graph, choice, |_, _, _| None).value;
        assert!(matches!(graph.values[result].scalar_literal(), Some(bits) if bits == expected));
    }
    assert_eq!(
        live.specialize(&mut graph, choice, |_, _, _| None).value,
        choice
    );
    assert!(graph.blocks[0].items.is_empty());
}

#[test]
fn reported_aliases_survive_the_memo_and_leave_with_their_scope() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut available = Availability::default();
    let scope = available.checkpoint();
    let mut specializer =
        Specializer::default().on_branch(&graph.values, &available, condition, true);
    let result = specializer.specialize(&mut graph, choice, |_, _, _| None);
    assert!(result
        .aliases
        .iter()
        .any(|alias| alias.recipe == choice && alias.residual == result.value));
    available.record_aliases(result.aliases);
    let block = specializer.begin_block(None);
    available.bind(result.value, result.value);
    assert_eq!(available.get(choice), Some(result.value));
    available.restore(scope);
    assert_eq!(available.get(choice), None);
    specializer.end_block(block);
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
        definition: ValueDefinition::Expression(Expression::Convert { input: choice }),
    });
    let mut specializer = Specializer::default();
    let result = specializer
        .specialize(&mut graph, carrier, |_, _, _| None)
        .value;
    assert!(graph.values[result].scalar_literal().is_none());
    for (truth, expected) in [(true, 0x100), (false, 0x200)] {
        let mut branch =
            specializer.on_branch(&graph.values, &Availability::default(), condition, truth);
        let result = branch.specialize(&mut graph, carrier, |_, _, _| None).value;
        assert_eq!(graph.values[result].scalar_literal(), Some(expected));
    }
}

#[test]
fn changing_facts_invalidates_previous_folds() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut specializer = Specializer::default();
    assert_eq!(
        specializer
            .specialize(&mut graph, choice, |_, _, _| None)
            .value,
        choice
    );
    specializer
        .facts_mut()
        .assume(&graph.values, condition, true);
    let result = specializer
        .specialize(&mut graph, choice, |_, _, _| None)
        .value;
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));
}

#[test]
fn entering_another_block_discards_cached_availability() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let placed = graph.values.push(graph.values[choice]);
    graph.blocks[0].items.push(BlockItem::Evaluate(placed));
    let mut specializer = Specializer::default();
    assert_eq!(
        specializer
            .specialize(&mut graph, choice, |_, _, _| Some(placed))
            .value,
        placed
    );
    let block = specializer.begin_block(None);
    assert_eq!(
        specializer
            .specialize(&mut graph, choice, |_, _, _| None)
            .value,
        choice
    );
    specializer.end_block(block);
}

#[test]
fn nested_blocks_restore_inherited_and_replaced_facts_and_folds() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut specializer = Specializer::default();
    let inherited = specializer.begin_block(None);
    specializer
        .facts_mut()
        .assume(&graph.values, condition, true);
    let result = specializer
        .specialize(&mut graph, choice, |_, _, _| None)
        .value;
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));

    let mut incoming = ScalarFacts::default();
    incoming.assume(&graph.values, condition, false);
    let replaced = specializer.begin_block(Some(incoming));
    let child = specializer.begin_block(None);
    let result = specializer
        .specialize(&mut graph, choice, |_, _, _| None)
        .value;
    assert!(matches!(graph.values[result].scalar_literal(), Some(11)));
    specializer.end_block(child);
    assert_eq!(
        specializer.facts().constant(&graph.values, condition),
        Some(0)
    );
    specializer.end_block(replaced);
    let result = specializer
        .specialize(&mut graph, choice, |_, _, _| None)
        .value;
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));
    specializer.end_block(inherited);
    assert_eq!(
        specializer
            .specialize(&mut graph, choice, |_, _, _| None)
            .value,
        choice
    );
}

#[test]
fn exact_carrier_constants_survive_specialization_and_fact_inference() {
    let mut graph = conditional_value();
    let condition = graph.blocks[0].parameters[0];
    let literal = graph.values.carrier_literal(crate::Type::I8, 256);
    let view = graph.values.intern(Value {
        ty: crate::Type::I32,
        definition: ValueDefinition::Expression(Expression::Convert { input: literal }),
    });
    let mut specializer =
        Specializer::default().on_branch(&graph.values, &Availability::default(), condition, true);
    assert_eq!(
        specializer
            .specialize(&mut graph, literal, |_, _, _| None)
            .value,
        literal
    );
    let result = specializer
        .specialize(&mut graph, view, |_, _, _| None)
        .value;
    assert!(matches!(graph.values[result].scalar_literal(), Some(256)));
}

#[test]
fn branch_facts_follow_executed_conditions_to_their_source_recipes() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let observed = graph.values.push(Value {
        ty: crate::Type::I1,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 1,
        },
    });
    let mut available = Availability::default();
    available.bind(condition, observed);
    let mut specializer = Specializer::default();
    specializer.assume(&graph.values, &available, observed, true);
    let result = specializer
        .specialize(&mut graph, choice, |_, _, _| None)
        .value;
    assert!(matches!(graph.values[result].scalar_literal(), Some(7)));
}

#[test]
fn observing_a_narrow_replacement_does_not_define_the_sources_upper_bits() {
    let mut values = ValueTable::default();
    let wide = values.push(Value {
        ty: crate::Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let byte = values.push(Value {
        ty: crate::Type::I8,
        definition: ValueDefinition::Expression(Expression::Convert { input: wide }),
    });
    assert_eq!(values.bounds[byte].unsigned, 32);
    let mut available = Availability::default();
    available.bind(wide, byte);
    let mut specializer = Specializer::default();
    // Only the low byte of this replacement is observed by the switch.
    specializer.equal(&values, &available, byte, 0x81);
    assert_eq!(specializer.facts.constant(&values, byte), Some(0x81));
    assert_eq!(specializer.facts.constant(&values, wide), None);

    let mut specializer = Specializer::default();
    specializer.assume(&values, &available, byte, true);
    assert_eq!(specializer.facts.constant(&values, wide), None);
    assert_eq!(specializer.facts.constant(&values, byte), None);
    let bit = values.push(Value {
        ty: crate::Type::I1,
        definition: ValueDefinition::Expression(Expression::Convert { input: wide }),
    });
    assert_eq!(specializer.facts.constant(&values, bit), Some(1));
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
                        definition: ValueDefinition::Expression(Expression::Convert {
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
                definition: ValueDefinition::Expression(Expression::Bitwise {
                    operator,
                    left,
                    right,
                }),
            });
            let mut specializer = Specializer::default();
            // The low bytes alone would permit absorption. Both narrow views
            // still have unknown upper i32 bits, which the operation must keep.
            specializer.facts_mut().assume_bits(inputs[0], 3, 3);
            specializer.facts_mut().assume_bits(inputs[1], 0xfc, 0);
            assert_eq!(graph.values.bounds[left].unsigned, 32);
            assert_eq!(graph.values.bounds[right].unsigned, 32);
            assert_eq!(
                specializer
                    .specialize(&mut graph, result, |_, _, _| None)
                    .value,
                result
            );
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
        definition: ValueDefinition::Expression(Expression::LowBits {
            input: inputs[0],
            bits: 1,
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
        definition: ValueDefinition::Expression(Expression::LowBits {
            input: inputs[0],
            bits: 1,
        }),
    });
    let result = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Bitwise {
            operator: BitwiseOp::Or,
            left: choice,
            right: update,
        }),
    });
    let mut available = Availability::default();
    let scope = available.checkpoint();
    let mut live = Specializer::default();
    let mut branch = live.on_branch(&graph.values, &available, bit, true);
    branch.facts_mut().assume_bits(inputs[1], 1, 1);
    let absorbed = branch.specialize(&mut graph, result, |_, _, _| None);
    assert_eq!(absorbed.value, inputs[1]);
    assert!(absorbed
        .aliases
        .iter()
        .any(|alias| alias.recipe == choice && alias.residual == inputs[1]));
    available.record_aliases(absorbed.aliases);
    available.bind(inputs[1], inputs[1]);
    assert_eq!(available.get(result), Some(inputs[1]));
    available.restore(scope);
    assert_eq!(available.get(result), None);
    let unproven = live.specialize(&mut graph, result, |_, _, _| None).value;
    assert!(matches!(
        graph.values[unproven].definition,
        ValueDefinition::Expression(Expression::Bitwise {
            operator: BitwiseOp::Or,
            ..
        })
    ));
}
