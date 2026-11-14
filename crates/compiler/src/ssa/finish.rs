//! Local folding and reuse of already defined calculations, including SSA joins.
use super::*;
use rustc_hash::FxHashMap;
mod joins;
mod motion;
mod paths;
use paths::Paths;

pub(super) fn run(graph: &mut SsaGraph, changed: &[bool]) {
    let reachable = graph.reachable();
    let incoming = predecessors(graph, &reachable);
    let dominators = Dominators::new(graph.entry.0, &successors(graph, &reachable), &incoming);
    let proofs = Paths::new(graph, &dominators, &incoming);
    let frontiers = dominators.frontiers(&incoming);
    let count = graph.values.len();
    let numbers = super::numbering::Numbering::new(&graph.values);
    let mut definitions = vec![None; count];
    for (index, block) in graph.blocks.iter().enumerate() {
        for &value in &block.parameters {
            definitions[value] = Some(BlockId(index));
        }
        for &item in &block.items {
            match item {
                BlockItem::Evaluate(id) => {
                    for output in graph.values.expression_results(id) {
                        definitions[output] = Some(BlockId(index));
                    }
                }
                BlockItem::Effect(id) => {
                    for &output in &graph.effects[id.0].results {
                        definitions[output] = Some(BlockId(index));
                    }
                }
            }
        }
    }
    let mut pass = Reuse {
        graph,
        definitions,
        dominators,
        incoming,
        proofs,
        changed,
        frontiers,
        aliases: (0..count).collect(),
        numbers,
        visible: vec![None; count],
        changes: Vec::new(),
        executions: FxHashMap::default(),
        complete: vec![false; reachable.len()],
    };
    let mut children = vec![Vec::new(); reachable.len()];
    for (block, parent) in pass.dominators.parent.iter().enumerate() {
        if let Some(parent) = parent {
            if *parent != block {
                children[*parent].push(block);
            }
        }
    }
    for children in &mut children {
        children.sort_by_key(|&block| pass.dominators.rank[block]);
    }
    let mut work = vec![(pass.graph.entry.0, None)];
    while let Some((index, checkpoint)) = work.pop() {
        if let Some(checkpoint) = checkpoint {
            while pass.changes.len() > checkpoint {
                let (number, previous) = pass.changes.pop().unwrap();
                pass.visible[number] = previous;
            }
            continue;
        }
        let checkpoint = pass.changes.len();
        let block = BlockId(index);
        for (component, parameter) in pass.graph.blocks[index]
            .parameters
            .clone()
            .into_iter()
            .enumerate()
        {
            if index == pass.graph.entry.0 {
                continue;
            }
            let mut common = None;
            let mut same = !pass.incoming[index].is_empty();
            for &source in &pass.incoming[index] {
                if !pass.complete[source] {
                    same = false;
                    break;
                }
                for edge in pass.graph.outgoing(BlockId(source)) {
                    if edge.target == block {
                        let argument = pass.resolve(edge.arguments[component]);
                        if common.is_some_and(|previous| previous != argument) {
                            same = false;
                        }
                        common = Some(argument);
                    }
                }
            }
            if same {
                if let Some(value) = common {
                    pass.aliases[parameter] = value;
                    pass.numbers.assign(parameter, pass.numbers.get(value));
                }
            }
        }
        let items = std::mem::take(&mut pass.graph.blocks[index].items);
        for item in items {
            match item {
                BlockItem::Evaluate(producer) => pass.calculation(producer, block),
                BlockItem::Effect(effect) => {
                    pass.graph.effects[effect.0].operation = pass.graph.effects[effect.0]
                        .operation
                        .clone()
                        .map_inputs(|input| pass.resolve(input));
                    pass.graph.blocks[index].items.push(item);
                }
            }
        }
        let mut exit = pass.graph.blocks[index].exit.clone();
        exit.map_inputs(|input| pass.resolve(input));
        pass.graph.blocks[index].exit = exit;
        pass.complete[index] = true;
        work.push((index, Some(checkpoint)));
        work.extend(children[index].iter().rev().map(|&child| (child, None)));
    }
    for id in 0..pass.aliases.len() {
        pass.aliases[id] = pass.resolve(id);
    }
    for value in &mut pass.graph.values.values {
        if let ValueDefinition::Expression(expression) = value.definition {
            value.definition =
                ValueDefinition::Expression(expression.map(|&input| pass.aliases[input]));
        }
    }
    for effect in &mut pass.graph.effects {
        effect.operation = effect
            .operation
            .clone()
            .map_inputs(|input| pass.aliases[input]);
    }
    for block in &mut pass.graph.blocks {
        block.exit.map_inputs(|input| pass.aliases[input]);
    }
}

struct Reuse<'a> {
    graph: &'a mut SsaGraph,
    definitions: Vec<Option<BlockId>>,
    changed: &'a [bool],
    proofs: Paths,
    dominators: Dominators,
    incoming: Vec<Vec<usize>>,
    frontiers: Vec<Vec<usize>>,
    aliases: Vec<usize>,
    numbers: super::numbering::Numbering,
    visible: Vec<Option<usize>>,
    changes: Vec<(usize, Option<usize>)>,
    executions: FxHashMap<usize, Vec<(BlockId, usize)>>,
    complete: Vec<bool>,
}

impl Reuse<'_> {
    fn extend(&mut self) {
        self.numbers.extend(&self.graph.values);
        while self.aliases.len() < self.graph.values.len() {
            let id = self.aliases.len();
            self.aliases.push(id);
            self.visible.push(None);
            self.definitions.push(None);
        }
    }

    fn resolve(&self, mut value: usize) -> usize {
        while self.aliases[value] != value {
            value = self.aliases[value];
        }
        value
    }

    fn bind(&mut self, number: usize, value: usize, block: BlockId) {
        let previous = self.visible[number].replace(value);
        self.changes.push((number, previous));
        self.executions
            .entry(number)
            .or_default()
            .push((block, value));
    }

    fn calculation(&mut self, producer: usize, block: BlockId) {
        let result = self.graph.values.expression(producer).unwrap();
        let expression = result.expression.map(|&input| self.resolve(input));
        if expression == result.expression && !self.changed.get(producer).copied().unwrap_or(false)
        {
            self.materialize(producer, block);
            return;
        }
        let outputs: Vec<_> = self.graph.values.expression_results(producer).collect();
        for (component, output) in outputs.into_iter().enumerate() {
            let ty = self.graph.values[output].ty;
            let folded =
                crate::expression::fold_carrier(&mut self.graph.values, ty, expression, component);
            self.extend();
            let value = self.materialize(folded, block);
            self.aliases[output] = value;
            self.numbers.assign(output, self.numbers.get(value));
        }
    }

    fn materialize(&mut self, root: usize, block: BlockId) -> usize {
        let mut work = vec![(root, false)];
        while let Some((id, ready)) = work.pop() {
            if self.aliases[id] != id {
                continue;
            }
            let Some(result) = self.graph.values.expression(id) else {
                continue;
            };
            // Inputs which already belong to the graph are SSA definitions.
            // Only freshly folded expressions require scheduling here.
            if self.visible[self.numbers.get(id)].is_some() {
                self.aliases[id] = self.visible[self.numbers.get(id)].unwrap();
                continue;
            }
            if !ready {
                work.push((id, true));
                for &input in result.expression.inputs().rev() {
                    let input = self.resolve(input);
                    if self.graph.values.expression(input).is_some()
                        && !self.executions.contains_key(&self.numbers.get(input))
                    {
                        work.push((input, false));
                    }
                }
                continue;
            }
            let expression = result.expression.map(|&input| self.resolve(input));
            let representative =
                self.numbers
                    .define(&self.graph.values, result.producer, expression);
            let outputs: Vec<_> = self
                .graph
                .values
                .expression_results(result.producer)
                .collect();
            let mut reused = Vec::new();
            for component in 0..outputs.len() {
                let number = self
                    .graph
                    .values
                    .expression_result(representative, component);
                let value = self.visible[number].or_else(|| {
                    if self.may_join(number, block) {
                        self.hoist(number, block).or_else(|| {
                            // Keep reconstruction local: carrying a cheap value
                            // across many joins costs more than computing it.
                            self.join(number, block, block, &mut 8)
                                .map(|(value, _)| value)
                        })
                    } else {
                        None
                    }
                });
                reused.push((number, value));
            }
            let existing = reused.iter().all(|(_, value)| value.is_some());
            if !existing {
                self.graph.values.values[result.producer].definition =
                    ValueDefinition::Expression(expression);
                self.graph.blocks[block.0]
                    .items
                    .push(BlockItem::Evaluate(result.producer));
            }
            for (output, (number, value)) in outputs.into_iter().zip(reused) {
                self.numbers.assign(output, number);
                if existing {
                    self.aliases[output] = value.unwrap();
                } else {
                    self.definitions[output] = Some(block);
                    self.bind(number, output, block);
                }
            }
        }
        self.resolve(root)
    }
}
