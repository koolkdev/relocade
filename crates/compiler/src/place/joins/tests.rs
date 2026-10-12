use super::*;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};
use crate::integer::BinaryOp;

mod parameters;
mod predecessors;

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

fn square(graph: &mut FunctionGraph) -> usize {
    expression(
        graph,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Mul),
            left: 1,
            right: 1,
        },
    )
}

fn placed(graph: &mut FunctionGraph, block: BlockId, recipe: usize) -> usize {
    let value = graph.values.push(graph.values[recipe]);
    graph.blocks[block.0].items.push(BlockItem::Evaluate(value));
    value
}

fn joins(graph: &mut FunctionGraph) -> (Joins, ReducedFlow) {
    let reachable = graph.reachable();
    let incoming = predecessors(graph, &reachable);
    let dominators = Dominators::new(0, &successors(graph, &reachable), &incoming);
    let flow = ReducedFlow::new(graph, &dominators, &incoming);
    let reachable = graph.reachable();
    let incoming = predecessors(graph, &reachable);
    let dominators = Dominators::new(0, &successors(graph, &reachable), &incoming);
    (Joins::new(graph, &incoming, dominators), flow)
}

#[test]
fn a_descendant_demand_adds_one_parameter_at_the_owning_join() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let uses = Diamond::new(&mut graph, arms.join);
    let inactive = graph.block(0, &[]);
    graph.blocks[inactive.0].exit = Exit::Jump(edge(arms.join));
    let recipe = square(&mut graph);
    let unused = expression(
        &mut graph,
        Type::I8,
        Expression::Unary {
            operator: Unary::Convert,
            input: recipe,
        },
    );
    let (mut joins, mut flow) = joins(&mut graph);
    let left = placed(&mut graph, arms.left, recipe);
    let right = placed(&mut graph, arms.right, recipe);
    for (block, value) in [(arms.left, left), (arms.right, right)] {
        joins.record(block.0, [(recipe, value), (unused, value)].into_iter());
        joins.complete(block.0);
    }
    joins.prepare(
        &graph,
        &graph.reachable(),
        arms.join.0,
        &mut Availability::default(),
    );
    assert!(graph.blocks[arms.join.0].parameters.is_empty());
    let result = joins
        .resolve(&mut graph, uses.left.0, recipe, &mut flow)
        .unwrap();
    assert!(matches!(
        graph.values[result].definition,
        ValueDefinition::Parameter { block, component: 0 } if block == arms.join
    ));
    assert_eq!(
        joins.resolve(&mut graph, uses.right.0, recipe, &mut flow),
        Some(result)
    );
    assert_eq!(graph.blocks[arms.join.0].parameters, [result]);
    assert!(graph.blocks[uses.left.0].parameters.is_empty());
    for (block, argument) in [(arms.left, left), (arms.right, right)] {
        assert_eq!(graph.blocks[block.0].exit.edges()[0].arguments, [argument]);
    }
    let filler = graph.blocks[inactive.0].exit.edges()[0].arguments[0];
    assert!(matches!(graph.values[filler].scalar_literal(), Some(0)));
}

#[test]
fn an_indexed_join_cannot_supply_a_sibling_branch() {
    let mut graph = graph();
    let outer = Diamond::new(&mut graph, BlockId(0));
    let inner = Diamond::new(&mut graph, outer.left);
    let independent = graph.values.intern(Value {
        ty: Type::I1,
        definition: ValueDefinition::Expression(Expression::Unary {
            operator: Unary::ZeroTest { nonzero: true },
            input: 1,
        }),
    });
    if let Exit::If { condition, .. } = &mut graph.blocks[outer.left.0].exit {
        *condition = independent;
    }
    graph.blocks[inner.join.0].exit = Exit::Jump(edge(outer.join));
    let recipe = square(&mut graph);
    let (mut joins, mut flow) = joins(&mut graph);
    for block in [inner.left, inner.right] {
        let value = placed(&mut graph, block, recipe);
        joins.record(block.0, [(recipe, value)].into_iter());
        joins.complete(block.0);
    }
    joins.prepare(
        &graph,
        &graph.reachable(),
        inner.join.0,
        &mut Availability::default(),
    );
    assert!(joins
        .resolve(&mut graph, inner.join.0, recipe, &mut flow)
        .is_some());
    assert_eq!(
        joins.resolve(&mut graph, outer.right.0, recipe, &mut flow),
        None
    );
    assert_eq!(
        joins.resolve(&mut graph, outer.join.0, recipe, &mut flow),
        None
    );
}

#[test]
fn a_nearer_join_is_preferred_but_a_failed_merge_keeps_ancestor_reuse() {
    let mut graph = graph();
    let outer = Diamond::new(&mut graph, BlockId(0));
    let inner = Diamond::new(&mut graph, outer.join);
    let nearby = square(&mut graph);
    let one = graph.values.literal(Type::I32, 1);
    let fallback = expression(
        &mut graph,
        Type::I32,
        Expression::Binary {
            operator: Binary::Integer(BinaryOp::Add),
            left: 1,
            right: one,
        },
    );
    let (mut joins, mut flow) = joins(&mut graph);
    for arms in [&outer, &inner] {
        for block in [arms.left, arms.right] {
            let value = placed(&mut graph, block, nearby);
            joins.record(block.0, [(nearby, value)].into_iter());
            if block != inner.right {
                let value = placed(&mut graph, block, fallback);
                joins.record(block.0, [(fallback, value)].into_iter());
            }
            joins.complete(block.0);
        }
        joins.prepare(
            &graph,
            &graph.reachable(),
            arms.join.0,
            &mut Availability::default(),
        );
    }
    let near_result = joins
        .resolve(&mut graph, inner.join.0, nearby, &mut flow)
        .unwrap();
    assert_eq!(graph.blocks[inner.join.0].parameters, [near_result]);
    assert!(graph.blocks[outer.join.0].parameters.is_empty());
    let ancestor_result = joins
        .resolve(&mut graph, inner.join.0, fallback, &mut flow)
        .unwrap();
    assert_eq!(
        joins.resolve(&mut graph, inner.join.0, fallback, &mut flow),
        Some(ancestor_result)
    );
    assert_eq!(graph.blocks[outer.join.0].parameters, [ancestor_result]);
    assert_eq!(graph.blocks[inner.join.0].parameters, [near_result]);
}

#[test]
fn incoming_aliases_keep_their_predecessor_scope() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let uses = Diamond::new(&mut graph, arms.join);
    let number = square(&mut graph);
    let choice = expression(
        &mut graph,
        Type::I32,
        Expression::Select {
            condition: 0,
            when_true: 1,
            when_false: number,
        },
    );
    let alias = expression(
        &mut graph,
        Type::I8,
        Expression::Unary {
            operator: Unary::Convert,
            input: choice,
        },
    );
    let (mut joins, mut flow) = joins(&mut graph);
    let ancestor = placed(&mut graph, BlockId(0), number);
    let child = placed(&mut graph, uses.left, number);
    joins.record(0, [(number, ancestor)].into_iter());
    joins.record(arms.left.0, [(alias, 1)].into_iter());
    for block in [arms.left, arms.right] {
        joins.complete(block.0);
    }
    let mut available = Availability::default();
    available.bind(number, ancestor);
    joins.prepare(&graph, &graph.reachable(), arms.join.0, &mut available);
    joins.record(uses.left.0, [(number, child)].into_iter());
    let result = joins
        .resolve(&mut graph, uses.left.0, alias, &mut flow)
        .unwrap();
    assert_eq!(graph.values[result].ty, Type::I8);
    // Logical narrowing keeps the physical i32 bits, including the upper bits.
    assert_eq!(graph.values.bounds[result].unsigned, 32);
    assert_eq!(graph.blocks[arms.left.0].exit.edges()[0].arguments, [1]);
    assert_eq!(
        graph.blocks[arms.right.0].exit.edges()[0].arguments,
        [ancestor]
    );
    assert_eq!(
        joins.resolve(&mut graph, uses.right.0, alias, &mut flow),
        Some(result)
    );
}

#[test]
fn identical_incoming_carriers_can_reuse_a_value_with_a_different_logical_type() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let byte = expression(
        &mut graph,
        Type::I8,
        Expression::Unary {
            operator: Unary::Convert,
            input: 1,
        },
    );
    let (mut joins, mut flow) = joins(&mut graph);
    joins.record(arms.left.0, [(byte, 1)].into_iter());
    for source in [arms.left, arms.right] {
        joins.complete(source.0);
    }
    joins.prepare(
        &graph,
        &graph.reachable(),
        arms.join.0,
        &mut Availability::default(),
    );
    let result = joins
        .resolve(&mut graph, arms.join.0, byte, &mut flow)
        .unwrap();
    assert_eq!(result, 1);
    assert_eq!(graph.values[result].ty, Type::I32);
    assert_eq!(graph.values[byte].ty, Type::I8);
    assert!(graph.blocks[arms.join.0].parameters.is_empty());
    let reducer = Reducer::from_facts(ScalarFacts::default().with_assumptions(
        &graph.values,
        [Assumption::Bits {
            value: result,
            mask: u32::MAX.into(),
            bits: 0x1234,
        }],
    ));
    assert_eq!(reducer.facts().constant(&graph.values, byte), Some(0x34));
    assert_eq!(
        reducer.facts().constant(&graph.values, result),
        Some(0x1234)
    );
}

#[test]
fn guarded_join_reuse_stays_with_matching_facts() {
    let mut graph = graph();
    let arms = Diamond::new(&mut graph, BlockId(0));
    let uses = Diamond::new(&mut graph, arms.join);
    let (mut joins, mut flow) = joins(&mut graph);
    // Specialization can introduce this recipe after the join index was created.
    let recipe = square(&mut graph);
    let value = placed(&mut graph, arms.left, recipe);
    joins.record(arms.left.0, [(recipe, value)].into_iter());
    for block in [arms.left, arms.right] {
        joins.complete(block.0);
    }
    joins.prepare(
        &graph,
        &graph.reachable(),
        arms.join.0,
        &mut Availability::default(),
    );
    assert_eq!(
        joins.resolve(&mut graph, arms.join.0, recipe, &mut flow),
        None
    );
    let parameter = joins
        .resolve(&mut graph, uses.left.0, recipe, &mut flow)
        .unwrap();
    assert_eq!(graph.blocks[arms.join.0].parameters, [parameter]);
    assert_eq!(
        joins.resolve(&mut graph, uses.left.0, recipe, &mut flow),
        Some(parameter)
    );
    assert_eq!(
        joins.resolve(&mut graph, uses.right.0, recipe, &mut flow),
        None
    );
    assert_eq!(
        joins.resolve(&mut graph, arms.join.0, recipe, &mut flow),
        None
    );
}
