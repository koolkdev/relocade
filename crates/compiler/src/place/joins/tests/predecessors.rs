//! Incoming paths must be complete before their values or facts can justify reuse.
use super::*;

#[test]
fn completed_facts_survive_rollback_of_the_incoming_paths() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let mut joins = joins(&graph);
    let mut facts = Facts::default();
    for block in [arms.left, arms.right] {
        let scope = facts.checkpoint();
        facts.assume_bits(1, 0xffff_ffff, 5);
        joins.complete(block.0, &facts);
        facts.restore(scope);
        assert_eq!(facts.constant(&graph.values, 1), None);
    }
    let merged = joins
        .prepare(&graph, arms.join.0, &Availability::default())
        .unwrap();
    assert_eq!(merged.constant(&graph.values, 1), Some(5));
}

#[test]
fn a_join_keeps_common_facts_without_any_value_candidates() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let one = graph.values.constant(Type::I32, 1);
    let low_bit = expression(
        &mut graph,
        Type::I32,
        Expression::Binary {
            operator: BinaryOp::And,
            left: 1,
            right: one,
        },
    );
    let mut joins = joins(&graph);
    for (block, value) in [(arms.left, 5), (arms.right, 7)] {
        let mut facts = Facts::default();
        facts.assume_bits(1, 0xffff_ffff, value);
        joins.complete(block.0, &facts);
    }
    let facts = joins
        .prepare(&graph, arms.join.0, &Availability::default())
        .unwrap();
    assert_eq!(facts.constant(&graph.values, low_bit), Some(1));
    assert_eq!(facts.constant(&graph.values, 1), None);
    assert!(joins.inputs[arms.join.0].is_none());
    assert!(graph.blocks[arms.join.0].parameters.is_empty());
}

#[test]
fn every_reachable_predecessor_must_prove_a_common_fact() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let bypass = graph.block(0, &[]);
    let selector = graph.values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 2,
        },
    });
    graph.blocks[0].parameters.push(selector);
    graph.blocks[0].exit = Exit::Switch {
        selector,
        cases: vec![(0, edge(arms.left)), (1, edge(arms.right))],
        default: edge(bypass),
    };
    graph.blocks[bypass.0].exit = Exit::Jump(edge(arms.join));
    let mut joins = joins(&graph);
    for source in [arms.left, arms.right] {
        let mut facts = Facts::default();
        facts.assume_bits(1, 0xffff_ffff, 5);
        joins.complete(source.0, &facts);
    }
    joins.complete(bypass.0, &Facts::default());
    let facts = joins
        .prepare(&graph, arms.join.0, &Availability::default())
        .unwrap();
    assert_eq!(facts.constant(&graph.values, 1), None);
}

#[test]
fn completing_a_backedge_does_not_reopen_entry_eligibility() {
    let mut graph = graph();
    let header = graph.block(0, &[]);
    let after = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::Jump(edge(header));
    graph.blocks[header.0].exit = Exit::If {
        condition: 0,
        taken: edge(header),
        otherwise: edge(after),
    };
    graph.blocks[after.0].exit = Exit::Return(vec![1]);
    let recipe = square(&mut graph);
    let mut joins = joins(&graph);
    joins.complete(0, &Facts::default());
    assert!(joins
        .prepare(&graph, header.0, &Availability::default())
        .is_none());
    let value = placed(&mut graph, header, recipe);
    joins.record(header.0, [(recipe, value)].into_iter());
    joins.complete(header.0, &Facts::default());
    assert_eq!(
        joins.resolve(&mut graph, after.0, recipe, &Facts::default()),
        None
    );
    assert!(graph.blocks[header.0].parameters.is_empty());
}

#[test]
fn a_missing_incoming_value_is_not_recomputed_or_merged() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let recipe = square(&mut graph);
    let mut joins = joins(&graph);
    let value = placed(&mut graph, arms.left, recipe);
    joins.record(arms.left.0, [(recipe, value)].into_iter());
    joins.complete(arms.left.0, &Facts::default());
    joins.complete(arms.right.0, &Facts::default());
    joins
        .prepare(&graph, arms.join.0, &Availability::default())
        .unwrap();
    let count = graph.values.len();
    for _ in 0..2 {
        assert_eq!(
            joins.resolve(&mut graph, arms.join.0, recipe, &Facts::default()),
            None
        );
    }
    assert_eq!(graph.values.len(), count);
    assert!(graph.blocks[arms.join.0].parameters.is_empty());
    assert!(graph.blocks[arms.right.0].items.is_empty());
}

#[test]
fn a_discarded_predecessor_does_not_need_a_value_or_completed_facts() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let recipe = square(&mut graph);
    let mut joins = joins(&graph);
    let value = placed(&mut graph, arms.left, recipe);
    joins.record(arms.left.0, [(recipe, value)].into_iter());
    joins.complete(arms.left.0, &Facts::default());
    let condition = graph.values.constant(Type::I1, 1);
    graph.blocks[0].exit = Exit::If {
        condition,
        taken: edge(arms.left),
        otherwise: edge(arms.right),
    };
    joins
        .prepare(&graph, arms.join.0, &Availability::default())
        .unwrap();
    assert_eq!(
        joins.resolve(&mut graph, arms.join.0, recipe, &Facts::default()),
        Some(value)
    );
    assert!(graph.blocks[arms.join.0].parameters.is_empty());
}

#[test]
fn a_join_inside_a_discarded_arm_has_no_incoming_values() {
    let mut graph = graph();
    let outer = Diamond::new(&mut graph, BlockId(0));
    let inner = Diamond::new(&mut graph, outer.left);
    let recipe = square(&mut graph);
    let mut joins = joins(&graph);
    let condition = graph.values.constant(Type::I1, 0);
    graph.blocks[0].exit = Exit::If {
        condition,
        taken: edge(outer.left),
        otherwise: edge(outer.right),
    };
    assert!(joins
        .prepare(&graph, inner.join.0, &Availability::default())
        .is_none());
    assert_eq!(
        joins.resolve(&mut graph, inner.join.0, recipe, &Facts::default()),
        None
    );
    assert!(graph.blocks[inner.join.0].parameters.is_empty());
}

#[test]
fn a_folded_switch_excludes_an_inactive_edge_from_a_reachable_source() {
    let mut graph = graph();
    let arm = graph.block(0, &[]);
    let after = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::Switch {
        selector: 1,
        cases: vec![(7, edge(arm))],
        default: edge(after),
    };
    graph.blocks[arm.0].exit = Exit::Jump(edge(after));
    graph.blocks[after.0].exit = Exit::Return(vec![1]);
    let recipe = square(&mut graph);
    let mut joins = joins(&graph);
    let value = placed(&mut graph, arm, recipe);
    joins.record(arm.0, [(recipe, value)].into_iter());
    joins.complete(arm.0, &Facts::default());
    let selector = graph.values.constant(Type::I32, 7);
    if let Exit::Switch {
        selector: input, ..
    } = &mut graph.blocks[0].exit
    {
        *input = selector;
    }
    joins
        .prepare(&graph, after.0, &Availability::default())
        .unwrap();
    assert_eq!(
        joins.resolve(&mut graph, after.0, recipe, &Facts::default()),
        Some(value)
    );
}

#[test]
fn surviving_predecessors_keep_their_arguments_when_an_arm_is_removed() {
    let mut graph = graph();
    let outer = Diamond::new(&mut graph, BlockId(0));
    let inner = Diamond::new(&mut graph, outer.left);
    graph.blocks[inner.left.0].exit = Exit::Jump(edge(outer.join));
    graph.blocks[inner.right.0].exit = Exit::Jump(edge(outer.join));
    let recipe = square(&mut graph);
    let mut joins = joins(&graph);
    let first = placed(&mut graph, outer.right, recipe);
    let second = placed(&mut graph, inner.right, recipe);
    for (block, value) in [(outer.right, first), (inner.right, second)] {
        joins.record(block.0, [(recipe, value)].into_iter());
        joins.complete(block.0, &Facts::default());
    }
    let condition = graph.values.constant(Type::I1, 0);
    graph.blocks[outer.left.0].exit = Exit::If {
        condition,
        taken: edge(inner.left),
        otherwise: edge(inner.right),
    };
    joins
        .prepare(&graph, outer.join.0, &Availability::default())
        .unwrap();
    let result = joins
        .resolve(&mut graph, outer.join.0, recipe, &Facts::default())
        .unwrap();
    assert_eq!(graph.blocks[outer.join.0].parameters, [result]);
    assert_eq!(
        graph.blocks[outer.right.0].exit.edges()[0].arguments,
        [first]
    );
    assert_eq!(
        graph.blocks[inner.right.0].exit.edges()[0].arguments,
        [second]
    );
    let filler = graph.blocks[inner.left.0].exit.edges()[0].arguments[0];
    assert!(matches!(
        graph.values[filler].definition,
        ValueDefinition::Constant(0)
    ));
}
