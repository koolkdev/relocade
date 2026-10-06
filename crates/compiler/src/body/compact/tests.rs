use super::*;
use crate::{
    body::{
        BitBounds, BlockId, Edge, Effect, Layout, Operation, OperationKind, Value, ValueDefinition,
    },
    integer::BinaryOp,
    memory::{Mem, MemoryAccess},
    Expression, Func, Type,
};

fn graph(types: &[Type]) -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    for (component, &ty) in types.iter().enumerate() {
        let value = graph.values.push(Value {
            ty,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component,
            },
        });
        graph.blocks[graph.entry.0].parameters.push(value);
    }
    graph
}

fn effect(graph: &mut FunctionGraph, operation: Operation, types: &[Type]) -> EffectId {
    let id = EffectId(graph.effects.len());
    let results = types
        .iter()
        .enumerate()
        .map(|(component, &ty)| {
            graph.values.push(Value {
                ty,
                definition: ValueDefinition::Result {
                    producer: BlockItem::Effect(id),
                    component,
                },
            })
        })
        .collect();
    graph.effects.push(Effect {
        operation,
        results,
        origin: graph.entry,
    });
    id
}

fn load(address: usize) -> Operation {
    Operation::load(
        MemoryAccess {
            memory: Mem(0),
            offset: 0,
            bytes: 4,
        },
        address,
    )
}

#[test]
fn complete_result_groups_and_stored_bounds_survive_compaction() {
    let mut graph = graph(&[Type::I8, Type::I64]);
    let address = graph.values.constant(Type::I32, 123);
    // A later parameter keeps its learned bounds when the dead address is removed.
    let input = graph.values.push_with_bounds(
        Value {
            ty: Type::I64,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component: 2,
            },
        },
        BitBounds {
            unsigned: 64,
            signed: 8,
        },
    );
    graph.blocks[0].parameters.push(input);
    graph.values.push(Value {
        ty: Type::I64,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: BinaryOp::Add,
            left: 1,
            right: input,
        }),
    });
    let wide = graph.values.push(Value {
        ty: Type::I64,
        definition: ValueDefinition::Expression(Expression::MultiplyWide {
            signed: false,
            left: 1,
            right: input,
        }),
    });
    effect(&mut graph, load(address), &[Type::I32]);
    let call = effect(
        &mut graph,
        Operation::call(Func(7), vec![wide + 1]),
        &[Type::I64, Type::I32],
    );
    let fence = effect(&mut graph, Operation::fence(), &[]);
    graph.blocks[0].items = vec![
        BlockItem::Evaluate(wide),
        BlockItem::Effect(call),
        BlockItem::Effect(fence),
    ];
    graph.blocks[0].exit = Exit::Return(vec![9]);
    graph.memories.push(Mem(0));

    graph.compact(vec![
        false, true, false, true, false, false, true, false, false, true,
    ]);

    assert_eq!(graph.values.len(), 7);
    assert_eq!(graph.effects.len(), 2);
    assert_eq!(graph.blocks[0].parameters, [0, 1, 2]);
    assert_eq!(graph.values.bounds[2].unsigned, 64);
    assert_eq!(graph.values.bounds[2].signed, 8);
    assert!(
        graph.blocks[0].items
            == [
                BlockItem::Evaluate(3),
                BlockItem::Effect(EffectId(0)),
                BlockItem::Effect(EffectId(1)),
            ]
    );
    assert_eq!(
        graph.results(BlockItem::Evaluate(3)).collect::<Vec<_>>(),
        [3, 4]
    );
    assert_eq!(
        graph.inputs(BlockItem::Evaluate(3)).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(matches!(
        graph.values[4].definition,
        ValueDefinition::Result {
            producer: BlockItem::Evaluate(3),
            component: 1
        }
    ));
    assert_eq!(graph.effects[0].results, [5, 6]);
    assert_eq!(graph.effects[0].operation.inputs().collect::<Vec<_>>(), [4]);
    assert!(matches!(
        graph.effects[0].operation.kind(),
        OperationKind::Call { target: Func(7) }
    ));
    assert!(matches!(
        graph.effects[1].operation.kind(),
        OperationKind::Fence
    ));
    assert!(matches!(
        graph.values[6].definition,
        ValueDefinition::Result {
            producer: BlockItem::Effect(EffectId(0)),
            component: 1
        }
    ));
    assert!(matches!(&graph.blocks[0].exit, Exit::Return(values) if values == &[6]));
    assert_eq!(graph.memories, [Mem(0)]);
}

#[test]
fn inactive_edge_occurrences_release_arguments_even_when_their_target_is_live() {
    for (ty, bits, active) in [
        (Type::I1, 1, 0),
        (Type::I1, 0, 1),
        (Type::I32, 5, 0),
        (Type::I32, 7, 1),
        (Type::I32, 99, 2),
    ] {
        let mut graph = graph(&[Type::I32]);
        let join = graph.block(0, &[Type::I32]);
        let selector = graph.values.constant(ty, bits);
        let argument = graph.values.constant(Type::I32, 42);
        let dead_read = effect(&mut graph, load(0), &[Type::I32]);
        let dead_argument = graph.effects[dead_read.0].results[0];
        let edge = |index| Edge {
            target: join,
            arguments: vec![if index == active {
                argument
            } else {
                dead_argument
            }],
        };
        graph.blocks[0].exit = if ty == Type::I1 {
            Exit::If {
                condition: selector,
                taken: edge(0),
                otherwise: edge(1),
            }
        } else {
            Exit::Switch {
                selector,
                cases: vec![(5, edge(0)), (7, edge(1))],
                default: edge(2),
            }
        };
        graph.blocks[join.0].exit = Exit::Return(vec![1]);

        graph.compact(vec![false, true, true, true, false]);

        assert_eq!(graph.values.len(), 5);
        assert!(graph.effects.is_empty());
        assert_eq!(graph.blocks[join.0].parameters, [1]);
        assert_eq!(graph.outgoing(BlockId(0))[0].arguments, [3]);
        for (index, edge) in graph.blocks[0].exit.edges().into_iter().enumerate() {
            assert_eq!(edge.target, join);
            assert_eq!(edge.arguments.len(), 1);
            let expected = if index == active { 42 } else { 0 };
            assert!(
                matches!(graph.values[edge.arguments[0]].definition, ValueDefinition::Constant(bits) if bits == expected)
            );
        }
        // Finalization can compact again after replacing equivalent values.
        graph.compact(vec![true; graph.values.len()]);
        assert_eq!(graph.values.len(), 5);
    }
}

#[test]
fn unreachable_blocks_release_their_contents_but_keep_layout_anchors() {
    let mut graph = graph(&[Type::I32]);
    let discarded = graph.block(0, &[Type::I32]);
    let read = effect(&mut graph, load(1), &[Type::I32]);
    graph.effects[read.0].origin = discarded;
    graph.blocks[discarded.0]
        .items
        .push(BlockItem::Effect(read));
    graph.blocks[discarded.0].exit = Exit::Return(vec![2]);
    graph.blocks[0].exit = Exit::Return(vec![0]);
    graph.layout = vec![Layout::Block(BlockId(0)), Layout::Block(discarded)];

    graph.compact(vec![true, false, false]);

    assert_eq!(graph.values.len(), 1);
    assert!(graph.effects.is_empty());
    assert_eq!(graph.blocks.len(), 2);
    assert_eq!(graph.reachable(), [true, false]);
    assert_eq!(graph.blocks[discarded.0].items.capacity(), 0);
    assert_eq!(graph.blocks[discarded.0].parameters.capacity(), 0);
    assert!(matches!(graph.blocks[discarded.0].exit, Exit::Trap));
    assert!(
        matches!(graph.layout.as_slice(), [Layout::Block(BlockId(0)), Layout::Block(block)] if *block == discarded)
    );
}

#[test]
fn loop_parameters_and_parallel_backedge_arguments_are_remapped_together() {
    let mut graph = graph(&[Type::I32, Type::I32]);
    graph.values.constant(Type::I32, 99);
    let header = graph.block(0, &[Type::I32, Type::I32]);
    graph.blocks[0].exit = Exit::Jump(Edge {
        target: header,
        arguments: vec![0, 1],
    });
    graph.blocks[header.0].exit = Exit::Jump(Edge {
        target: header,
        arguments: vec![4, 3],
    });

    graph.compact(vec![true, true, false, true, true]);

    assert_eq!(graph.values.len(), 4);
    assert_eq!(graph.blocks[header.0].parameters, [2, 3]);
    assert_eq!(graph.outgoing(header)[0].arguments, [3, 2]);
    for (component, value) in [2, 3].into_iter().enumerate() {
        assert!(
            matches!(graph.values[value].definition, ValueDefinition::Parameter { block, component: actual } if block == header && actual == component)
        );
    }
}
