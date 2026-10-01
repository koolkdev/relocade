//! Instruction covers over the placed graph's value and effect identities.
use super::wasm_type;
use crate::{
    body::{BlockItem, Exit, FunctionGraph, OperationKind, ValueDefinition},
    Expression, Type,
};

pub(super) struct Selection {
    pub(super) skipped: Vec<bool>,
    pub(super) signed_load: Vec<Option<usize>>,
    pub(super) narrow_test: Vec<bool>,
    aliases: Vec<Option<usize>>,
    uses: Vec<usize>,
    regions: Vec<usize>,
}
impl Selection {
    pub(super) fn new(graph: &FunctionGraph, reachable: &[bool]) -> Self {
        let count = graph.values.len();
        let mut this = Self {
            skipped: vec![false; count],
            signed_load: vec![None; graph.effects.len()],
            narrow_test: vec![false; count],
            aliases: vec![None; count],
            uses: vec![0; count],
            regions: vec![0; count],
        };
        let mut order = Vec::new();
        let mut region = 0;
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            region += 1;
            for &item in &block.items {
                for &value in graph.results(&item) {
                    this.regions[value] = region;
                    order.push(value);
                }
                for input in graph.inputs(item) {
                    this.uses[input] += 1;
                }
                if let BlockItem::Effect(effect) = item {
                    if !matches!(
                        graph.effects[effect.0].operation.kind(),
                        OperationKind::Load { .. }
                    ) {
                        region += 1;
                    }
                }
            }
            for input in block.exit.inputs() {
                this.uses[input] += 1;
            }
        }
        for &value in &order {
            if let ValueDefinition::Expression(Expression::Convert { input }) =
                graph.values[value].definition
            {
                if wasm_type(graph.values[value].ty) == wasm_type(graph.values[input].ty) {
                    this.aliases[value] = Some(input);
                    this.skipped[value] = true;
                }
            }
        }
        for &value in order.iter().rev() {
            if let Some(input) = this.aliases[value] {
                this.uses[input] += this.uses[value].saturating_sub(1);
            }
        }
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            for item in &block.items {
                if let BlockItem::Evaluate(value) = *item {
                    if let ValueDefinition::Expression(Expression::Select { condition, .. }) =
                        graph.values[value].definition
                    {
                        this.truth(graph, condition, false);
                    }
                }
            }
            if let Exit::If { condition, .. } = block.exit {
                this.truth(graph, condition, true);
            }
        }
        for &value in order.iter().rev() {
            match graph.values[value].definition {
                ValueDefinition::Expression(Expression::SignExtend { input })
                    if !this.skipped[value] =>
                {
                    this.cover_load(graph, value, input)
                }
                ValueDefinition::Expression(Expression::ZeroTest { input, .. })
                    if this.aliases[value].is_none() =>
                {
                    let input = this.resolve(input);
                    if this.uses[input] == 1
                        && matches!(graph.values[input].ty, Type::I8 | Type::I16)
                        && matches!(graph.values[input].definition, ValueDefinition::Expression(Expression::LowBits { bits, .. }) if bits == graph.values[input].ty.bits())
                    {
                        this.narrow_test[input] = true;
                    }
                }
                _ => {}
            }
        }
        this
    }
    pub(super) fn enabled(&self, producer: BlockItem) -> bool {
        match producer {
            BlockItem::Evaluate(value) => !self.skipped[value],
            BlockItem::Effect(_) => true,
        }
    }
    pub(super) fn results<'a>(
        &self,
        graph: &'a FunctionGraph,
        producer: &'a BlockItem,
    ) -> impl DoubleEndedIterator<Item = usize> + 'a {
        let replacement = match *producer {
            BlockItem::Effect(effect) => self.signed_load[effect.0],
            BlockItem::Evaluate(_) => None,
        };
        let original = if replacement.is_some() {
            &[]
        } else {
            graph.results(producer)
        };
        replacement.into_iter().chain(original.iter().copied())
    }
    pub(super) fn resolve(&self, mut value: usize) -> usize {
        while let Some(alias) = self.aliases[value] {
            value = alias;
        }
        value
    }
    pub(super) fn condition(
        &self,
        graph: &FunctionGraph,
        value: usize,
        inverted: bool,
    ) -> (usize, bool) {
        if self.skipped[value] {
            if let ValueDefinition::Expression(Expression::ZeroTest { input, nonzero }) =
                graph.values[value].definition
            {
                return (self.resolve(input), inverted ^ !nonzero);
            }
        }
        (self.resolve(value), inverted)
    }
    fn truth(&mut self, graph: &FunctionGraph, value: usize, can_invert: bool) {
        if self.uses[value] != 1 {
            return;
        }
        let ValueDefinition::Expression(Expression::ZeroTest { input, nonzero }) =
            graph.values[value].definition
        else {
            return;
        };
        if graph.values[input].ty == Type::I64 || (!nonzero && !can_invert) {
            return;
        }
        self.skipped[value] = true;
        if nonzero {
            self.aliases[value] = Some(input);
        }
    }
    fn cover_load(&mut self, graph: &FunctionGraph, result: usize, mut input: usize) {
        let mut bits = graph.values[input].ty.bits();
        let mut extensions = vec![result];
        loop {
            input = self.resolve(input);
            if self.uses[input] != 1 {
                return;
            }
            match graph.values[input].definition {
                ValueDefinition::Expression(Expression::SignExtend { input: original })
                    if graph.values[original].ty.bits() <= bits =>
                {
                    extensions.push(input);
                    bits = graph.values[original].ty.bits();
                    input = original;
                }
                ValueDefinition::Result { effect, .. } => {
                    let OperationKind::Load { access } = graph.effects[effect.0].operation.kind()
                    else {
                        return;
                    };
                    if bits != access.bytes * 8 || self.regions[input] != self.regions[result] {
                        return;
                    }
                    self.signed_load[effect.0] = Some(result);
                    for extension in extensions {
                        self.skipped[extension] = true;
                    }
                    return;
                }
                _ => return,
            }
        }
    }
}
