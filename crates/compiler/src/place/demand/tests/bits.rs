use super::*;
use crate::expression::LaneBinaryOp;

fn parameter(graph: &mut FunctionGraph, ty: Type) -> usize {
    graph.values.push(Value {
        ty,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 0,
        },
    })
}

fn expression(graph: &mut FunctionGraph, ty: Type, expression: Expression<usize>) -> usize {
    graph.values.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

fn demands(graph: &FunctionGraph) -> Demands {
    let reachable = graph.reachable();
    let dominators = Dominators::new(
        0,
        &successors(graph, &reachable),
        &predecessors(graph, &reachable),
    );
    analyze(graph, &reachable, &dominators)
}

#[test]
fn packed_carries_stay_in_their_lane_but_comparisons_require_the_whole_lane() {
    let mut graph = FunctionGraph::new();
    let left = parameter(&mut graph, Type::V128);
    let right = parameter(&mut graph, Type::V128);
    for (operator, expected) in [
        (LaneBinaryOp::Add, 0x001f_0000_u128),
        (LaneBinaryOp::Sub, 0x001f_0000),
        (LaneBinaryOp::Eq, 0xffff_0000),
        (LaneBinaryOp::GtSigned, 0xffff_0000),
    ] {
        let value = expression(
            &mut graph,
            Type::V128,
            Expression::VectorBinary {
                operator,
                lane_type: Type::I16,
                left,
                right,
            },
        );
        let requirements = inputs(&graph.values, value, 1 << 20);
        assert_eq!(
            requirements.inputs().copied().collect::<Vec<_>>(),
            [expected; 2]
        );
    }
}

#[test]
fn scalar_observations_keep_carry_inputs_and_do_not_truncate_carriers_to_logical_widths() {
    let mut graph = FunctionGraph::new();
    let left = parameter(&mut graph, Type::I8);
    let right = parameter(&mut graph, Type::I8);
    let sum = expression(
        &mut graph,
        Type::I8,
        Expression::Binary {
            operator: BinaryOp::Add,
            left,
            right,
        },
    );
    let requirements = inputs(&graph.values, sum, 1 << 16);
    assert_eq!(
        requirements.inputs().copied().collect::<Vec<_>>(),
        [0x1ffff; 2]
    );
    let byte = expression(
        &mut graph,
        Type::I8,
        Expression::LowBits {
            input: sum,
            bits: 8,
        },
    );
    graph.blocks[0].exit = Exit::Return(vec![byte]);
    let partial = demands(&graph);
    assert_eq!(partial.bits[sum], 0xff);
    assert_eq!(partial.bits[left], 0xff);
    graph.blocks[0].exit = Exit::Return(vec![byte, sum]);
    let shared = demands(&graph);
    assert_eq!(shared.bits[sum], u128::from(u32::MAX));
    assert_eq!(shared.bits[left], u128::from(u32::MAX));
}

#[test]
fn independent_result_demands_keep_high_product_dependencies_and_complete_edge_arguments() {
    let mut graph = FunctionGraph::new();
    let left = parameter(&mut graph, Type::I64);
    let right = parameter(&mut graph, Type::I64);
    let product = expression(
        &mut graph,
        Type::I64,
        Expression::MultiplyWide {
            signed: false,
            left,
            right,
        },
    );
    let high = graph.values.expression_result(product, 1);
    let low = expression(
        &mut graph,
        Type::I64,
        Expression::LowBits {
            input: high,
            bits: 8,
        },
    );
    graph.blocks[0].exit = Exit::Return(vec![low]);
    let observed = demands(&graph);
    assert_eq!(observed.bits[product], 0);
    assert_eq!(observed.bits[high], 0xff);
    assert_eq!(observed.bits[left], u128::from(u64::MAX));
    let loop_ = graph.block(0, &[Type::I64]);
    graph.blocks[0].exit = Exit::Jump(Edge {
        target: loop_,
        arguments: vec![high],
    });
    graph.blocks[loop_.0].exit = Exit::Jump(Edge {
        target: loop_,
        arguments: vec![high],
    });
    assert_eq!(demands(&graph).bits[high], u128::from(u64::MAX));
}
