//! Semantic identities are independent of where an expression is executed.
use super::*;
use rustc_hash::FxHashMap;

pub(super) struct Numbering {
    numbers: Vec<usize>,
    expressions: FxHashMap<Value, usize>,
}

impl Numbering {
    pub(super) fn new(values: &ValueTable<impl crate::body::Phase>) -> Self {
        let mut numbering = Self {
            numbers: Vec::new(),
            expressions: FxHashMap::default(),
        };
        numbering.extend(values);
        numbering
    }

    pub(super) fn extend(&mut self, values: &ValueTable<impl crate::body::Phase>) {
        for id in self.numbers.len()..values.len() {
            let number = if matches!(values[id].definition, ValueDefinition::Literal(_)) {
                *self.expressions.entry(values[id]).or_insert(id)
            } else {
                id
            };
            self.numbers.push(number);
        }
    }

    pub(super) fn get(&self, value: usize) -> usize {
        self.numbers[value]
    }
    pub(super) fn assign(&mut self, value: usize, number: usize) {
        self.numbers[value] = number;
    }

    pub(super) fn define(
        &mut self,
        values: &ValueTable<impl crate::body::Phase>,
        producer: usize,
        expression: Expression<usize>,
    ) -> usize {
        let key = Value {
            ty: values[producer].ty,
            definition: ValueDefinition::Expression(
                expression.map(|&input| self.numbers[values.representation(input)]),
            ),
        };
        let representative = *self.expressions.entry(key).or_insert(producer);
        for (component, output) in values.expression_results(producer).enumerate() {
            self.numbers[output] = values.expression_result(representative, component);
        }
        representative
    }
}
