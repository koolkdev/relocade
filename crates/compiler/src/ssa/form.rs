//! Establish definitions and explicit copies before interpreting path facts.
use super::*;
mod coverage;
use coverage::Coverage;

#[derive(Default)]
enum Sites {
    #[default]
    None,
    One(BlockId),
    Many(Vec<BlockId>),
}

impl Sites {
    fn add(&mut self, block: BlockId, dominators: &Dominators) {
        match self {
            Self::None => *self = Self::One(block),
            Self::One(previous) if dominators.dominates(previous.0, block.0) => {}
            Self::One(previous) if dominators.dominates(block.0, previous.0) => *previous = block,
            Self::One(previous) => *self = Self::Many(vec![*previous, block]),
            Self::Many(sites) => {
                if sites
                    .iter()
                    .any(|site| dominators.dominates(site.0, block.0))
                {
                    return;
                }
                sites.retain(|site| !dominators.dominates(block.0, site.0));
                sites.push(block);
            }
        }
    }

    fn choose(self, coverage: &mut Coverage<'_>) -> Vec<BlockId> {
        let sites = match self {
            Self::None => return Vec::new(),
            Self::One(site) => return vec![site],
            Self::Many(sites) => sites,
        };
        let mut pending = vec![sites];
        let mut placed = Vec::new();
        while let Some(sites) = pending.pop() {
            let common = sites.iter().fold(sites[0].0, |common, site| {
                coverage.dominators.common(common, site.0).unwrap()
            });
            if sites.len() == 1 || coverage.covers(common, &sites) {
                placed.push(BlockId(common));
                continue;
            }
            let mut groups = std::collections::BTreeMap::<usize, Vec<BlockId>>::new();
            for site in sites {
                groups
                    .entry(coverage.dominators.child_containing(common, site.0))
                    .or_default()
                    .push(site);
            }
            pending.extend(groups.into_values());
        }
        placed
    }
}

pub(super) fn build(
    graph: &mut FunctionGraph,
    reachable: &[bool],
    dominators: &Dominators,
) -> Vec<Option<BlockId>> {
    let original = graph.values.len();
    let mut demands: Vec<_> = (0..original).map(|_| Sites::None).collect();
    for (index, block) in graph.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        for &item in &block.items {
            for input in graph.inputs(item) {
                if graph.values.expression(input).is_some() {
                    demands[input].add(BlockId(index), dominators);
                }
            }
        }
        for input in block.exit.inputs() {
            if graph.values.expression(input).is_some() {
                demands[input].add(BlockId(index), dominators);
            }
        }
    }
    let mut coverage = Coverage::new(graph, reachable, dominators);
    let mut scheduled = vec![Vec::new(); graph.blocks.len()];
    for id in (0..original).rev() {
        let sites = std::mem::take(&mut demands[id]).choose(&mut coverage);
        if let Some(result) = graph.values.expression(id) {
            for site in sites {
                scheduled[site.0].push(result.producer);
                for &input in result.expression.inputs() {
                    debug_assert!(input < id);
                    if graph.values.expression(input).is_some() {
                        demands[input].add(site, dominators);
                    }
                }
            }
        }
    }

    drop(coverage);
    let mut children = vec![Vec::new(); graph.blocks.len()];
    for (block, parent) in dominators.parent.iter().enumerate() {
        if let Some(parent) = parent {
            if *parent != block {
                children[*parent].push(block);
            }
        }
    }
    for children in &mut children {
        children.sort_by_key(|&block| dominators.rank[block]);
    }
    // The expression-building adapter ends here. Analysis receives only actual
    // SSA definitions, without retaining the unused recipe arena beside them.
    let source = std::mem::take(&mut graph.values);
    let effects = std::mem::take(&mut graph.effects);
    for (index, block) in graph.blocks.iter_mut().enumerate() {
        if !reachable[index] {
            block.items.clear();
            block.parameters.clear();
            block.callees.clear();
            block.exit = Exit::Trap;
        }
    }
    graph.values.define_calculations();
    let mut former = Formation {
        graph,
        source,
        effects,
        definitions: Vec::new(),
        bindings: vec![None; original],
        changes: Vec::new(),
    };
    let mut here = vec![0; original];
    let mut work = vec![(former.graph.entry.0, None)];
    while let Some((index, checkpoint)) = work.pop() {
        if let Some(checkpoint) = checkpoint {
            while former.changes.len() > checkpoint {
                let (id, previous) = former.changes.pop().unwrap();
                former.bindings[id] = previous;
            }
            continue;
        }
        let block = BlockId(index);
        let checkpoint = former.changes.len();
        for (component, id) in former.graph.blocks[index]
            .parameters
            .clone()
            .into_iter()
            .enumerate()
        {
            let value = former
                .graph
                .values
                .push_with_bounds(former.source[id], former.source.bounds[id]);
            former.graph.blocks[index].parameters[component] = value;
            former.define(id, value, block);
        }
        let items = std::mem::take(&mut former.graph.blocks[index].items);
        for &id in &scheduled[index] {
            here[id] = index + 1;
        }
        for item in items {
            let BlockItem::Effect(id) = item else {
                unreachable!()
            };
            let operation = former.effects[id.0]
                .operation
                .clone()
                .map_inputs(|input| former.value(input, block, &here));
            let effect = EffectId(former.graph.effects.len());
            let mut results = Vec::new();
            for (component, output) in former.effects[id.0].results.clone().into_iter().enumerate()
            {
                let value = former.graph.values.push_with_bounds(
                    Value {
                        ty: former.source[output].ty,
                        definition: ValueDefinition::Result {
                            producer: BlockItem::Effect(effect),
                            component,
                        },
                    },
                    former.source.bounds[output],
                );
                results.push(value);
                former.define(output, value, block);
            }
            former.graph.effects.push(Effect {
                results,
                operation,
                origin: block,
            });
            former.graph.blocks[index]
                .items
                .push(BlockItem::Effect(effect));
        }
        for &id in scheduled[index].iter().rev() {
            former.value(id, block, &here);
        }
        let mut exit = former.graph.blocks[index].exit.clone();
        exit.map_inputs(|input| former.value(input, block, &here));
        former.graph.blocks[index].exit = exit;
        work.push((index, Some(checkpoint)));
        work.extend(children[index].iter().rev().map(|&child| (child, None)));
    }
    former.definitions
}

struct Formation<'a> {
    graph: &'a mut FunctionGraph,
    source: ValueTable,
    effects: Vec<Effect>,
    definitions: Vec<Option<BlockId>>,
    bindings: Vec<Option<usize>>,
    changes: Vec<(usize, Option<usize>)>,
}

impl Formation<'_> {
    fn define(&mut self, source: usize, value: usize, block: BlockId) {
        self.definitions.resize(self.graph.values.len(), None);
        self.definitions[value] = Some(block);
        let previous = self.bindings[source].replace(value);
        self.changes.push((source, previous));
    }

    fn value(&mut self, root: usize, block: BlockId, here: &[usize]) -> usize {
        let mut pending = vec![(root, false)];
        while let Some((id, ready)) = pending.pop() {
            if self.bindings[id].is_some() {
                continue;
            }
            let Some(result) = self.source.expression(id) else {
                assert!(
                    matches!(self.source[id].definition, ValueDefinition::Literal(_)),
                    "SSA operand has an available definition"
                );
                let value = self.graph.values.intern(self.source[id]);
                self.definitions.resize(self.graph.values.len(), None);
                self.bindings[id] = Some(value);
                continue;
            };
            assert!(
                here[result.producer] == block.0 + 1,
                "a demanded calculation has a planned definition"
            );
            if !ready {
                pending.push((id, true));
                pending.extend(
                    result
                        .expression
                        .inputs()
                        .rev()
                        .map(|&input| (input, false)),
                );
                continue;
            }
            let expression = result
                .expression
                .map(|&input| self.bindings[input].unwrap());
            let definition = Value {
                ty: self.source[result.producer].ty,
                definition: ValueDefinition::Expression(expression),
            };
            let producer = self.graph.values.push(definition);
            self.graph.blocks[block.0]
                .items
                .push(BlockItem::Evaluate(producer));
            let originals: Vec<_> = self.source.expression_results(result.producer).collect();
            for (component, original) in originals.into_iter().enumerate() {
                let value = self.graph.values.expression_result(producer, component);
                let retained = &mut self.graph.values.bounds[value];
                retained.unsigned = retained.unsigned.min(self.source.bounds[original].unsigned);
                retained.signed = retained.signed.min(self.source.bounds[original].signed);
                self.define(original, value, block);
            }
        }
        self.bindings[root].unwrap()
    }
}
