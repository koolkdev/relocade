//! Cover pure conversions at the original ordinary-memory effect.
//!
//! Only changed accesses occupy the selection map. Ordinary accesses keep their
//! operands and results in the graph and use the plain encoding form.

use super::Selection;
use crate::body::{BlockItem, EffectId, FunctionGraph, OperationKind, ValueDefinition};
use crate::emit::memory::{LoadForm, StoreForm};
use crate::expression::UnaryOp as Unary;
use crate::{Expression, Type};

#[derive(Clone, Copy)]
pub(in crate::emit) struct SelectedLoad {
    pub(in crate::emit) result: usize,
    pub(in crate::emit) form: LoadForm,
}

#[derive(Clone, Copy)]
pub(in crate::emit) struct SelectedStore {
    pub(in crate::emit) value: usize,
    pub(in crate::emit) form: StoreForm,
}

#[derive(Clone, Copy)]
pub(super) enum MemorySelection {
    Load(SelectedLoad),
    Store(SelectedStore),
}

impl Selection {
    pub(in crate::emit) fn load(&self, graph: &FunctionGraph, effect: EffectId) -> SelectedLoad {
        match self.memory.get(&effect) {
            Some(MemorySelection::Load(load)) => *load,
            _ => SelectedLoad {
                result: graph.effects[effect.0].results[0],
                form: LoadForm::Plain,
            },
        }
    }

    pub(in crate::emit) fn store(&self, graph: &FunctionGraph, effect: EffectId) -> SelectedStore {
        match self.memory.get(&effect) {
            Some(MemorySelection::Store(store)) => *store,
            _ => SelectedStore {
                value: self.resolve(
                    graph.effects[effect.0]
                        .operation
                        .inputs()
                        .next_back()
                        .unwrap(),
                ),
                form: StoreForm::Plain,
            },
        }
    }

    pub(super) fn cover_store(&mut self, graph: &FunctionGraph, effect: EffectId) {
        let operation = &graph.effects[effect.0].operation;
        let OperationKind::Store { access } = operation.kind() else {
            return;
        };
        let value = self.resolve(operation.inputs().next_back().unwrap());
        let Some(mut store) = self.store_conversion(graph, value, access.bytes) else {
            return;
        };
        self.replace_input(graph, value, store.value);
        while matches!(store.form, StoreForm::Plain) {
            let Some(next) = self.store_conversion(graph, store.value, access.bytes) else {
                break;
            };
            self.replace_input(graph, store.value, next.value);
            store = next;
        }
        self.memory.insert(effect, MemorySelection::Store(store));
    }

    fn store_conversion(
        &self,
        graph: &FunctionGraph,
        value: usize,
        bytes: u8,
    ) -> Option<SelectedStore> {
        let ValueDefinition::Expression(Expression::Unary { operator, input }) =
            graph.values[value].definition
        else {
            return None;
        };
        let input_type = graph.values[input].ty;
        let bits = bytes * 8;
        let form = match operator {
            // A store observes only its low bits, regardless of signedness.
            Unary::LowBits { bits: kept } if kept >= bits && input_type.is_integer() => {
                StoreForm::Plain
            }
            Unary::SignExtend { input_type: source }
                if source.bits() >= bits && input_type.is_integer() =>
            {
                StoreForm::Plain
            }
            Unary::Convert
                if input_type.is_integer()
                    && graph.values[value].ty.is_integer()
                    && input_type.bits() >= bits =>
            {
                StoreForm::Plain
            }
            Unary::ExtractLane { lane }
                if graph.values[value].ty.bits() == bits && input_type.carrier() == Type::V128 =>
            {
                StoreForm::Lane { lane }
            }
            Unary::Reinterpret
                if graph.values[value].ty == Type::I64
                    && bits == 64
                    && input_type.carrier() == Type::V128 =>
            {
                StoreForm::Lane { lane: 0 }
            }
            _ => return None,
        };
        Some(SelectedStore {
            value: self.resolve(input),
            form,
        })
    }

    pub(super) fn cover_load(&mut self, graph: &FunctionGraph, result: usize) {
        let ValueDefinition::Expression(Expression::Unary {
            operator,
            mut input,
        }) = graph.values[result].definition
        else {
            return;
        };
        let (mut bits, form) = match operator {
            Unary::SignExtend { input_type } => (input_type.bits(), LoadForm::Signed),
            Unary::Reinterpret
                if graph.values[result].ty == Type::V64 && graph.values[input].ty == Type::I64 =>
            {
                (64, LoadForm::Splat64)
            }
            _ => return,
        };
        let signed = matches!(form, LoadForm::Signed);
        let mut conversions = vec![result];
        loop {
            input = self.resolve(input);
            if self.uses[input] != 1 {
                return;
            }
            match graph.values[input].definition {
                ValueDefinition::Expression(Expression::Unary {
                    operator: Unary::SignExtend { input_type },
                    input: original,
                }) if signed && input_type.bits() <= bits => {
                    conversions.push(input);
                    bits = input_type.bits();
                    input = original;
                }
                ValueDefinition::Result {
                    producer: BlockItem::Effect(effect),
                    ..
                } => {
                    let OperationKind::Load { access } = graph.effects[effect.0].operation.kind()
                    else {
                        return;
                    };
                    if bits != access.bytes * 8 {
                        return;
                    }
                    // Only the loaded result's representation changes. The read
                    // stays at this effect, including across later writes/calls.
                    self.memory
                        .insert(effect, MemorySelection::Load(SelectedLoad { result, form }));
                    for conversion in conversions {
                        self.skipped[conversion] = true;
                    }
                    return;
                }
                _ => return,
            }
        }
    }
}
