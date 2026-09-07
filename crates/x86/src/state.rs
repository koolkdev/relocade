pub(super) mod exit;

use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Mem, MemoryImport, Program, Val, I32};

use crate::register::{Gpr32, Register32};

fn register_offset(register: Gpr32) -> u32 {
    match register {
        Gpr32::Eax => 24,
        Gpr32::Ecx => 28,
        Gpr32::Edx => 32,
        Gpr32::Ebx => 36,
        Gpr32::Esp => 40,
        Gpr32::Ebp => 44,
        Gpr32::Esi => 48,
        Gpr32::Edi => 52,
    }
}

const EIP_OFFSET: u32 = 56;
const INSTRUCTION_COUNT_OFFSET: u32 = 144;

pub(super) fn declare(program: &mut Program) -> Mem {
    program.import_memory(MemoryImport {
        module: "wasm86".into(),
        name: "cpuState".into(),
        minimum: 1,
        maximum: None,
    })
}

pub(super) fn read_eip(
    body: &mut FunctionBuilder<'_>,
    memory: Mem,
) -> Result<Val<I32>, BuildError> {
    body.load::<I32>(memory, EIP_OFFSET)
}

pub(super) struct State {
    memory: Mem,
    registers: Vec<(Register32, Val<I32>)>,
}

impl State {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            registers: Vec::new(),
        }
    }

    pub(super) fn write_register(
        &mut self,
        body: &FunctionBuilder<'_>,
        register: impl Into<Register32>,
        value: impl IntoOp<I32>,
    ) -> Result<(), BuildError> {
        let value = body.value(value)?;
        let register = register.into();
        if let Register32::Named(name) = &register {
            // An indexed write may name any register, so a later named write
            // cannot replace a value that will be published before it.
            for (existing, current) in self.registers.iter_mut().rev() {
                match existing {
                    Register32::Indexed(_) => break,
                    Register32::Named(key) if key == name => {
                        *current = value;
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
        self.registers.push((register, value));
        Ok(())
    }

    /// Writes the pending snapshot for this exit without clearing it.
    /// Publications must be on mutually exclusive paths; this does not flush state
    /// for continued execution on the same path.
    pub(super) fn publish(
        &self,
        body: &mut FunctionBuilder<'_>,
        next_eip: impl IntoOp<I32>,
        completed: u32,
    ) -> Result<(), BuildError> {
        for (register, value) in &self.registers {
            match register {
                Register32::Named(register) => {
                    body.store(self.memory, register_offset(*register), value)?;
                }
                Register32::Indexed(index) => {
                    body.store_at(self.memory, index.shl(2), 24, value)?;
                }
            }
        }
        body.store::<I32>(self.memory, EIP_OFFSET, next_eip)?;
        let count = body.load::<I32>(self.memory, INSTRUCTION_COUNT_OFFSET)?;
        body.store(self.memory, INSTRUCTION_COUNT_OFFSET, count.add(completed))
    }
}

#[cfg(test)]
mod tests;
