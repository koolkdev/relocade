//! Function encoding with symbolic locals and loop-aware storage reuse.

use wasm_encoder::{Encode, Function, Instruction, ValType};

mod locals;

pub(super) enum LocalOp {
    Get,
    Set,
    Tee,
}

struct LocalEvent {
    // Byte position before deferred local instructions are inserted.
    offset: usize,
    slot: usize,
    operation: LocalOp,
}

enum ControlFrame {
    Block,
    Loop { first_access: usize },
}

pub(super) struct FunctionEncoder {
    parameter_count: u32,
    slot_types: Vec<ValType>,
    bytes: Vec<u8>,
    events: Vec<LocalEvent>,
    controls: Vec<ControlFrame>,
    loop_ranges: Vec<(usize, usize)>,
}

impl FunctionEncoder {
    pub(super) fn new(parameter_count: u32, slot_types: Vec<ValType>) -> Self {
        Self {
            parameter_count,
            slot_types,
            bytes: Vec::new(),
            events: Vec::new(),
            controls: Vec::new(),
            loop_ranges: Vec::new(),
        }
    }

    pub(super) fn instruction(&mut self, instruction: Instruction<'_>) {
        match &instruction {
            Instruction::Block(_) | Instruction::If(_) => self.controls.push(ControlFrame::Block),
            Instruction::Loop(_) => self.controls.push(ControlFrame::Loop {
                first_access: self.events.len(),
            }),
            Instruction::End => {
                if let ControlFrame::Loop { first_access } = self
                    .controls
                    .pop()
                    .expect("an ending instruction closes an open control")
                {
                    // Local lifetimes use access positions, independent of the
                    // number or byte size of instructions between accesses.
                    self.loop_ranges.push((first_access, self.events.len()));
                }
            }
            _ => {}
        }
        instruction.encode(&mut self.bytes);
    }

    pub(super) fn local(&mut self, slot: usize, operation: LocalOp) {
        self.events.push(LocalEvent {
            offset: self.bytes.len(),
            slot,
            operation,
        });
    }

    pub(super) fn temporary(&mut self, ty: ValType) -> usize {
        let slot = self.slot_types.len();
        self.slot_types.push(ty);
        slot
    }

    pub(super) fn finish(mut self) -> Function {
        debug_assert!(self.controls.is_empty(), "function controls are closed");
        Instruction::End.encode(&mut self.bytes);
        // Wasm local declarations precede instructions. Allocate from the final
        // access order, then insert local instructions into the buffered code.
        let allocated = locals::allocate(
            self.events.iter().map(|event| event.slot),
            &self.slot_types,
            &self.loop_ranges,
        );
        let mut function = Function::new_with_locals_types(allocated.types);
        let mut previous = 0;
        for event in self.events {
            function.raw(self.bytes[previous..event.offset].iter().copied());
            previous = event.offset;
            let local = self
                .parameter_count
                .checked_add(
                    allocated.indices[event.slot].expect("an emitted local access has an index"),
                )
                .expect("function locals fit the Wasm index space");
            function.instruction(&match event.operation {
                LocalOp::Get => Instruction::LocalGet(local),
                LocalOp::Set => Instruction::LocalSet(local),
                LocalOp::Tee => Instruction::LocalTee(local),
            });
        }
        function.raw(self.bytes[previous..].iter().copied());
        function
    }
}

#[cfg(test)]
mod tests;
