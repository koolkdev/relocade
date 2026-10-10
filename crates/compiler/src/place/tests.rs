//! Placement must leave discarded control flow unvisited.
use super::*;
use crate::{integer::BinaryOp, Type};

fn graph(selector_type: Type) -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    for (component, ty) in [selector_type, Type::I32].into_iter().enumerate() {
        let parameter = graph.values.push(Value {
            ty,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component,
            },
        });
        graph.blocks[0].parameters.push(parameter);
    }
    graph
}

fn edge(target: BlockId) -> Edge {
    Edge {
        target,
        arguments: Vec::new(),
    }
}

fn square(graph: &mut FunctionGraph) -> usize {
    let number = graph.blocks[0].parameters[1];
    graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Mul,
            left: number,
            right: number,
        }),
    })
}

fn assert_return_unchanged(graph: &FunctionGraph, block: BlockId, recipe: usize) {
    // Visiting this exit would specialize its input under the enclosing guard.
    // Inspect placement before finalization reclaims discarded block contents.
    let Exit::Return(values) = &graph.blocks[block.0].exit else {
        panic!("the discarded block retains its return");
    };
    assert_eq!(values, &[recipe]);
}

#[test]
fn a_folded_if_skips_its_discarded_subtree_and_keeps_the_join() {
    let mut graph = graph(Type::I1);
    let condition = graph.blocks[0].parameters[0];
    let number = graph.blocks[0].parameters[1];
    let guarded = graph.block(0, &[]);
    let discarded = graph.block(0, &[]);
    let descendant = graph.block(0, &[]);
    let join = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::If {
        condition,
        taken: edge(guarded),
        otherwise: edge(join),
    };
    graph.blocks[guarded.0].exit = Exit::If {
        condition,
        taken: edge(join),
        otherwise: edge(discarded),
    };
    let product = square(&mut graph);
    let conditional_product = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Select {
            condition,
            when_true: number,
            when_false: product,
        }),
    });
    graph.blocks[discarded.0].exit = Exit::Jump(edge(descendant));
    graph.blocks[descendant.0].exit = Exit::Return(vec![conditional_product]);
    let sum = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Add,
            left: number,
            right: number,
        }),
    });
    graph.blocks[join.0].exit = Exit::Return(vec![sum]);
    assert!(graph.reachable().iter().all(|&reachable| reachable));

    place_calculations(&mut graph, &[]);

    assert_return_unchanged(&graph, descendant, conditional_product);
    let reachable = graph.reachable();
    assert!(!reachable[discarded.0]);
    assert!(!reachable[descendant.0]);
    assert!(reachable[join.0]);
    let Exit::Return(values) = &graph.blocks[join.0].exit else {
        panic!("the surviving join returns its calculation");
    };
    assert!(graph.blocks[join.0]
        .items
        .contains(&BlockItem::Evaluate(values[0])));
}

#[test]
fn a_folded_switch_skips_discarded_cases_and_default() {
    let mut graph = graph(Type::I32);
    let selector = graph.blocks[0].parameters[0];
    let number = graph.blocks[0].parameters[1];
    let guarded = graph.block(0, &[]);
    let discarded_case = graph.block(0, &[]);
    let discarded_default = graph.block(0, &[]);
    let join = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::Switch {
        selector,
        cases: vec![(7, edge(guarded))],
        default: edge(join),
    };
    graph.blocks[guarded.0].exit = Exit::Switch {
        selector,
        cases: vec![(7, edge(join)), (9, edge(discarded_case))],
        default: edge(discarded_default),
    };
    let product = square(&mut graph);
    let sum = graph.values.intern(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Add,
            left: product,
            right: selector,
        }),
    });
    graph.blocks[discarded_case.0].exit = Exit::Return(vec![sum]);
    graph.blocks[discarded_default.0].exit = Exit::Return(vec![sum]);
    graph.blocks[join.0].exit = Exit::Return(vec![number]);
    assert!(graph.reachable().iter().all(|&reachable| reachable));

    place_calculations(&mut graph, &[]);

    assert_return_unchanged(&graph, discarded_case, sum);
    assert_return_unchanged(&graph, discarded_default, sum);
    let reachable = graph.reachable();
    assert!(!reachable[discarded_case.0]);
    assert!(!reachable[discarded_default.0]);
    assert!(reachable[join.0]);
}

#[test]
fn a_loop_backedge_does_not_keep_a_discarded_region_alive() {
    let mut graph = graph(Type::I1);
    let condition = graph.blocks[0].parameters[0];
    let number = graph.blocks[0].parameters[1];
    let guarded = graph.block(0, &[]);
    let header = graph.block(0, &[]);
    let backedge = graph.block(0, &[]);
    let after = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::If {
        condition,
        taken: edge(guarded),
        otherwise: edge(after),
    };
    graph.blocks[guarded.0].exit = Exit::If {
        condition,
        taken: edge(after),
        otherwise: edge(header),
    };
    let product = square(&mut graph);
    let nonzero = graph.values.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::ZeroTest {
            input: product,
            nonzero: true,
        }),
    });
    let zero = graph.values.literal(Type::I1, 0);
    let repeat = graph.values.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Select {
            condition,
            when_true: nonzero,
            when_false: zero,
        }),
    });
    graph.blocks[header.0].exit = Exit::If {
        condition: repeat,
        taken: edge(backedge),
        otherwise: edge(after),
    };
    graph.blocks[backedge.0].exit = Exit::Jump(edge(header));
    graph.blocks[after.0].exit = Exit::Return(vec![number]);
    assert!(graph.reachable().iter().all(|&reachable| reachable));

    place_calculations(&mut graph, &[]);

    let Exit::If { condition, .. } = graph.blocks[header.0].exit else {
        panic!("the discarded loop retains its conditional exit");
    };
    assert_eq!(condition, repeat);
    let reachable = graph.reachable();
    assert!(!reachable[header.0]);
    assert!(!reachable[backedge.0]);
    assert!(reachable[after.0]);
}
