use super::*;
use crate::expression::BinaryOp as Binary;
use crate::{bitwise::BitwiseOp, Type};

mod exclusions;

#[test]
fn rewritten_if_and_switch_guards_observe_both_semantic_forms() {
    use crate::expression::UnaryOp as Unary;
    use crate::integer::BinaryOp;

    for switch in [false, true] {
        let mut graph = graph();
        let outer = graph.block(0, &[]);
        let taken = graph.block(0, &[]);
        let otherwise = graph.block(0, &[]);
        let other = parameter(&mut graph, BlockId(0), Type::I32);
        let zero = graph.values.literal(Type::I32, 0);
        let one = graph.values.literal(Type::I32, 1);
        let choice = graph.values.intern(Value {
            ty: Type::I32,
            definition: ValueDefinition::Expression(Expression::Select {
                condition: 0,
                when_true: 1,
                when_false: other,
            }),
        });
        let [authored_sum, reduced_sum] = [choice, 1].map(|input| {
            graph.values.intern(Value {
                ty: Type::I32,
                definition: ValueDefinition::Expression(Expression::Binary {
                    operator: Binary::Integer(BinaryOp::Add),
                    left: input,
                    right: one,
                }),
            })
        });
        graph.blocks[0].exit = Exit::If {
            condition: 0,
            taken: edge(outer),
            otherwise: edge(otherwise),
        };
        graph.blocks[outer.0].exit = if switch {
            Exit::Switch {
                selector: authored_sum,
                cases: vec![(0, edge(taken))],
                default: edge(otherwise),
            }
        } else {
            let condition = graph.values.intern(Value {
                ty: Type::I1,
                definition: ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::ZeroTest { nonzero: false },
                    input: authored_sum,
                }),
            });
            Exit::If {
                condition,
                taken: edge(taken),
                otherwise: edge(otherwise),
            }
        };
        graph.blocks[taken.0].exit = Exit::Return(vec![authored_sum, reduced_sum]);
        graph.blocks[otherwise.0].exit = Exit::Return(vec![zero, zero]);
        let reachable = graph.reachable();
        let predecessors = predecessors(&graph, &reachable);
        let dominators = Dominators::new(0, &successors(&graph, &reachable), &predecessors);

        let flow = ReducedFlow::new(&mut graph, &dominators, &predecessors);

        assert_eq!(
            flow.facts(taken).constant(&graph.values, authored_sum),
            Some(0)
        );
        assert_eq!(
            flow.facts(taken).constant(&graph.values, reduced_sum),
            Some(0)
        );
        let Exit::Return(values) = &graph.blocks[taken.0].exit else {
            unreachable!()
        };
        assert!(values
            .iter()
            .all(|&value| graph.values[value].scalar_literal() == Some(0)));
        assert!(graph.blocks.iter().all(|block| block.items.is_empty()));
    }
}

fn graph() -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    for (component, ty) in [Type::I1, Type::I32].into_iter().enumerate() {
        let parameter = graph.values.push_with_bounds(
            Value {
                ty,
                definition: ValueDefinition::Parameter {
                    block: graph.entry,
                    component,
                },
            },
            BitBounds {
                unsigned: ty.bits(),
                signed: ty.bits().saturating_add(1).min(ty.carrier().bits()),
            },
        );
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

struct Diamond {
    left: BlockId,
    right: BlockId,
    join: BlockId,
}

impl Diamond {
    fn new(graph: &mut FunctionGraph, entry: BlockId) -> Self {
        let left = graph.block(0, &[]);
        let right = graph.block(0, &[]);
        let join = graph.block(0, &[]);
        graph.blocks[entry.0].exit = Exit::If {
            condition: 0,
            taken: edge(left),
            otherwise: edge(right),
        };
        graph.blocks[left.0].exit = Exit::Jump(edge(join));
        graph.blocks[right.0].exit = Exit::Jump(edge(join));
        graph.blocks[join.0].exit = Exit::Return(vec![1]);
        Self { left, right, join }
    }
}

fn expression(graph: &mut FunctionGraph, ty: Type, expression: Expression<usize>) -> usize {
    graph.values.push(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

fn joined(
    graph: &FunctionGraph,
    block: usize,
    contexts: &[Option<Reducer>],
) -> Option<ScalarFacts> {
    let sources = predecessors(graph, &graph.reachable());
    incoming_facts(graph, block, &sources[block], contexts)
}

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
fn reduced_edge_values_establish_join_facts_before_any_execution_is_placed() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let result = parameter(&mut graph, arms.join, Type::I32);
    let five = graph.values.literal(Type::I32, 5);
    let seven = graph.values.literal(Type::I32, 7);
    for (source, when_true, when_false) in [(arms.left, five, seven), (arms.right, seven, five)] {
        let choice = expression(
            &mut graph,
            Type::I32,
            Expression::Select {
                condition: 0,
                when_true,
                when_false,
            },
        );
        graph.blocks[source.0].exit.edges_mut()[0]
            .arguments
            .push(choice);
    }
    graph.blocks[arms.join.0].exit = Exit::Return(vec![result]);
    let reachable = graph.reachable();
    let predecessors = predecessors(&graph, &reachable);
    let dominators = Dominators::new(0, &successors(&graph, &reachable), &predecessors);
    let flow = ReducedFlow::new(&mut graph, &dominators, &predecessors);
    for block in [arms.left, arms.right] {
        assert_eq!(graph.blocks[block.0].exit.edges()[0].arguments, [five]);
    }
    assert!(matches!(&graph.blocks[arms.join.0].exit, Exit::Return(values) if values == &[five]));
    assert_eq!(
        flow.facts(arms.join).constant(&graph.values, result),
        Some(5)
    );
    assert!(graph.blocks.iter().all(|block| block.items.is_empty()));
}

#[test]
fn a_join_keeps_common_facts_without_any_value_candidates() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let one = graph.values.literal(Type::I32, 1);
    let low_bit = expression(
        &mut graph,
        Type::I32,
        Expression::Binary {
            operator: Binary::Bitwise(BitwiseOp::And),
            left: 1,
            right: one,
        },
    );
    let mut contexts: Vec<Option<Reducer>> = (0..graph.blocks.len()).map(|_| None).collect();
    for (block, value) in [(arms.left, 5), (arms.right, 7)] {
        let facts = ScalarFacts::default().with_assumptions(
            &graph.values,
            [facts::Assumption::Bits {
                value: 1,
                mask: 0xffff_ffff,
                bits: value,
            }],
        );
        contexts[block.0] = Some(Reducer::from_facts(facts));
    }
    let facts = joined(&graph, arms.join.0, &contexts).unwrap();
    assert_eq!(facts.constant(&graph.values, low_bit), Some(1));
    assert_eq!(facts.constant(&graph.values, 1), None);
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
    let mut contexts: Vec<Option<Reducer>> = (0..graph.blocks.len()).map(|_| None).collect();
    for source in [arms.left, arms.right] {
        let facts = ScalarFacts::default().with_assumptions(
            &graph.values,
            [facts::Assumption::Bits {
                value: 1,
                mask: 0xffff_ffff,
                bits: 5,
            }],
        );
        contexts[source.0] = Some(Reducer::from_facts(facts));
    }
    contexts[bypass.0] = Some(Reducer::from_facts(ScalarFacts::default()));
    let facts = joined(&graph, arms.join.0, &contexts).unwrap();
    assert_eq!(facts.constant(&graph.values, 1), None);
}

#[test]
fn argument_facts_transfer_common_bits_without_value_candidates() {
    for (right_value, expected_value, expected_low_bits) in [
        (Some(0x2a5), Some(0xa5), Some(5)),
        (Some(0x2b5), None, Some(5)),
        (None, None, None),
    ] {
        let mut graph = graph();
        let right_argument = parameter(&mut graph, BlockId(0), Type::I32);
        let arms = Diamond::new(&mut graph, BlockId(0));
        let result = parameter(&mut graph, arms.join, Type::I8);
        for (block, argument) in [(arms.left, 1), (arms.right, right_argument)] {
            graph.blocks[block.0].exit.edges_mut()[0]
                .arguments
                .push(argument);
        }
        let mask = graph.values.literal(Type::I8, 15);
        let low = expression(
            &mut graph,
            Type::I8,
            Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::And),
                left: result,
                right: mask,
            },
        );
        let mut contexts: Vec<Option<Reducer>> = (0..graph.blocks.len()).map(|_| None).collect();
        for (source, argument, value) in [
            (arms.left, 1, Some(0x1a5)),
            (arms.right, right_argument, right_value),
        ] {
            let mut facts = ScalarFacts::default();
            if let Some(value) = value {
                facts = facts.with_assumptions(
                    &graph.values,
                    [facts::Assumption::Bits {
                        value: argument,
                        mask: u32::MAX.into(),
                        bits: value,
                    }],
                );
            }
            contexts[source.0] = Some(Reducer::from_facts(facts));
        }
        let facts = joined(&graph, arms.join.0, &contexts).unwrap();
        assert_eq!(facts.constant(&graph.values, low), expected_low_bits);
        assert_eq!(facts.constant(&graph.values, result), expected_value);
    }
}

#[test]
fn two_edges_from_one_predecessor_must_agree_unless_one_is_discarded() {
    for discard in [false, true] {
        let mut graph = graph();
        let arms = Diamond::new(&mut graph, BlockId(0));
        let result = parameter(&mut graph, arms.join, Type::I32);
        let five = graph.values.literal(Type::I32, 5);
        let seven = graph.values.literal(Type::I32, 7);
        let incoming = |value| Edge {
            target: arms.join,
            arguments: vec![value],
        };
        graph.blocks[arms.left.0].exit = Exit::If {
            condition: 0,
            taken: incoming(five),
            otherwise: incoming(seven),
        };
        graph.blocks[arms.right.0].exit = Exit::Jump(incoming(five));
        let mut contexts: Vec<Option<Reducer>> = (0..graph.blocks.len()).map(|_| None).collect();
        for source in [arms.left, arms.right] {
            contexts[source.0] = Some(Reducer::from_facts(ScalarFacts::default()));
        }
        if discard {
            let yes = graph.values.literal(Type::I1, 1);
            if let Exit::If { condition, .. } = &mut graph.blocks[arms.left.0].exit {
                *condition = yes;
            }
        }
        let facts = joined(&graph, arms.join.0, &contexts).unwrap();
        assert_eq!(facts.constant(&graph.values, result), discard.then_some(5));
    }
}

#[test]
fn a_loop_parameter_does_not_inherit_its_initial_constant() {
    let mut graph = graph();
    let header = graph.block(0, &[Type::I32]);
    let result = graph.blocks[header.0].parameters[0];
    let one = graph.values.literal(Type::I32, 1);
    graph.blocks[0].exit = Exit::Jump(Edge {
        target: header,
        arguments: vec![one],
    });
    graph.blocks[header.0].exit = Exit::If {
        condition: 0,
        taken: Edge {
            target: header,
            arguments: vec![result],
        },
        otherwise: Edge {
            target: header,
            arguments: vec![1],
        },
    };
    let mut contexts: Vec<Option<Reducer>> = (0..graph.blocks.len()).map(|_| None).collect();
    contexts[0] = Some(Reducer::from_facts(ScalarFacts::default()));
    assert!(joined(&graph, header.0, &contexts).is_none());
}

#[test]
fn a_proven_component_does_not_make_its_sibling_available() {
    let mut graph = graph();
    let input = parameter(&mut graph, BlockId(0), Type::I64);
    let low = expression(
        &mut graph,
        Type::I64,
        Expression::MultiplyWide {
            signed: false,
            left: input,
            right: input,
        },
    );
    let high = graph.values.expression_result(low, 1);
    let facts = ScalarFacts::default().with_assumptions(
        &graph.values,
        [Assumption::Bits {
            value: low,
            mask: u64::MAX,
            bits: 0,
        }],
    );
    let flow = ReducedFlow {
        reducers: vec![Some(Reducer::from_facts(facts))],
        exclusions: FxHashMap::default(),
        control_changed: false,
    };
    let reachable = graph.reachable();
    let predecessors = predecessors(&graph, &reachable);
    let dominators = Dominators::new(0, &successors(&graph, &reachable), &predecessors);
    let joins = Joins::new(&graph, &predecessors, dominators);
    let mut placer = Placer {
        graph: &mut graph,
        reachable,
        flow,
        joins,
        available: Availability::default(),
        shared: vec![Vec::new()],
    };
    let folded = placer.materialize(low, BlockId(0));
    assert_eq!(placer.graph.values[folded].scalar_literal(), Some(0));
    assert!(placer.graph.blocks[0].items.is_empty());
    let placed = placer.materialize(high, BlockId(0));
    assert_eq!(placer.graph.blocks[0].items.len(), 1);
    assert!(placer.graph.producer_of(placed) == Some(placer.graph.blocks[0].items[0]));
    assert_eq!(placer.materialize(high, BlockId(0)), placed);
    assert_eq!(placer.materialize(low, BlockId(0)), folded);
    assert_eq!(placer.graph.blocks[0].items.len(), 1);
}
