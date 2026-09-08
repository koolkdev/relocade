mod cpu;
pub(super) mod exit;
mod flags;

pub(super) use cpu::Cpu;

use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Val, I32};

use crate::{
    register::{Gpr32, Register, RegisterSelection, RegisterType},
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

fn indexed_offset(slot: Val<I32>, byte: Option<Val<I32>>) -> Val<I32> {
    let offset = slot.shl(2);
    match byte {
        Some(byte) => offset.add(byte),
        None => offset,
    }
}

const EIP_OFFSET: u32 = 56;
const INSTRUCTION_COUNT_OFFSET: u32 = 144;

pub(super) struct State<'cpu> {
    cpu: &'cpu Cpu,
    values: Environment,
    flags: flags::FlagState,
}

impl<'cpu> State<'cpu> {
    pub(super) fn new(cpu: &'cpu Cpu) -> Self {
        Self {
            cpu,
            values: Environment::new(cpu.memory()),
            flags: flags::FlagState::default(),
        }
    }

    pub(super) fn read_register<T: RegisterType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        register: impl Into<Register<T>>,
    ) -> Result<Val<T>, BuildError> {
        match register.into().selection {
            RegisterSelection::Named { parent, byte } => self
                .values
                .read(body, Location::new(register_offset(parent) + byte)),
            RegisterSelection::Indexed { slot, byte } => {
                let offset = indexed_offset(slot, byte);
                self.values
                    .read_at(body, Span::new(24, T::BACKING_SLOT_COUNT * 4), offset, 24)
            }
        }
    }

    pub(super) fn write_register<T: RegisterType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        register: impl Into<Register<T>>,
        value: impl IntoOp<T>,
    ) -> Result<(), BuildError> {
        match register.into().selection {
            RegisterSelection::Named { parent, byte } => {
                self.values
                    .define(body, Location::new(register_offset(parent) + byte), value)
            }
            RegisterSelection::Indexed { slot, byte } => {
                let offset = indexed_offset(slot, byte);
                self.values.write_at(
                    body,
                    Span::new(24, T::BACKING_SLOT_COUNT * 4),
                    offset,
                    24,
                    value,
                )
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
        body.store::<I32>(self.cpu.memory(), EIP_OFFSET, next_eip)?;
        let count = body.load::<I32>(self.cpu.memory(), INSTRUCTION_COUNT_OFFSET)?;
        body.store(
            self.cpu.memory(),
            INSTRUCTION_COUNT_OFFSET,
            count.add(completed),
        )
    }
}

#[cfg(test)]
mod tests;
