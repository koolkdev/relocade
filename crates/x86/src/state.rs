pub(super) mod exit;

use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Mem, MemoryImport, Program, Val, I32};

use crate::{
    register::{Gpr32, Register32},
    ssa::{Environment, Location, Span},
};

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
    values: Environment,
}

impl State {
    pub(super) fn new(memory: Mem) -> Self {
        Self {
            memory,
            values: Environment::new(memory),
        }
    }

    pub(super) fn read_register(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        register: impl Into<Register32>,
    ) -> Result<Val<I32>, BuildError> {
        match register.into() {
            Register32::Named(register) => {
                self.values.read(body, Location(register_offset(register)))
            }
            Register32::Indexed(index) => {
                self.values
                    .read_at(body, Span::new(24, 32), index.shl(2), 24)
            }
        }
    }

    pub(super) fn write_register(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        register: impl Into<Register32>,
        value: impl IntoOp<I32>,
    ) -> Result<(), BuildError> {
        match register.into() {
            Register32::Named(register) => {
                self.values
                    .define(body, Location(register_offset(register)), value)
            }
            Register32::Indexed(index) => {
                self.values
                    .write_at(body, Span::new(24, 32), index.shl(2), 24, value)
            }
        }
    }

    /// Publishes current completed instructions on a terminating path. Indexed
    /// accesses may already have synchronized register definitions to backing.
    /// Later definitions do not change an earlier authored exit; this does not
    /// restore an older state after partially executing a new instruction.
    pub(super) fn publish(
        &self,
        body: &mut FunctionBuilder<'_>,
        next_eip: impl IntoOp<I32>,
        completed: u32,
    ) -> Result<(), BuildError> {
        self.values.publish(body)?;
        body.store::<I32>(self.memory, EIP_OFFSET, next_eip)?;
        let count = body.load::<I32>(self.memory, INSTRUCTION_COUNT_OFFSET)?;
        body.store(self.memory, INSTRUCTION_COUNT_OFFSET, count.add(completed))
    }
}

#[cfg(test)]
mod tests;
