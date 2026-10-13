//! Incoming executions must be complete before their values can justify reuse.
use super::*;
use crate::expression::UnaryOp as Unary;

#[test]
fn incoming_values_use_the_nearest_binding_on_each_predecessor_path() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let nearer = graph.block(0, &[]);
    let left_source = graph.block(0, &[]);
    graph.blocks[arms.left.0].exit = Exit::Jump(edge(nearer));
    graph.blocks[nearer.0].exit = Exit::Jump(edge(left_source));
    graph.blocks[left_source.0].exit = Exit::Jump(edge(arms.join));
    let recipe = square(&mut graph);
    let (mut joins, mut flow) = joins(&mut graph);
    let distant_value = placed(&mut graph, arms.left, recipe);
    let nearer_value = placed(&mut graph, nearer, recipe);
    let right_value = placed(&mut graph, arms.right, recipe);
    for (block, value) in [
        (arms.left, distant_value),
        (nearer, nearer_value),
        (arms.right, right_value),
    ] {
        joins.record(block.0, [(recipe, value)].into_iter());
    }
    for source in [left_source, arms.right] {
        joins.complete(source.0);
    }
    joins.prepare(
        &graph,
        &graph.reachable(),
        arms.join.0,
        &mut Availability::default(),
    );
    let result = joins
        .resolve(&mut graph, arms.join.0, recipe, &mut flow)
        .unwrap();
    assert_eq!(graph.blocks[arms.join.0].parameters, [result]);
    assert_eq!(
        graph.blocks[left_source.0].exit.edges()[0].arguments,
        [nearer_value]
    );
    assert_eq!(
        graph.blocks[arms.right.0].exit.edges()[0].arguments,
        [right_value]
    );
    assert!(graph.blocks[left_source.0].items.is_empty());
}

#[test]
fn incoming_aliases_see_a_common_ancestor_binding_requested_after_preparation() {
    let mut graph = graph();
    let earlier = Diamond::new(&mut graph, BlockId(0));
    let later = Diamond::new(&mut graph, earlier.join);
    let number = square(&mut graph);
    let choice = expression(
        &mut graph,
        Type::I32,
        Expression::Ternary {
            operator: Ternary::Select,
            first: 1,
            second: number,
            third: 0,
        },
    );
    let alias = expression(
        &mut graph,
        Type::I8,
        Expression::Unary {
            operator: Unary::Convert,
            input: choice,
        },
    );
    let (mut joins, mut flow) = joins(&mut graph);
    for source in [earlier.left, earlier.right] {
        let value = placed(&mut graph, source, number);
        joins.record(source.0, [(number, value)].into_iter());
        joins.complete(source.0);
    }
    joins.record(later.left.0, [(alias, 1)].into_iter());
    for source in [later.left, later.right] {
        joins.complete(source.0);
    }
    let reachable = graph.reachable();
    let mut available = Availability::default();
    joins.prepare(&graph, &reachable, earlier.join.0, &mut available);
    joins.prepare(&graph, &reachable, later.join.0, &mut available);

    // Both requests come from the current consuming block. The first publishes
    // an ancestor result needed to resolve the later join's incoming alias.
    let ancestor_value = joins
        .resolve(&mut graph, later.join.0, number, &mut flow)
        .unwrap();
    let alias_value = joins
        .resolve(&mut graph, later.join.0, alias, &mut flow)
        .unwrap();
    assert_eq!(graph.blocks[earlier.join.0].parameters, [ancestor_value]);
    assert_eq!(graph.blocks[later.join.0].parameters, [alias_value]);
    assert_eq!(graph.blocks[later.left.0].exit.edges()[0].arguments, [1]);
    assert_eq!(
        graph.blocks[later.right.0].exit.edges()[0].arguments,
        [ancestor_value]
    );
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
    let (mut joins, mut flow) = joins(&mut graph);
    joins.complete(0);
    joins.prepare(
        &graph,
        &graph.reachable(),
        header.0,
        &mut Availability::default(),
    );
    let value = placed(&mut graph, header, recipe);
    joins.record(header.0, [(recipe, value)].into_iter());
    joins.complete(header.0);
    assert_eq!(joins.resolve(&mut graph, after.0, recipe, &mut flow), None);
    assert!(graph.blocks[header.0].parameters.is_empty());
}

#[test]
fn a_missing_incoming_value_is_not_recomputed_or_merged() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let recipe = square(&mut graph);
    let (mut joins, mut flow) = joins(&mut graph);
    let value = placed(&mut graph, arms.left, recipe);
    joins.record(arms.left.0, [(recipe, value)].into_iter());
    joins.complete(arms.left.0);
    joins.complete(arms.right.0);
    joins.prepare(
        &graph,
        &graph.reachable(),
        arms.join.0,
        &mut Availability::default(),
    );
    let count = graph.values.len();
    for _ in 0..2 {
        assert_eq!(
            joins.resolve(&mut graph, arms.join.0, recipe, &mut flow),
            None
        );
    }
    assert_eq!(graph.values.len(), count);
    assert!(graph.blocks[arms.join.0].parameters.is_empty());
    assert!(graph.blocks[arms.right.0].items.is_empty());
}

#[test]
fn a_discarded_predecessor_does_not_need_a_completed_execution() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let recipe = square(&mut graph);
    let (mut joins, mut flow) = joins(&mut graph);
    let value = placed(&mut graph, arms.left, recipe);
    joins.record(arms.left.0, [(recipe, value)].into_iter());
    joins.complete(arms.left.0);
    let condition = graph.values.literal(Type::I1, 1);
    graph.blocks[0].exit = Exit::If {
        condition,
        taken: edge(arms.left),
        otherwise: edge(arms.right),
    };
    joins.prepare(
        &graph,
        &graph.reachable(),
        arms.join.0,
        &mut Availability::default(),
    );
    assert_eq!(
        joins.resolve(&mut graph, arms.join.0, recipe, &mut flow),
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
    let (mut joins, mut flow) = joins(&mut graph);
    let condition = graph.values.literal(Type::I1, 0);
    graph.blocks[0].exit = Exit::If {
        condition,
        taken: edge(outer.left),
        otherwise: edge(outer.right),
    };
    joins.prepare(
        &graph,
        &graph.reachable(),
        inner.join.0,
        &mut Availability::default(),
    );
    assert_eq!(
        joins.resolve(&mut graph, inner.join.0, recipe, &mut flow),
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
    let (mut joins, mut flow) = joins(&mut graph);
    let value = placed(&mut graph, arm, recipe);
    joins.record(arm.0, [(recipe, value)].into_iter());
    joins.complete(arm.0);
    let selector = graph.values.literal(Type::I32, 7);
    if let Exit::Switch {
        selector: input, ..
    } = &mut graph.blocks[0].exit
    {
        *input = selector;
    }
    joins.prepare(
        &graph,
        &graph.reachable(),
        after.0,
        &mut Availability::default(),
    );
    assert_eq!(
        joins.resolve(&mut graph, after.0, recipe, &mut flow),
        Some(value)
    );
}

#[test]
fn surviving_predecessors_keep_their_arguments_when_an_arm_is_removed() {
    let mut graph = graph();
    let outer = Diamond::new(&mut graph, BlockId(0));
    let inner = Diamond::new(&mut graph, outer.left);
    let independent = expression(
        &mut graph,
        Type::I1,
        Expression::Unary {
            operator: Unary::ZeroTest { nonzero: true },
            input: 1,
        },
    );
    if let Exit::If { condition, .. } = &mut graph.blocks[outer.left.0].exit {
        *condition = independent;
    }
    graph.blocks[inner.left.0].exit = Exit::Jump(edge(outer.join));
    graph.blocks[inner.right.0].exit = Exit::Jump(edge(outer.join));
    let recipe = square(&mut graph);
    let (mut joins, mut flow) = joins(&mut graph);
    let first = placed(&mut graph, outer.right, recipe);
    let second = placed(&mut graph, inner.right, recipe);
    for (block, value) in [(outer.right, first), (inner.right, second)] {
        joins.record(block.0, [(recipe, value)].into_iter());
        joins.complete(block.0);
    }
    let condition = graph.values.literal(Type::I1, 0);
    graph.blocks[outer.left.0].exit = Exit::If {
        condition,
        taken: edge(inner.left),
        otherwise: edge(inner.right),
    };
    joins.prepare(
        &graph,
        &graph.reachable(),
        outer.join.0,
        &mut Availability::default(),
    );
    let result = joins
        .resolve(&mut graph, outer.join.0, recipe, &mut flow)
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
    assert!(matches!(graph.values[filler].scalar_literal(), Some(0)));
}
