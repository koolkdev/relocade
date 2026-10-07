use super::*;
use crate::{
    body::{Edge, Value, ValueDefinition},
    Type,
};

fn jump(target: BlockId) -> Exit {
    Exit::Jump(Edge {
        target,
        arguments: Vec::new(),
    })
}

fn condition(graph: &mut FunctionGraph) -> usize {
    let value = graph.values.push(Value {
        ty: Type::I1,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 0,
        },
    });
    graph.blocks[0].parameters.push(value);
    value
}

#[test]
fn resolved_conditionals_and_switches_keep_only_the_surviving_layout() {
    for (ty, bits, arm) in [
        (Type::I1, 1, 0),
        (Type::I1, 0, 1),
        (Type::I32, 5, 0),
        (Type::I32, 7, 1),
        (Type::I32, 99, 2),
    ] {
        let mut graph = FunctionGraph::new();
        let arms = std::array::from_fn::<_, 3, _>(|_| graph.block(0, &[]));
        let join = graph.block(0, &[]);
        for &block in &arms {
            graph.blocks[block.0].exit = jump(join);
        }
        graph.blocks[join.0].exit = Exit::Return(Vec::new());
        let selector = graph.values.constant(ty, bits);
        let edge = |index: usize| Edge {
            target: arms[index],
            arguments: Vec::new(),
        };
        let layout = if ty == Type::I1 {
            graph.blocks[0].exit = Exit::If {
                condition: selector,
                taken: edge(0),
                otherwise: edge(1),
            };
            Layout::If {
                branch: graph.entry,
                taken: vec![Layout::Block(arms[0])],
                otherwise: vec![Layout::Block(arms[1])],
                join,
            }
        } else {
            graph.blocks[0].exit = Exit::Switch {
                selector,
                cases: vec![(5, edge(0)), (7, edge(1))],
                default: edge(2),
            };
            Layout::Switch {
                branch: graph.entry,
                cases: vec![
                    (5, vec![Layout::Block(arms[0])]),
                    (7, vec![Layout::Block(arms[1])]),
                ],
                default: vec![Layout::Block(arms[2])],
                join,
            }
        };
        graph.layout = vec![layout, Layout::Block(join)];

        graph.compact(|_| true);

        assert!(matches!(&graph.blocks[0].exit, Exit::Jump(edge) if edge.target == arms[arm]));
        assert!(
            matches!(graph.layout.as_slice(), [Layout::Block(first), Layout::Block(body), Layout::Block(last)]
            if *first == graph.entry && *body == arms[arm] && *last == join)
        );
    }
}

#[test]
fn scopes_keep_early_branch_labels_and_flatten_a_final_entrance() {
    for early in [false, true] {
        let mut graph = FunctionGraph::new();
        let input = if early {
            condition(&mut graph)
        } else {
            graph.values.constant(Type::I1, 0)
        };
        let branch = graph.block(0, &[]);
        let taken = graph.block(0, &[]);
        let otherwise = graph.block(0, &[]);
        let join = graph.block(0, &[]);
        let after = graph.block(0, &[]);
        graph.blocks[0].exit = jump(branch);
        graph.blocks[branch.0].exit = Exit::If {
            condition: input,
            taken: Edge {
                target: taken,
                arguments: Vec::new(),
            },
            otherwise: Edge {
                target: otherwise,
                arguments: Vec::new(),
            },
        };
        graph.blocks[taken.0].exit = jump(after);
        graph.blocks[otherwise.0].exit = jump(join);
        graph.blocks[join.0].exit = jump(after);
        graph.blocks[after.0].exit = Exit::Return(Vec::new());
        graph.layout = vec![
            Layout::Scope {
                preheader: graph.entry,
                body: vec![
                    Layout::If {
                        branch,
                        taken: vec![Layout::Block(taken)],
                        otherwise: vec![Layout::Block(otherwise)],
                        join,
                    },
                    Layout::Block(join),
                ],
                after,
            },
            Layout::Block(after),
        ];

        graph.compact(|_| true);

        if early {
            assert!(matches!(graph.layout[0], Layout::Scope { .. }));
            assert!(matches!(graph.blocks[branch.0].exit, Exit::If { .. }));
        } else {
            let blocks: Vec<_> = graph
                .layout
                .iter()
                .map(|item| {
                    let Layout::Block(block) = item else {
                        panic!("all remaining edges fall through")
                    };
                    *block
                })
                .collect();
            assert_eq!(blocks, [graph.entry, branch, otherwise, join, after]);
        }
    }
}
