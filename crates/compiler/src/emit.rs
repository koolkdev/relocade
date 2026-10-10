//! Forward Wasm encoding over placed graph IDs and operand-stack coverage.
use crate::{
    body::{BlockId, BlockItem, FunctionGraph, OperationKind, ValueDefinition},
    Type, WasmFeatures,
};
use wasm_encoder::{Function, Instruction as Wasm, ValType};
mod control;
mod expression;
mod function;
mod memory;
mod selection;
mod switch;
mod view;
use selection::Selection;

pub(super) fn wasm_type(ty: Type) -> ValType {
    match ty.carrier() {
        Type::I64 => ValType::I64,
        Type::F64 => ValType::F64,
        _ => ValType::I32,
    }
}

pub(super) fn encode(
    graph: FunctionGraph,
    parameter_count: u32,
    memories: &[Option<u32>],
    functions: &[Option<u32>],
    features: WasmFeatures,
) -> Function {
    let reachable = graph.reachable();
    let selection = Selection::new(&graph, &reachable);
    let view = view::OperandView::new(&graph, &selection, &reachable);
    let mut locals = vec![None; graph.values.len()];
    for &value in &graph.blocks[graph.entry.0].parameters {
        let ValueDefinition::Parameter { component, .. } = graph.values[value].definition else {
            panic!("entry parameters name parameter definitions")
        };
        locals[value] = Some(component as u32);
    }
    let mut slot_types = Vec::new();
    for (index, value) in graph.values.iter().enumerate() {
        if locals[index].is_none() && !matches!(value.definition, ValueDefinition::Literal(_)) {
            locals[index] = Some(parameter_count + slot_types.len() as u32);
            slot_types.push(wasm_type(value.ty));
        }
    }
    let switch_local = parameter_count + slot_types.len() as u32;
    slot_types.push(ValType::I32);
    let mut writer = Writer {
        graph: &graph,
        memories,
        functions,
        features,
        selection,
        view,
        locals,
        switch_local,
        encoder: function::FunctionEncoder::new(parameter_count, slot_types),
        labels: Vec::new(),
        reachable,
        terminal: false,
    };
    writer.layouts(&graph.layout, None);
    if !writer.terminal {
        writer.emit(Wasm::Unreachable);
    }
    writer.encoder.finish()
}

enum Pending {
    Value(usize),
    Item(BlockItem),
    Finish(BlockItem),
}
struct Writer<'a> {
    graph: &'a FunctionGraph,
    memories: &'a [Option<u32>],
    functions: &'a [Option<u32>],
    features: WasmFeatures,
    selection: Selection,
    view: view::OperandView,
    locals: Vec<Option<u32>>,
    switch_local: u32,
    encoder: function::FunctionEncoder,
    labels: Vec<Option<BlockId>>,
    reachable: Vec<bool>,
    terminal: bool,
}
impl Writer<'_> {
    fn emit(&mut self, instruction: Wasm<'_>) {
        self.terminal = matches!(
            instruction,
            Wasm::Return | Wasm::ReturnCall(_) | Wasm::Br(_) | Wasm::Unreachable
        );
        self.encoder.instruction(instruction);
    }
    fn local(&self, value: usize) -> u32 {
        self.locals[value].expect("a nonconstant value has a symbolic local")
    }
    fn value(&mut self, value: usize) {
        self.expand(Pending::Value(value));
    }
    fn item(&mut self, item: BlockItem) {
        self.expand(Pending::Item(item));
    }
    fn expand(&mut self, first: Pending) {
        let mut pending = vec![first];
        while let Some(task) = pending.pop() {
            match task {
                Pending::Value(value) => {
                    let value = self.selection.resolve(value);
                    match self.graph.values[value].definition {
                        ValueDefinition::Literal(bits) => {
                            self.emit(match self.graph.values[value].ty.carrier() {
                                Type::I64 => Wasm::I64Const(u64::from(bits) as i64),
                                Type::F64 => {
                                    Wasm::F64Const(wasm_encoder::Ieee64::new(u64::from(bits)))
                                }
                                _ => Wasm::I32Const(u64::from(bits) as i32),
                            })
                        }
                        _ => {
                            if let Some(item) =
                                self.view.producer[value].filter(|&item| self.view.inline(item))
                            {
                                pending.push(Pending::Item(item));
                            } else {
                                self.emit(Wasm::LocalGet(self.local(value)));
                            }
                        }
                    }
                }
                Pending::Item(item) => {
                    pending.push(Pending::Finish(item));
                    pending.extend(self.graph.inputs(item).rev().map(Pending::Value));
                }
                Pending::Finish(item) => self.instruction(item),
            }
        }
    }
    fn block_items(&mut self, block: BlockId) {
        for &item in &self.graph.blocks[block.0].items {
            if !self.selection.enabled(item) || self.view.inline(item) {
                continue;
            }
            self.item(item);
            for result in self.selection.results(self.graph, &item).rev() {
                if self.view.uses[result] == 0 {
                    self.emit(Wasm::Drop);
                } else {
                    self.emit(Wasm::LocalSet(self.local(result)));
                }
            }
        }
    }
    fn instruction(&mut self, item: BlockItem) {
        match item {
            BlockItem::Evaluate(value) => {
                let ty = self.graph.values[value].ty;
                self.expression(ty, self.selection.expression(self.graph, value));
            }
            BlockItem::Effect(id) => {
                let effect = &self.graph.effects[id.0];
                let instruction = match effect.operation.kind() {
                    OperationKind::Load { access } => {
                        let covered = self.selection.signed_load[id.0];
                        let result = covered.unwrap_or(effect.results[0]);
                        memory::load(
                            memory::argument(self.memories, access),
                            access.bytes,
                            self.graph.values[result].ty,
                            covered.is_some(),
                        )
                    }
                    OperationKind::Store { access } => {
                        let value = effect.operation.inputs().next_back().unwrap();
                        memory::store(
                            memory::argument(self.memories, access),
                            access.bytes,
                            self.graph.values[value].ty,
                        )
                    }
                    OperationKind::MemoryFill { memory } => Wasm::MemoryFill(
                        self.memories[memory.0].expect("a filled memory is retained"),
                    ),
                    OperationKind::MemoryCopy {
                        destination_memory,
                        source_memory,
                    } => Wasm::MemoryCopy {
                        dst_mem: self.memories[destination_memory.0]
                            .expect("a copied memory is retained"),
                        src_mem: self.memories[source_memory.0]
                            .expect("a copied memory is retained"),
                    },
                    OperationKind::Call { target } => {
                        Wasm::Call(self.functions[target.0].expect("a called function is retained"))
                    }
                    OperationKind::Atomic { access, operator } => memory::atomic(
                        memory::argument(self.memories, access),
                        access.bytes,
                        operator,
                    ),
                    OperationKind::Fence => Wasm::AtomicFence,
                };
                self.emit(instruction);
            }
        }
    }
}
