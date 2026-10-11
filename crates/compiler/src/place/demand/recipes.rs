//! Choose execution recipes from complete semantic equivalences.
use super::*;

/// Required roots are the execution recipes chosen for residual uses. Other
/// surviving alternatives offer reuse but cannot justify placement themselves.
#[derive(Default)]
pub(super) struct Roots {
    pub(super) required: Vec<usize>,
    pub(super) reusable: Vec<usize>,
}

/// Placement can choose a full-equivalent recipe whose inputs already survive.
/// Choices affect execution only; semantic normal forms and facts stay fixed.
pub(in crate::place) struct Plan {
    pub(in crate::place) shared: Vec<Vec<usize>>,
    recipes: HashMap<(usize, usize), usize>,
}

impl Plan {
    pub(in crate::place) fn new(
        graph: &FunctionGraph,
        reachable: &[bool],
        dominators: &Dominators,
        flow: &ReducedFlow,
    ) -> Self {
        let requirements = requirements(graph, dominators);
        let mut plan = Self {
            shared: Vec::new(),
            recipes: HashMap::new(),
        };
        let roots = plan.roots(graph, flow, reachable, dominators, &requirements);
        plan.shared = schedule_roots(graph, reachable, dominators, &roots, &requirements);
        plan
    }

    pub(in crate::place) fn recipe(&self, block: BlockId, residual: usize) -> usize {
        self.recipes
            .get(&(block.0, residual))
            .copied()
            .unwrap_or(residual)
    }

    fn roots(
        &mut self,
        graph: &FunctionGraph,
        flow: &ReducedFlow,
        reachable: &[bool],
        dominators: &Dominators,
        requirements: &[Option<Requirement>],
    ) -> Vec<Roots> {
        let uses: Vec<Vec<_>> = graph
            .blocks
            .iter()
            .enumerate()
            .map(|(block, data)| {
                if !reachable[block] {
                    return Vec::new();
                }
                let mut uses = data.exit.inputs();
                for item in &data.items {
                    if let BlockItem::Effect(id) = item {
                        uses.extend(graph.effects[id.0].operation.inputs());
                    }
                }
                uses
            })
            .collect();
        let mut used = vec![false; graph.values.len()];
        for &value in uses.iter().flatten() {
            used[value] = true;
        }
        for id in (0..used.len()).rev() {
            if used[id] {
                if let Some(result) = graph.values.expression(id) {
                    for &input in result.expression.inputs() {
                        used[input] = true;
                    }
                }
            }
        }
        uses.into_iter()
            .enumerate()
            .map(|(block, mut required)| {
                let mut reusable = Vec::new();
                for input in &mut required {
                    let root = *input;
                    let Some(result) = graph.values.expression(root) else {
                        continue;
                    };
                    let mut chosen = self.recipe(BlockId(block), root);
                    for recipe in flow
                        .recipes(BlockId(block), root)
                        .filter(|&recipe| recipe != root)
                    {
                        if used[recipe] {
                            reusable.push(recipe);
                        }
                        if chosen != root {
                            continue;
                        }
                        let Some(alternative) = graph.values.expression(recipe) else {
                            continue;
                        };
                        if result.component != 0
                            || alternative.component != 0
                            || result.expression.result_types(graph.values[root].ty).len() != 1
                            || result.expression.map(|_| ()) != alternative.expression.map(|_| ())
                            || graph.values[root].ty.carrier() != graph.values[recipe].ty.carrier()
                        {
                            continue;
                        }
                        // Keep the value DAG's order and require every alternative
                        // input to survive independently of this root's choice.
                        if !alternative
                            .expression
                            .inputs()
                            .all(|&input| input < result.producer && used[input])
                        {
                            continue;
                        }
                        // The alternative must be executable wherever the residual
                        // is: equivalence alone does not make later snapshots available.
                        if !requirements[recipe]
                            .zip(requirements[root])
                            .is_some_and(|(a, b)| a.available_at(b, dominators))
                        {
                            continue;
                        }
                        chosen = recipe;
                        self.recipes.insert((block, root), recipe);
                    }
                    *input = chosen;
                }
                Roots { required, reusable }
            })
            .collect()
    }
}
