use super::*;
use crate::{integer::BinaryOp, Type};

fn graph() -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    for (component, ty) in [Type::I1, Type::I1, Type::I32, Type::I32]
        .into_iter()
        .enumerate()
    {
        let id = graph.values.push(Value {
            ty,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component,
            },
        });
        graph.blocks[0].parameters.push(id);
    }
    graph
}

fn xor(graph: &mut FunctionGraph, block: BlockId) -> usize {
    let id = evaluate(
        graph,
        block,
        Type::I32,
        Expression::Binary {
            operator: BinaryOp::Xor,
            left: 2,
            right: 3,
        },
    );
    graph.blocks[block.0].exit = Exit::Return(vec![id]);
    id
}

fn evaluate(
    graph: &mut FunctionGraph,
    block: BlockId,
    ty: Type,
    expression: Expression<usize>,
) -> usize {
    let id = graph.values.push(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    });
    graph.blocks[block.0].items.push(BlockItem::Evaluate(id));
    id
}

fn edge(target: BlockId) -> Edge {
    Edge {
        target,
        arguments: Vec::new(),
    }
}

#[test]
fn surviving_evaluations_share_only_when_their_paths_cover_the_branch() {
    for bypass in [false, true] {
        let mut graph = graph();
        let taken = graph.block(0, &[]);
        let otherwise = graph.block(0, &[]);
        graph.blocks[0].exit = Exit::If {
            condition: 0,
            taken: edge(taken),
            otherwise: edge(otherwise),
        };
        let first = xor(&mut graph, taken);
        let second = xor(&mut graph, otherwise);
        if bypass {
            let dispatch = graph.block(0, &[]);
            let unused = graph.block(0, &[]);
            graph.blocks[0].exit = Exit::If {
                condition: 0,
                taken: edge(taken),
                otherwise: edge(dispatch),
            };
            graph.blocks[dispatch.0].exit = Exit::If {
                condition: 1,
                taken: edge(otherwise),
                otherwise: edge(unused),
            };
            graph.blocks[unused.0].exit = Exit::Return(vec![2]);
        }
        assert_eq!(share(&mut graph, None).is_some(), !bypass);
        if bypass {
            assert!(graph.blocks[0].items.is_empty());
            assert!(graph.blocks[taken.0].items == [BlockItem::Evaluate(first)]);
            assert!(graph.blocks[otherwise.0].items == [BlockItem::Evaluate(second)]);
        } else {
            assert!(graph.blocks[0].items == [BlockItem::Evaluate(first)]);
            assert!(graph.blocks[taken.0].items.is_empty());
            assert!(graph.blocks[otherwise.0].items.is_empty());
        }
    }
}

#[test]
fn an_existing_evaluation_is_chosen_by_instruction_order() {
    let mut graph = graph();
    let first_allocated = xor(&mut graph, BlockId(0));
    let first_executed = xor(&mut graph, BlockId(0));
    graph.blocks[0].items.reverse();
    let effect = EffectId(graph.effects.len());
    graph.effects.push(Effect {
        results: Vec::new(),
        operation: Operation::call(crate::Func(0), vec![first_allocated]),
        origin: graph.entry,
    });
    graph.blocks[0].items.push(BlockItem::Effect(effect));
    graph.blocks[0].exit = Exit::Return(vec![first_allocated]);
    let replacements = share(&mut graph, None).unwrap();
    assert_eq!(replacements[first_allocated], first_executed);
    assert!(
        graph.blocks[0].items
            == [
                BlockItem::Evaluate(first_executed),
                BlockItem::Effect(effect)
            ]
    );
    graph.replace_values(replacements);
    assert_eq!(graph.values.len(), 5);
    assert_eq!(
        graph.inputs(BlockItem::Effect(effect)).collect::<Vec<_>>(),
        [4]
    );
    let Exit::Return(values) = &graph.blocks[0].exit else {
        unreachable!()
    };
    assert_eq!(values, &[4]);
}

#[test]
fn shared_dependencies_follow_their_inputs_before_the_branch_condition() {
    for effect_input in [false, true] {
        let mut graph = graph();
        let entry = graph.entry;
        let mut input = evaluate(
            &mut graph,
            entry,
            Type::I32,
            Expression::Binary {
                operator: BinaryOp::Add,
                left: 2,
                right: 3,
            },
        );
        let argument = input;
        let effect = EffectId(graph.effects.len());
        let mut results = Vec::new();
        if effect_input {
            let result = graph.values.push(Value {
                ty: Type::I32,
                definition: ValueDefinition::Result {
                    producer: BlockItem::Effect(effect),
                    component: 0,
                },
            });
            results.push(result);
            input = result;
        }
        graph.effects.push(Effect {
            results,
            operation: Operation::call(crate::Func(0), vec![argument]),
            origin: entry,
        });
        graph.blocks[0].items.push(BlockItem::Effect(effect));
        let mut expected = graph.blocks[0].items.clone();
        let condition = evaluate(
            &mut graph,
            entry,
            Type::I1,
            Expression::ZeroTest {
                input: 2,
                nonzero: true,
            },
        );
        let taken = graph.block(0, &[]);
        let otherwise = graph.block(0, &[]);
        graph.blocks[0].exit = Exit::If {
            condition,
            taken: edge(taken),
            otherwise: edge(otherwise),
        };
        for block in [taken, otherwise] {
            let mut result = input;
            for operator in [BinaryOp::Xor, BinaryOp::Add] {
                result = evaluate(
                    &mut graph,
                    block,
                    Type::I32,
                    Expression::Binary {
                        operator,
                        left: result,
                        right: 3,
                    },
                );
            }
            graph.blocks[block.0].exit = Exit::Return(vec![result]);
        }
        expected.extend_from_slice(&graph.blocks[taken.0].items);
        expected.push(BlockItem::Evaluate(condition));
        let replacements = share(&mut graph, None).unwrap();
        assert_eq!(replacements[input], input);
        assert_eq!(replacements[argument], argument);
        assert!(graph.blocks[0].items == expected);
        for block in [taken, otherwise] {
            assert!(graph.blocks[block.0].items.is_empty());
        }
        let expected_values = graph.values.len() - 2;
        graph.replace_values(replacements);
        let count = graph.values.len();
        assert_eq!(count, expected_values);
        graph.compact(|_| true);
        assert_eq!(graph.values.len(), count);
    }
}

#[test]
fn equivalent_dependencies_need_an_available_shared_definition() {
    let mut graph = graph();
    let branch = graph.block(0, &[]);
    let first = graph.block(0, &[]);
    let second = graph.block(0, &[]);
    let outside = graph.block(0, &[]);
    let third = graph.block(0, &[]);
    let bypass = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::If {
        condition: 0,
        taken: edge(branch),
        otherwise: edge(outside),
    };
    graph.blocks[branch.0].exit = Exit::If {
        condition: 1,
        taken: edge(first),
        otherwise: edge(second),
    };
    graph.blocks[outside.0].exit = Exit::If {
        condition: 1,
        taken: edge(third),
        otherwise: edge(bypass),
    };
    for block in [first, second] {
        let input = xor(&mut graph, block);
        let result = graph.values.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: BinaryOp::Add,
                left: input,
                right: 2,
            }),
        });
        graph.blocks[block.0]
            .items
            .push(BlockItem::Evaluate(result));
        graph.blocks[block.0].exit = Exit::Return(vec![result]);
    }
    xor(&mut graph, third);
    graph.blocks[bypass.0].exit = Exit::Return(vec![2]);
    // The XOR group has a bypass, while the add group covers both branch arms.
    // Its equivalent operands still have separate definitions inside those arms.
    assert!(share(&mut graph, None).is_none());
    assert!(graph.blocks[branch.0].items.is_empty());
}

#[test]
fn a_backedge_keeps_work_off_iterations_that_bypass_its_uses() {
    let mut graph = graph();
    let header = graph.block(0, &[]);
    let first = graph.block(0, &[]);
    let dispatch = graph.block(0, &[]);
    let second = graph.block(0, &[]);
    graph.blocks[0].exit = Exit::Jump(edge(header));
    graph.blocks[header.0].exit = Exit::If {
        condition: 0,
        taken: edge(first),
        otherwise: edge(dispatch),
    };
    graph.blocks[dispatch.0].exit = Exit::If {
        condition: 1,
        taken: edge(second),
        otherwise: edge(header),
    };
    xor(&mut graph, first);
    xor(&mut graph, second);
    assert!(share(&mut graph, None).is_none());
    assert!(graph.blocks[header.0].items.is_empty());
}
