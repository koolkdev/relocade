use super::*;
use crate::Type;

fn expression(graph: &mut FunctionGraph, ty: Type, expression: Expression<usize>) -> usize {
    graph.values.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

fn round_trip() -> (FunctionGraph, usize, usize) {
    let mut graph = FunctionGraph::new();
    let input = graph.values.push(Value {
        ty: Type::V128,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 0,
        },
    });
    let low = expression(
        &mut graph,
        Type::I64,
        Expression::VectorExtract { input, lane: 0 },
    );
    let zero = graph.values.literal(Type::V128, 0_u128);
    let restored = expression(
        &mut graph,
        Type::V128,
        Expression::VectorReplace {
            vector: zero,
            value: low,
            lane: 0,
        },
    );
    (graph, input, restored)
}

#[test]
fn a_partial_residual_never_becomes_a_complete_alias_or_answers_a_full_observation() {
    let (mut graph, input, restored) = round_trip();
    let mut demands = vec![0; graph.values.len()];
    demands[restored] = u128::from(u64::MAX);
    let mut specializer = Specializer::new(demands);
    let mut available = Availability::default();
    let partial = specializer.specialize(&mut graph, restored, |_, _, _| None);
    assert_eq!(partial.value, input);
    available.record_aliases(partial.aliases);
    available.bind(input, input);
    assert_eq!(available.get(restored), None);
    assert!(available
        .sources(&graph.values, input, u128::MAX)
        .iter()
        .all(|source| source.recipe != restored));

    // A later full consumer must not reuse the low-half memo or its bindings.
    let mask = graph.values.literal(Type::V128, 1_u128 << 127);
    let full = expression(
        &mut graph,
        Type::V128,
        Expression::Bitwise {
            operator: BitwiseOp::Xor,
            left: restored,
            right: mask,
        },
    );
    let complete = specializer.specialize(&mut graph, full, |graph, id, _| {
        available.lookup(&graph.values, id)
    });
    let ValueDefinition::Expression(Expression::Bitwise { left, .. }) =
        graph.values[complete.value].definition
    else {
        panic!("the full vector still clears its upper half")
    };
    assert_ne!(left, input);
    assert!(matches!(
        graph.values[left].definition,
        ValueDefinition::Expression(Expression::VectorReplace { .. })
    ));
}

#[test]
fn complete_availability_wins_over_an_earlier_partial_residual() {
    let (mut graph, input, restored) = round_trip();
    let mut demands = vec![0; graph.values.len()];
    demands[restored] = u128::from(u64::MAX);
    let mut specializer = Specializer::new(demands);
    assert_eq!(
        specializer
            .specialize(&mut graph, restored, |_, _, _| None)
            .value,
        input
    );
    let executed = graph.values.push(graph.values[restored]);
    assert_eq!(
        specializer
            .specialize(&mut graph, restored, |_, id, _| (id == restored)
                .then_some(executed))
            .value,
        executed
    );
}
