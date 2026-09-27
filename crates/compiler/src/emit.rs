//! Structured control emission with explicit value-evaluation plans.
use wasm_encoder::{Function, ValType};

use crate::{
    body::{BlockTree, Body, Site, Target},
    effects::Effects,
    module::Types,
    place, Type,
};

mod branch;
mod control;
mod evaluation;
mod function;
mod integer;
mod loops;
mod lower;
mod memory;
mod switch;

use evaluation::{Evaluation, ValuePlanner};
use function::{FunctionEncoder, LocalOp};

pub(super) fn wasm_type(ty: Type) -> ValType {
    match ty {
        Type::I1 | Type::I8 | Type::I16 | Type::I32 => ValType::I32,
        Type::I64 => ValType::I64,
    }
}

struct ControlLabel {
    target: Option<Target>,
    outputs: Vec<usize>,
}

struct Emitter<'a> {
    body: &'a Body,
    memories: &'a [Option<u32>],
    functions: &'a [Option<u32>],
    effects: &'a [Effects],
    types: &'a mut Types,
    labels: Vec<ControlLabel>,
    planner: ValuePlanner<'a>,
    code: FunctionEncoder,
}

pub(super) fn encode(
    body: &Body,
    parameter_count: u32,
    memories: &[Option<u32>],
    functions: &[Option<u32>],
    effects: &[Effects],
    types: &mut Types,
) -> Function {
    let blocks = BlockTree::new(&body.block);
    let place::Placement {
        slots,
        captures,
        slot_types,
    } = place::plan(body, effects, &blocks);
    let mut emitter = Emitter {
        body,
        memories,
        functions,
        effects,
        types,
        labels: Vec::new(),
        planner: ValuePlanner::new(body, blocks, slots, captures),
        code: FunctionEncoder::new(parameter_count, slot_types),
    };
    emitter.block(&body.block, None);
    emitter.code.finish()
}

impl Emitter<'_> {
    fn values(&mut self, inputs: impl IntoIterator<Item = usize>) {
        let plan = self.planner.values(inputs);
        self.emit(plan);
    }

    fn emit_condition(&mut self, condition: usize, inverted: bool) {
        let plan = self.planner.condition(condition, inverted);
        self.emit(plan);
    }

    fn emit_captures(&mut self, site: Site) {
        let plan = self.planner.captures(site);
        self.emit(plan);
    }

    fn authored_call(&mut self, site: Site) {
        let plan = self.planner.call(site);
        self.emit(plan);
    }

    fn save_results(&mut self, outputs: &[usize]) {
        let plan = self.planner.save_results(outputs);
        self.emit(plan);
    }

    fn emit(&mut self, plan: Vec<Evaluation>) {
        lower::values(&mut self.code, self.memories, self.functions, plan);
    }
}
