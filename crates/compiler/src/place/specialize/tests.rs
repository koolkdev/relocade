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
    assert_eq!(live.specialize(&mut graph, choice, |_, _| None), choice);
    for (truth, expected) in [(false, 11), (true, 7)] {
        let mut preview = live.on_branch(&graph, condition, truth);
        let result = preview.specialize(&mut graph, choice, |_, _| None);
        assert!(
            matches!(graph.values[result].definition, ValueDefinition::Constant(bits) if bits == expected)
        );
    }
    assert_eq!(live.specialize(&mut graph, choice, |_, _| None), choice);
    assert!(graph.blocks[0].items.is_empty());
}

#[test]
fn changing_facts_invalidates_previous_folds() {
    let mut graph = conditional_value();
    let choice = returned(&graph);
    let condition = graph.blocks[0].parameters[0];
    let mut specializer = Specializer::default();
    assert_eq!(
        specializer.specialize(&mut graph, choice, |_, _| None),
        choice
    );
    specializer
        .facts_mut()
        .assume(&graph.values, condition, true);
    let result = specializer.specialize(&mut graph, choice, |_, _| None);
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
        specializer.specialize(&mut graph, choice, |_, _| Some(placed)),
        placed
    );
    specializer.begin_block();
    assert_eq!(
        specializer.specialize(&mut graph, choice, |_, _| None),
        choice
    );
}
