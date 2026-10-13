use super::super::*;
use crate::{expression::UnaryOp as Unary, integer::BitCountOp, Type};

fn byte_input() -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    let input = graph.values.push_with_bounds(
        Value {
            ty: Type::I8,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component: 0,
            },
        },
        BitBounds {
            unsigned: 8,
            signed: 9,
        },
    );
    graph.blocks[0].parameters.push(input);
    graph
}

fn count(values: &mut ValueTable, operator: BitCountOp, input: usize) -> usize {
    crate::expression::build(
        values,
        Type::I8,
        Expression::Unary {
            operator: Unary::BitCount {
                operator,
                input_type: Type::I8,
            },
            input,
        },
        0,
    )
}

fn flow(graph: &mut FunctionGraph) -> ReducedFlow {
    let reachable = graph.reachable();
    let predecessors = predecessors(graph, &reachable);
    let dominators = Dominators::new(0, &successors(graph, &reachable), &predecessors);
    ReducedFlow::new(graph, &dominators, &predecessors)
}

fn assert_primitive(values: &ValueTable, root: usize) {
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if let Some(result) = values.expression(id) {
            assert!(
                !matches!(
                    result.expression,
                    Expression::Unary {
                        operator: Unary::BitCount {
                            operator: BitCountOp::LeadingZeros | BitCountOp::TrailingZeros,
                            input_type: Type::I1 | Type::I8 | Type::I16,
                        },
                        ..
                    }
                ),
                "compound value {id} survives"
            );
            pending.extend(result.expression.inputs());
        }
    }
}

#[test]
fn late_semantic_recipes_use_the_same_lowering_as_initial_roots() {
    let mut graph = byte_input();
    let leading = count(&mut graph.values, BitCountOp::LeadingZeros, 0);
    let trailing = count(&mut graph.values, BitCountOp::TrailingZeros, leading);
    graph.blocks[0].exit = Exit::Return(vec![trailing]);
    let mut flow = flow(&mut graph);
    let lowered_input = flow.reduce(&mut graph.values, BlockId(0), leading);
    // Another path can specialize an expression to the primitive input created
    // by initial lowering. Its newly interned semantic recipe must lower too.
    let late = count(&mut graph.values, BitCountOp::TrailingZeros, lowered_input);
    let result = flow.reduce(&mut graph.values, BlockId(0), late);
    assert_primitive(&graph.values, result);
    assert_eq!(flow.reduce(&mut graph.values, BlockId(0), late), result);
}

#[test]
fn refolded_counts_consult_semantic_facts_before_lowering() {
    let mut graph = byte_input();
    let condition = graph.values.push_with_bounds(
        Value {
            ty: Type::I1,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component: 1,
            },
        },
        BitBounds {
            unsigned: 1,
            signed: 2,
        },
    );
    let zero = graph.values.literal(Type::I8, 0);
    let choice = graph.values.intern(Value {
        ty: Type::I8,
        definition: ValueDefinition::Expression(Expression::Ternary {
            operator: crate::expression::TernaryOp::Select,
            first: 0,
            second: zero,
            third: condition,
        }),
    });
    let known = count(&mut graph.values, BitCountOp::LeadingZeros, 0);
    let root = count(&mut graph.values, BitCountOp::LeadingZeros, choice);
    let facts = ScalarFacts::default().with_assumptions(
        &graph.values,
        [
            Assumption::Truth {
                condition,
                truth: true,
            },
            Assumption::Bits {
                value: known,
                mask: 0xff,
                bits: 0,
            },
        ],
    );
    let mut reducer = Reducer::from_facts(facts);
    assert_eq!(reducer.reduce(&mut graph.values, root), zero);
    assert!(reducer.recipes(zero).any(|recipe| recipe == root));
}

#[test]
fn primitive_bounds_keep_the_count_range_through_representation_views() {
    let mut graph = byte_input();
    let leading = count(&mut graph.values, BitCountOp::LeadingZeros, 0);
    let trailing = count(&mut graph.values, BitCountOp::TrailingZeros, 0);
    graph.blocks[0].exit = Exit::Return(vec![leading, trailing]);
    let mut flow = flow(&mut graph);
    for original in [leading, trailing] {
        let lowered = flow.reduce(&mut graph.values, BlockId(0), original);
        let primitive = graph.values.representation(lowered);
        assert_eq!(graph.values.bounds[primitive].unsigned, 4);
        assert_eq!(graph.values.bounds[primitive].signed, 5);
        assert!(flow
            .recipes(BlockId(0), lowered)
            .any(|recipe| recipe == original));
    }
}

#[test]
fn lowering_does_not_publish_a_path_local_count_bound() {
    let mut graph = byte_input();
    // A path can remove the normalization because of a guard. That proof is
    // not a property of the input's carrier or of a shared native adjustment.
    graph.values.bounds[0] = BitBounds {
        unsigned: 32,
        signed: 32,
    };
    let count = graph.values.intern(Value {
        ty: Type::I8,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::BitCount {
                operator: BitCountOp::LeadingZeros,
                input_type: Type::I8,
            },
            input: 0,
        }),
    });
    graph.blocks[0].exit = Exit::Return(vec![count]);
    let mut flow = flow(&mut graph);
    let lowered = flow.reduce(&mut graph.values, BlockId(0), count);
    assert_eq!(graph.values.bounds[lowered].unsigned, 32);
}

#[test]
fn unused_compounds_do_not_activate_lowering_for_scalar_roots() {
    let mut graph = byte_input();
    count(&mut graph.values, BitCountOp::LeadingZeros, 0);
    graph.blocks[0].exit = Exit::Return(vec![0]);
    let before = graph.values.len();
    let mut flow = flow(&mut graph);
    assert_eq!(flow.reduce(&mut graph.values, BlockId(0), 0), 0);
    assert_eq!(graph.values.len(), before);
}

#[test]
fn unused_compounds_stay_unexpanded_alongside_a_live_count() {
    let mut graph = byte_input();
    let leading = count(&mut graph.values, BitCountOp::LeadingZeros, 0);
    count(&mut graph.values, BitCountOp::TrailingZeros, 0);
    graph.blocks[0].exit = Exit::Return(vec![leading]);
    let before = graph.values.len();
    let mut flow = flow(&mut graph);
    let lowered = flow.reduce(&mut graph.values, BlockId(0), leading);
    assert_primitive(&graph.values, lowered);
    assert!(graph.values[before..].iter().all(|value| !matches!(
        value.definition,
        ValueDefinition::Expression(Expression::Unary {
            operator: Unary::BitCount {
                operator: BitCountOp::TrailingZeros,
                ..
            },
            ..
        })
    )));
}
