//! Function encoding with symbolic locals and loop-aware storage reuse.

use wasm_encoder::{Encode, Function, Instruction, ValType};

use super::EncodedFunction;
use crate::BranchHint;

enum LocalOp {
    Get,
    Set,
    Tee,
}

mod locals;

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
    branch_hints: Vec<(usize, BranchHint)>,
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
            branch_hints: Vec::new(),
            controls: Vec::new(),
            loop_ranges: Vec::new(),
        }
    }

    /// Scratch storage for the wide-multiply fallback. Other calculations use
    /// expression placement, which owns their stack and local storage.
    pub(super) fn temporary(&mut self, ty: ValType) -> u32 {
        let slot = self.parameter_count + self.slot_types.len() as u32;
        self.slot_types.push(ty);
        slot
    }

    /// Local indices below `parameter_count` name parameters. Higher indices
    /// name symbolic slots; finish() assigns their reusable physical locals.
    pub(super) fn instruction(&mut self, instruction: Instruction<'_>) {
        let local = match &instruction {
            Instruction::LocalGet(index) => Some((*index, LocalOp::Get)),
            Instruction::LocalSet(index) => Some((*index, LocalOp::Set)),
            Instruction::LocalTee(index) => Some((*index, LocalOp::Tee)),
            _ => None,
        };
        if let Some((index, operation)) = local {
            if index >= self.parameter_count {
                self.local((index - self.parameter_count) as usize, operation);
                return;
            }
        }
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

    fn local(&mut self, slot: usize, operation: LocalOp) {
        self.events.push(LocalEvent {
            offset: self.bytes.len(),
            slot,
            operation,
        });
    }

    /// Marks the next branch before deferred local instructions are inserted.
    pub(super) fn branch_hint(&mut self, hint: Option<BranchHint>) {
        if let Some(hint) = hint {
            self.branch_hints.push((self.bytes.len(), hint));
        }
    }

    pub(super) fn finish(mut self) -> EncodedFunction {
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
        let mut pending_hints = self.branch_hints.into_iter().peekable();
        let mut branch_hints = Vec::new();
        let mut previous = 0;
        for event in self.events {
            // Events at the same raw offset precede the branch. Resolve its
            // position only after every deferred local at that offset is encoded.
            while let Some(&(offset, hint)) = pending_hints
                .peek()
                .filter(|&&(offset, _)| offset < event.offset)
            {
                branch_hints.push(encode_hint(function.byte_len() + offset - previous, hint));
                pending_hints.next();
            }
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
        for (offset, hint) in pending_hints {
            branch_hints.push(encode_hint(function.byte_len() + offset - previous, hint));
        }
        function.raw(self.bytes[previous..].iter().copied());
        EncodedFunction {
            body: function,
            branch_hints,
        }
    }
}

fn encode_hint(offset: usize, hint: BranchHint) -> wasm_encoder::BranchHint {
    wasm_encoder::BranchHint {
        branch_func_offset: u32::try_from(offset).expect("branch offset fits the Wasm index space"),
        branch_hint_value: u32::from(hint == BranchHint::Likely),
    }
}

#[cfg(test)]
mod tests;
