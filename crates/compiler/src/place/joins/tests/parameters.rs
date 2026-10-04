//! Authored results inherit only bits agreed by all active incoming edges.
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
        let mask = graph.values.constant(Type::I8, 15);
        let low = expression(
            &mut graph,
            Type::I8,
            Expression::Binary {
                operator: BinaryOp::And,
                left: result,
                right: mask,
            },
        );
        let mut joins = joins(&graph);
        for (source, argument, value) in [
            (arms.left, 1, Some(0x1a5)),
            (arms.right, right_argument, right_value),
        ] {
            let mut facts = Facts::default();
            if let Some(value) = value {
                facts.assume_bits(argument, u32::MAX.into(), value);
            }
            joins.complete(source.0, &facts);
        }
        let facts = joins
            .prepare(&graph, arms.join.0, &Availability::default())
            .unwrap();
        assert_eq!(facts.constant(&graph.values, low), expected_low_bits);
        assert_eq!(facts.constant(&graph.values, result), expected_value);
        assert!(joins.inputs[arms.join.0].is_none());
    }
}

#[test]
fn two_edges_from_one_predecessor_must_agree_unless_one_is_discarded() {
    for discard in [false, true] {
        let mut graph = graph();
        let arms = Diamond::new(&mut graph, BlockId(0));
        let result = parameter(&mut graph, arms.join, Type::I32);
        let five = graph.values.constant(Type::I32, 5);
        let seven = graph.values.constant(Type::I32, 7);
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
        let mut joins = joins(&graph);
        for source in [arms.left, arms.right] {
            joins.complete(source.0, &Facts::default());
        }
        if discard {
            let yes = graph.values.constant(Type::I1, 1);
            if let Exit::If { condition, .. } = &mut graph.blocks[arms.left.0].exit {
                *condition = yes;
            }
        }
        let facts = joins
            .prepare(&graph, arms.join.0, &Availability::default())
            .unwrap();
        assert_eq!(facts.constant(&graph.values, result), discard.then_some(5));
    }
}

#[test]
fn a_loop_parameter_does_not_inherit_its_initial_constant() {
    let mut graph = graph();
    let header = graph.block(0, &[Type::I32]);
    let result = graph.blocks[header.0].parameters[0];
    let one = graph.values.constant(Type::I32, 1);
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
    let mut joins = joins(&graph);
    joins.complete(0, &Facts::default());
    assert!(joins
        .prepare(&graph, header.0, &Availability::default())
        .is_none());
}
