mod operands;
mod repetition;
mod stack;

pub(crate) use operands::PairValues;
pub(crate) use repetition::Repetition;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, MemoryInt, Val, I1, I32, I64};

use crate::flags::{Condition, Flag, FlagChange};
use crate::instruction::{self, DecodedInstruction};
use crate::memory::{Access, Intent, Memory};
use crate::state::{exit, Cpu, State};

/// Builds one execution path. State definitions and progress describe completed
/// instructions. Within a repeated string instruction, the current state also
/// includes successful elements, while EIP and the instruction count stay at entry.
pub(super) struct ExecutionBuilder<'body, 'module> {
    body: FunctionBuilder<'body>,
    state: State<'module>,
    memory: Option<&'module Memory>,
    dispatch: Func,
    eip: Val<I32>,
    completed: u32,
}

impl<'body, 'module> ExecutionBuilder<'body, 'module> {
    pub(super) fn new(
        body: FunctionBuilder<'body>,
        cpu: &'module Cpu,
        memory: Option<&'module Memory>,
        dispatch: Func,
        start: impl Into<Val<I32>>,
    ) -> Result<Self, BuildError> {
        let eip = body.value(start)?;
        Ok(Self {
            body,
            state: State::new(cpu),
            memory,
            dispatch,
            eip,
            completed: 0,
        })
    }

    pub(super) fn execute<V: Into<Val<I32>>, P: Into<Val<I32>>>(
        &mut self,
        decoded: DecodedInstruction<V, P>,
    ) -> Result<(), BuildError> {
        self.eip = self.body.value(decoded.eip)?;
        let fallthrough_eip = self.body.value(decoded.fallthrough_eip)?;
        self.eip = instruction::lower(self, decoded.instruction, fallthrough_eip)?;
        self.completed += 1;
        Ok(())
    }

    /// Defines a flag change while preserving flags omitted from its write mask.
    pub(super) fn write_flags(&mut self, change: impl Into<FlagChange>) -> Result<(), BuildError> {
        self.state.write_flags(&mut self.body, change)
    }

    pub(super) fn read_flag(&mut self, flag: Flag) -> Result<Val<I1>, BuildError> {
        self.state.read_flag(&mut self.body, flag)
    }

    /// Reads current logical flags in request order, resolving shared backing together.
    pub(super) fn read_flags<const N: usize>(
        &mut self,
        flags: [Flag; N],
    ) -> Result<[Val<I1>; N], BuildError> {
        self.state.read_flags(&mut self.body, flags)
    }

    pub(super) fn write_flag(
        &mut self,
        flag: Flag,
        value: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.state.write_flag(&mut self.body, flag, value)
    }

    pub(super) fn condition(&mut self, condition: Condition) -> Result<Val<I1>, BuildError> {
        self.state.condition(&mut self.body, condition)
    }

    /// Ends a faulting path at the current restart boundary. Call before defining
    /// results of the faulting instruction or its current string element.
    pub(crate) fn fault_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        code: impl Into<Val<I64>>,
    ) -> Result<(), BuildError> {
        self.body.if_(condition, |mut fault_body| {
            self.state
                .publish(&mut fault_body, &self.eip, self.completed)?;
            fault_body.return_(code.into())
        })
    }

    fn checked<T: MemoryInt>(
        &mut self,
        memory: &'module Memory,
        address: &Val<I32>,
        intent: Intent,
    ) -> Result<Access<T>, BuildError> {
        memory.resolve_access::<T>(&mut self.body, address, intent, |mut fault_body, fault| {
            self.state
                .publish(&mut fault_body, &self.eip, self.completed)?;
            fault_body.return_(exit::page_fault(&fault.address, &fault.error))
        })
    }

    pub(super) fn complete(mut self) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        self.body.tail_call(self.dispatch, &[self.eip.into()])
    }
}
