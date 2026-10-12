use super::*;

fn reduce(graph: &mut FunctionGraph) -> ReducedFlow {
    let reachable = graph.reachable();
    let predecessors = predecessors(graph, &reachable);
    let dominators = Dominators::new(0, &successors(graph, &reachable), &predecessors);
    ReducedFlow::new(graph, &dominators, &predecessors)
}

#[test]
fn repeated_exclusions_distinguish_siblings_and_their_join() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let uses = Diamond::new(&mut graph, arms.join);
    let mut flow = reduce(&mut graph);

    for _ in 0..2 {
        for block in [BlockId(0), arms.join, uses.join] {
            assert!(!flow.excludes(&graph.values, block, arms.left));
            assert!(!flow.excludes(&graph.values, block, arms.right));
        }
        for block in [arms.left, uses.left] {
            assert!(!flow.excludes(&graph.values, block, arms.left));
            assert!(flow.excludes(&graph.values, block, arms.right));
        }
        for block in [arms.right, uses.right] {
            assert!(flow.excludes(&graph.values, block, arms.left));
            assert!(!flow.excludes(&graph.values, block, arms.right));
        }
        // Placement may append a join parameter and another semantic recipe.
        // Existing facts and value definitions retain their meanings.
        parameter(&mut graph, arms.join, Type::I32);
        let one = graph.values.literal(Type::I32, 1);
        expression(
            &mut graph,
            Type::I32,
            Expression::Binary {
                operator: Binary::Bitwise(BitwiseOp::Xor),
                left: 1,
                right: one,
            },
        );
    }
}

#[test]
fn distinct_range_predicates_can_exclude_an_incoming_path() {
    use crate::expression::CompareOp as Compare;
    use crate::integer::CompareOp;

    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let uses = Diamond::new(&mut graph, arms.join);
    for (block, bound, operator) in [
        (BlockId(0), 10, CompareOp::LtUnsigned),
        (arms.join, 20, CompareOp::GeUnsigned),
    ] {
        let bound = graph.values.literal(Type::I32, bound);
        let condition = expression(
            &mut graph,
            Type::I1,
            Expression::Binary {
                operator: Binary::Compare(Compare::Integer(operator)),
                left: 1,
                right: bound,
            },
        );
        if let Exit::If {
            condition: guard, ..
        } = &mut graph.blocks[block.0].exit
        {
            *guard = condition;
        }
    }
    let mut flow = reduce(&mut graph);
    for _ in 0..2 {
        assert!(!flow.excludes(&graph.values, arms.join, arms.left));
        assert!(flow.excludes(&graph.values, uses.left, arms.left));
        assert!(!flow.excludes(&graph.values, uses.right, arms.left));
    }
}

#[test]
fn an_exclusion_miss_does_not_answer_the_reverse_query() {
    use crate::expression::UnaryOp as Unary;
    use crate::integer::BinaryOp;

    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let uses = Diamond::new(&mut graph, arms.join);
    let one = graph.values.literal(Type::I32, 1);
    let sum = expression(
        &mut graph,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: 1,
            right: one,
        },
    );
    for (block, input) in [(BlockId(0), 1), (arms.join, sum)] {
        let condition = expression(
            &mut graph,
            Type::I1,
            Expression::Unary {
                operator: Unary::ZeroTest { nonzero: false },
                input,
            },
        );
        if let Exit::If {
            condition: guard, ..
        } = &mut graph.blocks[block.0].exit
        {
            *guard = condition;
        }
    }
    let mut flow = reduce(&mut graph);
    for _ in 0..2 {
        // x == 0 proves x + 1 != 0. The inference rules do not invert addition
        // to learn x from the second guard, so the reverse query cannot prove it.
        assert!(!flow.excludes(&graph.values, uses.left, arms.left));
        assert!(flow.excludes(&graph.values, arms.left, uses.left));
    }
}
