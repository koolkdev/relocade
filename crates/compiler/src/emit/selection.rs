//! Choose Wasm instruction forms using placed uses and effect boundaries.
//!
//! Value normalization and folding live in expression::fold. These replacements
//! depend on placed consumers and which result bits they observe.
//! Selection leaves the graph's value definitions intact.
use super::wasm_type;
use crate::expression::{BinaryOp as Binary, TernaryOp as Ternary, UnaryOp as Unary};
use crate::{
    body::{BlockItem, EffectId, Exit, FunctionGraph, ValueDefinition},
    integer::BinaryOp,
    Expression, Type,
};
use rustc_hash::FxHashMap;

mod lanes;
mod memory;

pub(super) struct Selection {
    skipped: Vec<bool>,
    memory: FxHashMap<EffectId, memory::MemorySelection>,
    zero_test_masks: Vec<bool>,
    aliases: Vec<Option<usize>>,
    uses: Vec<usize>,
}
impl Selection {
    pub(super) fn new(graph: &FunctionGraph, reachable: &[bool]) -> Self {
        let count = graph.values.len();
        let mut this = Self {
            skipped: vec![false; count],
            memory: FxHashMap::default(),
            zero_test_masks: vec![false; count],
            aliases: vec![None; count],
            uses: vec![0; count],
        };
        let mut order = Vec::new();
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            for &item in &block.items {
                for value in graph.results(item) {
                    order.push(value);
                }
                for input in graph.inputs(item) {
                    this.uses[input] += 1;
                }
            }
            for input in block.exit.inputs() {
                this.uses[input] += 1;
            }
        }
        let mut has_lane_observations = false;
        for &value in &order {
            if let ValueDefinition::Expression(Expression::Unary { operator, input }) =
                graph.values[value].definition
            {
                match operator {
                    Unary::Convert
                        if wasm_type(graph.values[value].ty)
                            == wasm_type(graph.values[input].ty) =>
                    {
                        this.bypass_expression(graph, value, input);
                    }
                    Unary::ExtractLane { .. } => {
                        has_lane_observations |=
                            graph.values[value].ty.bits() < graph.values[input].ty.bits();
                    }
                    _ => {}
                }
            }
        }
        let lanes_changed = has_lane_observations && this.discard_unobserved_lanes(graph, &order);
        for (index, block) in graph.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            for item in &block.items {
                if let BlockItem::Effect(effect) = *item {
                    this.cover_store(graph, effect);
                }
                if let BlockItem::Evaluate(value) = *item {
                    if let ValueDefinition::Expression(Expression::Ternary {
                        operator: Ternary::Select,
                        third: condition,
                        ..
                    }) = graph.values[value].definition
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
            if lanes_changed && this.retire_unused(graph, value) {
                continue;
            }
            let ValueDefinition::Expression(Expression::Unary { operator, input }) =
                graph.values[value].definition
            else {
                continue;
            };
            match operator {
                Unary::SignExtend { .. } | Unary::Reinterpret if !this.skipped[value] => {
                    this.cover_load(graph, value);
                }
                Unary::ZeroTest { .. } if this.aliases[value].is_none() => {
                    let input = this.resolve(input);
                    let ty = graph.values[input].ty;
                    if this.uses[input] != 1 || !matches!(ty, Type::I8 | Type::I16) {
                        continue;
                    }
                    if let ValueDefinition::Expression(Expression::Unary {
                        operator: Unary::LowBits { bits },
                        ..
                    }) = graph.values[input].definition
                    {
                        if bits == ty.bits() {
                            this.zero_test_masks[input] = true;
                        }
                    }
                }
                _ => {}
            }
        }
        this
    }
    /// Operand-stack coverage and encoding consume the same selected operands.
    pub(super) fn inputs<'a>(
        &'a self,
        graph: &'a FunctionGraph,
        producer: BlockItem,
    ) -> impl DoubleEndedIterator<Item = usize> + 'a {
        let store = match producer {
            BlockItem::Effect(effect) => match self.memory.get(&effect) {
                Some(memory::MemorySelection::Store(store)) => Some(store.value),
                _ => None,
            },
            BlockItem::Evaluate(_) => None,
        };
        // A store's ordered inputs are its address and then its payload.
        let mut inputs = graph.inputs(producer);
        let last = inputs.next_back().map(|input| store.unwrap_or(input));
        inputs.chain(last)
    }
    pub(super) fn enabled(&self, producer: BlockItem) -> bool {
        match producer {
            BlockItem::Evaluate(value) => !self.skipped[value],
            BlockItem::Effect(_) => true,
        }
    }
    /// Select for the placed consumers, preserving ordered inputs and a prefix
    /// of the original results. Result enumeration uses this same expression.
    pub(super) fn expression(&self, graph: &FunctionGraph, value: usize) -> Expression<Type> {
        let ValueDefinition::Expression(expression) = graph.values[value].definition else {
            panic!("evaluate names an expression")
        };
        match expression.map(|&input| graph.values[input].ty) {
            Expression::Unary {
                operator: Unary::LowBits { bits: _ },
                ..
            } if self.zero_test_masks[value] => {
                // Masking and sign extension agree on zero, although their
                // nonzero carriers can differ. No other use observes this value.
                Expression::Unary {
                    operator: Unary::SignExtend {
                        input_type: graph.values[value].ty,
                    },
                    input: graph.values[value].ty,
                }
            }
            Expression::MultiplyWide { left, right, .. }
                if self.uses[graph.values.expression_result(value, 1)] == 0 =>
            {
                // Signed and unsigned multiplication have the same low half.
                Expression::Binary {
                    operator: Binary::Integer(BinaryOp::Mul),
                    left,
                    right,
                }
            }
            expression => expression,
        }
    }
    pub(super) fn results<'a>(
        &self,
        graph: &'a FunctionGraph,
        producer: &'a BlockItem,
    ) -> impl DoubleEndedIterator<Item = usize> + 'a {
        let replacement = match *producer {
            BlockItem::Effect(effect) => match self.memory.get(&effect) {
                Some(memory::MemorySelection::Load(load)) => Some(load.result),
                _ => None,
            },
            BlockItem::Evaluate(_) => None,
        };
        let count = match *producer {
            BlockItem::Evaluate(value) => self
                .expression(graph, value)
                .result_types(graph.values[value].ty)
                .len(),
            BlockItem::Effect(_) => graph.results(*producer).len(),
        };
        replacement.into_iter().chain(
            graph
                .results(*producer)
                .take(count)
                .filter(move |_| replacement.is_none()),
        )
    }
    pub(super) fn resolve(&self, mut value: usize) -> usize {
        while let Some(alias) = self.aliases[value] {
            value = alias;
        }
        value
    }
    /// Redirect one consumer, retaining a producer needed by other consumers.
    fn replace_input(&mut self, graph: &FunctionGraph, old: usize, input: usize) {
        let old = self.resolve(old);
        let input = self.resolve(input);
        self.uses[input] += 1;
        self.uses[old] -= 1;
        self.retire_unused(graph, old);
    }
    /// Bypass a single-result expression for all of its placed consumers.
    /// The caller proves the replacement preserves what those consumers observe.
    fn bypass_expression(&mut self, graph: &FunctionGraph, value: usize, input: usize) {
        debug_assert_eq!(graph.values.expression_results(value).len(), 1);
        let input = self.resolve(input);
        self.uses[input] += std::mem::take(&mut self.uses[value]);
        let retired = self.retire_unused(graph, value);
        debug_assert!(retired);
        self.aliases[value] = Some(input);
    }
    fn retire_unused(&mut self, graph: &FunctionGraph, value: usize) -> bool {
        if self.skipped[value] {
            return false;
        }
        let ValueDefinition::Expression(expression) = graph.values[value].definition else {
            return false;
        };
        if graph
            .values
            .expression_results(value)
            .any(|result| self.uses[result] != 0)
        {
            return false;
        }
        self.skipped[value] = true;
        for &input in expression.inputs() {
            let input = self.resolve(input);
            self.uses[input] -= 1;
        }
        true
    }
    pub(super) fn condition(
        &self,
        graph: &FunctionGraph,
        value: usize,
        inverted: bool,
    ) -> (usize, bool) {
        if self.skipped[value] {
            if let ValueDefinition::Expression(Expression::Unary {
                operator: Unary::ZeroTest { nonzero },
                input,
            }) = graph.values[value].definition
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
        let ValueDefinition::Expression(Expression::Unary {
            operator: Unary::ZeroTest { nonzero },
            input,
        }) = graph.values[value].definition
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
}
