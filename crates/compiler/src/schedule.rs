//! Resolve value dependencies and authored control into one ordered function.
use std::collections::HashMap;

use wasm_encoder::{BlockType, ValType};

use crate::{
    body::{BlockTree, Body, Site, Target},
    effects::Effects,
    emit::wasm_type,
    memory::{AtomicOperation, Location},
    module::Types,
    place, Expression, Func, Type,
};

mod branch;
mod control;
mod evaluate;
mod loops;
mod switch;
#[cfg(test)]
mod tests;

pub(super) struct Schedule {
    pub(super) instructions: Vec<Instruction>,
    pub(super) local_types: Vec<ValType>,
}

pub(super) enum LocalOp {
    Get,
    Set,
    Tee,
}

/// Instructions consume operands in Wasm stack order. Expressions retain input
/// types for opcode selection; memory operands use `()` because their values are
/// already on the stack. No instruction refers back to a source value or block.
pub(super) enum Instruction {
    Constant {
        ty: Type,
        bits: u64,
    },
    Parameter(u32),
    Local {
        slot: usize,
        operation: LocalOp,
    },
    Expression {
        result_type: Type,
        expression: Expression<Type>,
    },
    Load {
        location: Location<()>,
        result_type: Type,
        signed: bool,
    },
    Store(Location<()>),
    Atomic(AtomicOperation<()>),
    Fence,
    Call(Func),
    TailCall(Func),
    Block(BlockType),
    Loop(BlockType),
    If(BlockType),
    Else,
    End,
    Branch(u32),
    BranchIf(u32),
    BranchTable {
        targets: Vec<u32>,
        default: u32,
    },
    Drop,
    Return,
    Trap,
}

struct ControlLabel {
    target: Option<Target>,
    outputs: Vec<usize>,
}

struct Scheduler<'a> {
    body: &'a Body,
    blocks: BlockTree<'a>,
    effects: &'a [Effects],
    types: &'a mut Types,
    labels: Vec<ControlLabel>,
    slots: Vec<Option<usize>>,
    captures: HashMap<Site, Vec<usize>>,
    available: Vec<bool>,
    instructions: Vec<Instruction>,
    local_types: Vec<ValType>,
}

pub(super) fn plan(body: &Body, effects: &[Effects], types: &mut Types) -> Schedule {
    let blocks = BlockTree::new(&body.block);
    let place::Placement {
        slots,
        captures,
        slot_types,
    } = place::plan(body, effects, &blocks);
    let mut scheduler = Scheduler {
        body,
        blocks,
        effects,
        types,
        labels: Vec::new(),
        slots,
        captures,
        available: vec![false; body.values.len()],
        instructions: Vec::new(),
        local_types: slot_types,
    };
    scheduler.block(&body.block, None);
    debug_assert!(scheduler.labels.is_empty(), "function controls are closed");
    Schedule {
        instructions: scheduler.instructions,
        local_types: scheduler.local_types,
    }
}

impl Scheduler<'_> {
    fn local(&mut self, slot: usize, operation: LocalOp) {
        self.instructions
            .push(Instruction::Local { slot, operation });
    }

    fn temporary(&mut self, ty: ValType) -> usize {
        let slot = self.local_types.len();
        self.local_types.push(ty);
        slot
    }
}
