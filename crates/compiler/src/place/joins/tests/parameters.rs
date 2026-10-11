//! Forward authored parameters only when incoming execution values agree.
use super::*;

fn parameter(graph: &mut FunctionGraph, block: BlockId, ty: Type) -> usize {
    let component = graph.blocks[block.0].parameters.len();
    let value = graph.values.push(Value {
        ty,
        definition: ValueDefinition::Parameter { block, component },
    });
    graph.blocks[block.0].parameters.push(value);
    value
}

#[test]
fn one_completed_incoming_edge_makes_its_placed_argument_available_at_the_join() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let result = parameter(&mut graph, arms.join, Type::I32);
    let recipe = square(&mut graph);
    let argument = placed(&mut graph, arms.left, recipe);
    let unused = graph.values.literal(Type::I32, 99);
    graph.blocks[arms.left.0].exit.edges_mut()[0]
        .arguments
        .push(argument);
    graph.blocks[arms.right.0].exit.edges_mut()[0]
        .arguments
        .push(unused);
    let mut joins = joins(&graph);
    joins.complete(arms.left.0, &ValueAnalysis::default());
    // Model a branch folded after the original dominance analysis. The unused
    // arm is deliberately not completed and cannot constrain this join.
    let yes = graph.values.literal(Type::I1, 1);
    if let Exit::If { condition, .. } = &mut graph.blocks[0].exit {
        *condition = yes;
    }
    let mut available = Availability::default();
    joins.prepare(&graph, &graph.reachable(), arms.join.0, &mut available);
    assert_eq!(available.get(result), Some(argument));
    assert_eq!(available.get(argument), Some(argument));
}

#[test]
fn two_live_edges_from_a_single_predecessor_keep_their_parameter() {
    let mut graph = graph();
    let join = graph.block(0, &[Type::I32]);
    let result = graph.blocks[join.0].parameters[0];
    let seven = graph.values.literal(Type::I32, 7);
    graph.blocks[0].exit = Exit::If {
        condition: 0,
        taken: Edge {
            target: join,
            arguments: vec![1],
        },
        otherwise: Edge {
            target: join,
            arguments: vec![seven],
        },
    };
    graph.blocks[join.0].exit = Exit::Return(vec![result]);
    let mut joins = joins(&graph);
    let mut available = Availability::default();
    joins.prepare(&graph, &graph.reachable(), join.0, &mut available);
    assert_eq!(available.get(result), None);
}
