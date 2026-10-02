use super::*;

fn conditional_value() -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    let condition = graph.values.push(Value {
        ty: crate::Type::I1,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 0,
        },
    });
    graph.blocks[0].parameters.push(condition);
    let when_true = graph.values.constant(crate::Type::I32, 7);
    let when_false = graph.values.constant(crate::Type::I32, 11);
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
        assert!(
            matches!(graph.values[result].definition, ValueDefinition::Constant(bits) if bits == expected)
        );
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
    specializer.begin_block();
    available.bind(result.value, result.value);
    assert_eq!(available.get(choice), Some(result.value));
    available.restore(scope);
    assert_eq!(available.get(choice), None);
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
    assert!(matches!(
        graph.values[result].definition,
        ValueDefinition::Constant(7)
    ));
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
    specializer.begin_block();
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
    let literal = graph.values.carrier_constant(crate::Type::I8, 256);
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
    assert!(matches!(
        graph.values[result].definition,
        ValueDefinition::Constant(256)
    ));
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
    assert!(matches!(
        graph.values[result].definition,
        ValueDefinition::Constant(7)
    ));
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
