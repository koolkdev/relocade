use super::*;
use crate::{memory::MemoryAccess, Mem, Type};

struct Branches {
    graph: FunctionGraph,
    product: usize,
    taken: BlockId,
    otherwise: BlockId,
}

fn edge(target: BlockId) -> Edge {
    Edge {
        target,
        arguments: Vec::new(),
    }
}

impl Branches {
    fn new() -> Self {
        let mut graph = FunctionGraph::new();
        for (component, ty) in [Type::I1, Type::I32, Type::I1].into_iter().enumerate() {
            let parameter = graph.values.push(Value {
                ty,
                definition: ValueDefinition::Parameter {
                    block: graph.entry,
                    component,
                },
            });
            graph.blocks[0].parameters.push(parameter);
        }
        let product = graph.values.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: BinaryOp::Mul,
                left: 1,
                right: 1,
            }),
        });
        let taken = graph.block(0, &[]);
        let otherwise = graph.block(0, &[]);
        graph.blocks[0].exit = Exit::If {
            condition: 0,
            taken: edge(taken),
            otherwise: edge(otherwise),
        };
        for block in [taken, otherwise] {
            graph.blocks[block.0].exit = Exit::Return(vec![product]);
        }
        Self {
            graph,
            product,
            taken,
            otherwise,
        }
    }

    fn increment(&mut self) -> usize {
        let one = self.graph.values.literal(Type::I32, 1);
        self.graph.values.push(Value {
            ty: Type::I32,
            definition: ValueDefinition::Expression(Expression::Binary {
                operator: BinaryOp::Add,
                left: self.product,
                right: one,
            }),
        })
    }

    fn entry_schedule(&self) -> Vec<usize> {
        self.schedules().remove(0)
    }

    fn schedules(&self) -> Vec<Vec<usize>> {
        let reachable = self.graph.reachable();
        let dominators = Dominators::new(
            0,
            &successors(&self.graph, &reachable),
            &predecessors(&self.graph, &reachable),
        );
        schedules(&self.graph, &reachable, &dominators)
    }
}

#[test]
fn separate_exits_can_cover_a_retained_calculation_input() {
    let mut branches = Branches::new();
    let increment = branches.increment();
    branches.graph.blocks[branches.otherwise.0].exit = Exit::Return(vec![increment]);
    assert_eq!(branches.entry_schedule(), [branches.product]);
}

#[test]
fn publication_only_values_stay_at_their_uses() {
    assert!(Branches::new().entry_schedule().is_empty());
}

#[test]
fn a_single_site_witness_still_shares_publication_only_values() {
    let mut branches = Branches::new();
    let join = branches.graph.block(0, &[]);
    branches.graph.blocks[join.0].exit = Exit::Return(vec![branches.product]);
    for block in [branches.taken, branches.otherwise] {
        branches.graph.blocks[block.0].exit = Exit::Jump(edge(join));
    }
    let memory = Mem(0);
    branches.graph.memories.push(memory);
    let base = branches.graph.values.literal(Type::I32, 0);
    branches.graph.effects.push(Effect {
        results: Vec::new(),
        operation: Operation::store(
            MemoryAccess {
                memory,
                offset: 0,
                bytes: 4,
            },
            base,
            branches.product,
        ),
        origin: branches.taken,
    });
    branches.graph.blocks[branches.taken.0]
        .items
        .push(BlockItem::Effect(EffectId(0)));
    assert_eq!(branches.entry_schedule(), [branches.product]);
}

#[test]
fn unused_calculations_do_not_supply_a_sharing_witness() {
    let mut branches = Branches::new();
    branches.increment();
    assert!(branches.entry_schedule().is_empty());
}

#[test]
fn select_users_do_not_supply_retained_calculation_demand() {
    let mut branches = Branches::new();
    let zero = branches.graph.values.literal(Type::I32, 0);
    let choice = branches.graph.values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Expression(Expression::Select {
            condition: 2,
            when_true: branches.product,
            when_false: zero,
        }),
    });
    branches.graph.blocks[branches.taken.0].exit = Exit::Return(vec![branches.product, zero]);
    branches.graph.blocks[branches.otherwise.0].exit = Exit::Return(vec![branches.product, choice]);
    assert!(branches.entry_schedule().is_empty());
}

#[test]
fn a_bypass_prevents_collective_placement() {
    let mut branches = Branches::new();
    let increment = branches.increment();
    let use_value = branches.graph.block(0, &[]);
    let bypass = branches.graph.block(0, &[]);
    branches.graph.blocks[branches.otherwise.0].exit = Exit::If {
        condition: 2,
        taken: edge(use_value),
        otherwise: edge(bypass),
    };
    branches.graph.blocks[use_value.0].exit = Exit::Return(vec![increment]);
    let zero = branches.graph.values.literal(Type::I32, 0);
    branches.graph.blocks[bypass.0].exit = Exit::Return(vec![zero]);
    assert!(branches.entry_schedule().is_empty());
}

#[test]
fn a_backedge_before_the_demand_prevents_collective_placement() {
    let mut branches = Branches::new();
    let increment = branches.increment();
    let after = branches.graph.block(0, &[]);
    branches.graph.blocks[branches.otherwise.0].exit = Exit::If {
        condition: 2,
        taken: edge(branches.otherwise),
        otherwise: edge(after),
    };
    branches.graph.blocks[after.0].exit = Exit::Return(vec![increment]);
    assert!(branches.entry_schedule().is_empty());
}

#[test]
fn a_demand_before_the_backedge_covers_that_path() {
    let mut branches = Branches::new();
    let increment = branches.increment();
    let after = branches.graph.block(0, &[]);
    branches.graph.blocks[branches.otherwise.0].exit = Exit::Switch {
        selector: branches.product,
        cases: vec![(1, edge(branches.otherwise))],
        default: edge(after),
    };
    branches.graph.blocks[after.0].exit = Exit::Return(vec![increment]);
    assert_eq!(branches.entry_schedule(), [branches.product]);
}

fn branch_dominators() -> (Dominators, Dominators) {
    // Dispatch selects one of two conditional regions or a bypass. Each region
    // has a join before the common exit; only the first has an empty second arm.
    let successors = [
        vec![1, 4, 8],
        vec![2, 3],
        vec![3],
        vec![9],
        vec![5, 6],
        vec![7],
        vec![7],
        vec![9],
        vec![9],
        vec![],
    ];
    dominance_trees(&successors)
}

fn dominance_trees(successors: &[Vec<usize>]) -> (Dominators, Dominators) {
    let mut predecessors = vec![Vec::new(); successors.len()];
    for (source, targets) in successors.iter().enumerate() {
        for &target in targets {
            predecessors[target].push(source);
        }
    }
    (
        Dominators::new(0, successors, &predecessors),
        Dominators::new(successors.len() - 1, &predecessors, successors),
    )
}

#[test]
fn a_dispatch_bypass_does_not_prevent_sharing_within_its_cases() {
    let (dominators, postdominators) = branch_dominators();
    let mut demand = Demand {
        common_block: Some(0),
        multiple_blocks: true,
        required_sites: HashSet::from([2, 3, 5, 7]),
        retained_input: false,
    };
    let placements = demand.branch_placements(0, &dominators, &postdominators);
    assert_eq!(
        placements.into_iter().collect::<HashSet<_>>(),
        HashSet::from([1, 4])
    );
    assert_eq!(demand.common_block, Some(0));
    assert_eq!(demand.required_sites, HashSet::from([1, 4]));
}

#[test]
fn partial_sharing_preserves_uncovered_demands() {
    let (dominators, postdominators) = branch_dominators();
    let mut demand = Demand {
        common_block: Some(0),
        multiple_blocks: true,
        required_sites: HashSet::from([2, 3, 5, 6]),
        retained_input: true,
    };
    assert_eq!(
        demand.branch_placements(0, &dominators, &postdominators),
        [1]
    );
    assert_eq!(demand.common_block, Some(0));
    assert_eq!(demand.required_sites, HashSet::from([1, 5, 6]));
}

#[test]
fn branch_sharing_cannot_precede_an_input_definition() {
    let (dominators, postdominators) = branch_dominators();
    let mut demand = Demand {
        common_block: Some(0),
        multiple_blocks: true,
        required_sites: HashSet::from([2, 3, 5, 7]),
        retained_input: false,
    };
    assert!(demand
        .branch_placements(2, &dominators, &postdominators)
        .is_empty());
    assert_eq!(demand.required_sites, HashSet::from([2, 3, 5, 7]));
}

#[test]
fn nested_branches_share_below_a_rejected_region() {
    let (dominators, postdominators) = dominance_trees(&[
        vec![1, 6, 7],
        vec![2, 5],
        vec![3, 4],
        vec![4],
        vec![8],
        vec![8],
        vec![8],
        vec![8],
        vec![],
    ]);
    let mut demand = Demand {
        common_block: Some(0),
        multiple_blocks: true,
        required_sites: HashSet::from([3, 4, 5, 7]),
        retained_input: false,
    };
    assert_eq!(
        demand.branch_placements(0, &dominators, &postdominators),
        [2]
    );
    assert_eq!(demand.required_sites, HashSet::from([2, 5, 7]));
}

#[test]
fn a_backedge_can_bypass_a_subgroup_witness() {
    let mut branches = Branches::new();
    let optional = branches.graph.block(0, &[]);
    let backedge = branches.graph.block(0, &[]);
    let after = branches.graph.block(0, &[]);
    branches.graph.blocks[branches.taken.0].exit = Exit::If {
        condition: 2,
        taken: edge(optional),
        otherwise: edge(backedge),
    };
    branches.graph.blocks[optional.0].exit = Exit::Jump(edge(backedge));
    branches.graph.blocks[backedge.0].exit = Exit::If {
        condition: 0,
        taken: edge(branches.taken),
        otherwise: edge(after),
    };
    branches.graph.blocks[after.0].exit = Exit::Return(vec![branches.product]);
    branches.graph.memories.push(Mem(0));
    let base = branches.graph.values.literal(Type::I32, 0);
    branches.graph.effects.push(Effect {
        results: Vec::new(),
        operation: Operation::store(
            MemoryAccess {
                memory: Mem(0),
                offset: 0,
                bytes: 4,
            },
            base,
            branches.product,
        ),
        origin: optional,
    });
    branches.graph.blocks[optional.0]
        .items
        .push(BlockItem::Effect(EffectId(0)));
    assert!(branches.schedules().iter().all(Vec::is_empty));
}

#[test]
fn coverage_queries_do_not_reuse_previous_demands() {
    let successors = [vec![1, 2], vec![3], vec![3], Vec::new()];
    let mut coverage = Coverage::new(&successors, 3);
    assert!(coverage.all_paths_reach(0, &HashSet::from([1, 2])));
    assert!(!coverage.all_paths_reach(0, &HashSet::from([1])));
    assert!(coverage.all_paths_reach(0, &HashSet::from([1, 2])));
    assert!(!coverage.all_paths_reach(0, &HashSet::new()));
}

#[test]
fn converging_paths_can_reuse_an_explored_tail() {
    let successors = [vec![1, 2], vec![3], vec![3], vec![4], vec![5], Vec::new()];
    let mut coverage = Coverage::new(&successors, 5);
    assert!(coverage.all_paths_reach(0, &HashSet::from([4])));
}
