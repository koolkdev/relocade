use super::*;
use crate::{integer::BinaryOp, Type};

fn graph() -> FunctionGraph {
    let mut graph = FunctionGraph::new();
    for (component, ty) in [Type::I1, Type::I32].into_iter().enumerate() {
        let value = graph.values.push(Value {
            ty,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component,
            },
        });
        graph.blocks[0].parameters.push(value);
    }
    graph
}

fn expression(graph: &mut FunctionGraph, ty: Type, expression: Expression<usize>) -> usize {
    graph.values.push(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

fn square(graph: &mut FunctionGraph) -> usize {
    expression(
        graph,
        Type::I32,
        Expression::Binary {
            operator: BinaryOp::Mul,
            left: 1,
            right: 1,
        },
    )
}

fn edge(target: BlockId) -> Edge {
    Edge {
        target,
        arguments: Vec::new(),
    }
}

fn branch(
    graph: &mut FunctionGraph,
    from: BlockId,
    condition: usize,
    result: usize,
) -> [BlockId; 2] {
    let taken = graph.block(0, &[]);
    let otherwise = graph.block(0, &[]);
    graph.blocks[from.0].exit = Exit::If {
        condition,
        taken: edge(taken),
        otherwise: edge(otherwise),
    };
    for block in [taken, otherwise] {
        graph.blocks[block.0].exit = Exit::Return(vec![result]);
    }
    [taken, otherwise]
}

fn placer(graph: &mut FunctionGraph) -> Placer<'_> {
    let reachable = graph.reachable();
    let predecessors = predecessors(graph, &reachable);
    let dominators = Dominators::new(0, &successors(graph, &reachable), &predecessors);
    let joins = Joins::new(graph, &predecessors, dominators);
    let shared = vec![Vec::new(); graph.blocks.len()];
    Placer {
        graph,
        specializer: Specializer::default(),
        available: HashMap::new(),
        available_log: Vec::new(),
        expressions: HashMap::new(),
        expression_log: Vec::new(),
        shared,
        joins,
    }
}

#[test]
fn a_branch_can_eliminate_work_without_placing_its_preview() {
    enum Folded {
        Constant,
        Parameter,
        Available,
    }
    for folded in [Folded::Constant, Folded::Parameter, Folded::Available] {
        let mut graph = graph();
        let product = square(&mut graph);
        let constant = graph.values.constant(Type::I32, 17);
        let when_false = match folded {
            Folded::Constant => constant,
            Folded::Parameter => 1,
            Folded::Available => expression(
                &mut graph,
                Type::I32,
                Expression::Binary {
                    operator: BinaryOp::Add,
                    left: 1,
                    right: constant,
                },
            ),
        };
        let choice = expression(
            &mut graph,
            Type::I32,
            Expression::Select {
                condition: 0,
                when_true: product,
                when_false,
            },
        );
        branch(&mut graph, BlockId(0), 0, choice);
        let mut placer = placer(&mut graph);
        if matches!(folded, Folded::Available) {
            placer.materialize(when_false, BlockId(0));
        }
        let before_items = placer.graph.blocks[0].items.len();
        let before_bindings = placer.available.clone();
        let mut scheduled = vec![choice];
        placer.defer_eliminated(BlockId(0), &mut scheduled);
        assert!(scheduled.is_empty());
        assert_eq!(placer.graph.blocks[0].items.len(), before_items);
        assert_eq!(placer.available, before_bindings);
        assert!(placer.graph.blocks[1..]
            .iter()
            .all(|b| b.parameters.is_empty() && b.items.is_empty()));
        assert!(placer.graph.blocks[0]
            .exit
            .edges()
            .iter()
            .all(|e| e.arguments.is_empty()));
    }
}

#[test]
fn the_branch_decision_remains_before_its_own_outcomes() {
    let mut graph = graph();
    let predicate = expression(
        &mut graph,
        Type::I1,
        Expression::ZeroTest {
            input: 1,
            nonzero: true,
        },
    );
    branch(&mut graph, BlockId(0), predicate, predicate);
    let mut placer = placer(&mut graph);
    let mut scheduled = vec![predicate];
    placer.defer_eliminated(BlockId(0), &mut scheduled);
    assert_eq!(scheduled, [predicate]);
}

#[test]
fn arguments_passed_to_successors_remain_before_the_branch() {
    let mut graph = graph();
    let product = square(&mut graph);
    let choice = expression(
        &mut graph,
        Type::I32,
        Expression::Select {
            condition: 0,
            when_true: product,
            when_false: 1,
        },
    );
    let [taken, otherwise] = [0, 1].map(|_| {
        let target = graph.block(0, &[Type::I32]);
        graph.blocks[target.0].exit = Exit::Return(graph.blocks[target.0].parameters.clone());
        Edge {
            target,
            arguments: vec![choice],
        }
    });
    graph.blocks[0].exit = Exit::If {
        condition: 0,
        taken,
        otherwise,
    };
    let mut placer = placer(&mut graph);
    let mut scheduled = vec![choice];
    placer.defer_eliminated(BlockId(0), &mut scheduled);
    assert_eq!(scheduled, [choice]);
}

#[test]
fn retained_calculations_keep_their_dependencies_in_schedule_order() {
    let mut graph = graph();
    let product = square(&mut graph);
    let child = expression(
        &mut graph,
        Type::I32,
        Expression::Select {
            condition: 0,
            when_true: product,
            when_false: 1,
        },
    );
    let alias = expression(&mut graph, Type::I8, Expression::Convert { input: child });
    let parent = expression(
        &mut graph,
        Type::I8,
        Expression::Binary {
            operator: BinaryOp::Mul,
            left: alias,
            right: alias,
        },
    );
    branch(&mut graph, BlockId(0), 0, parent);
    let mut placer = placer(&mut graph);
    let mut scheduled = vec![child, alias, parent];
    placer.defer_eliminated(BlockId(0), &mut scheduled);
    assert_eq!(scheduled, [child, alias, parent]);
}

#[test]
fn sharing_analysis_resolves_ancestor_joins_before_caching_values() {
    let mut graph = graph();
    let product = square(&mut graph);
    let sources = branch(&mut graph, BlockId(0), 0, product);
    let join = graph.block(0, &[]);
    for source in sources {
        graph.blocks[source.0].exit = Exit::Jump(edge(join));
    }
    let uses = branch(&mut graph, join, 0, product);
    branch(&mut graph, uses[0], 0, product);
    let mut placer = placer(&mut graph);
    for source in sources {
        let value = placer.graph.values.push(placer.graph.values[product]);
        placer.graph.blocks[source.0]
            .items
            .push(BlockItem::Evaluate(value));
        placer
            .joins
            .record(source.0, [(product, value)].into_iter());
        placer.joins.complete(source.0, Facts::default());
    }
    placer
        .joins
        .prepare(placer.graph, join.0, &placer.available);
    let mut scheduled = vec![product];
    // A descendant first asks for the value while inspecting its early work.
    // Its specializer must cache the ancestor's joined value, not the recipe.
    placer.defer_eliminated(uses[0], &mut scheduled);
    let parameter = placer.graph.blocks[join.0].parameters[0];
    assert_eq!(placer.materialize(product, uses[0]), parameter);
    assert_eq!(placer.graph.blocks[join.0].parameters, [parameter]);
    assert!(placer.graph.blocks[uses[0].0].items.is_empty());
}
