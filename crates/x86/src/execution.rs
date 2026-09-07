use wasm86_compiler::{BuildError, Func, FunctionBuilder, IntoOp, Mem, Val, I32};

use crate::{
    address,
    instruction::{DecodedInstruction, Location32},
    memory::{Access, Intent, Memory},
    semantics,
    state::{exit, State},
};

/// Builds one execution path. State definitions and progress describe completed
/// instructions; a fault publishes that boundary before the current destination
/// is written.
pub(super) struct ExecutionBuilder<'a> {
    body: FunctionBuilder<'a>,
    state: State,
    memory: Option<Memory>,
    dispatch: Func,
    eip: Val<I32>,
    completed: u32,
}

impl<'a> ExecutionBuilder<'a> {
    pub(super) fn new(
        body: FunctionBuilder<'a>,
        cpu: Mem,
        memory: Option<Memory>,
        dispatch: Func,
        start: impl IntoOp<I32>,
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

    pub(super) fn execute<V: IntoOp<I32>, P: IntoOp<I32>>(
        &mut self,
        decoded: DecodedInstruction<V, P>,
    ) -> Result<(), BuildError> {
        self.eip = self.body.value(decoded.eip)?;
        semantics::lower(self, decoded.instruction)?;
        self.eip = self.body.value(decoded.next_eip)?;
        self.completed += 1;
        Ok(())
    }

    pub(super) fn read<V: IntoOp<I32>>(
        &mut self,
        location: Location32<V>,
    ) -> Result<Val<I32>, BuildError> {
        match location {
            Location32::Register(register) => self.state.read_register(&mut self.body, register),
            Location32::Memory(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address)?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access = self.checked(memory, &address, Intent::Read)?;
                memory.read(&mut self.body, &access)
            }
        }
    }

    pub(super) fn write<V: IntoOp<I32>>(
        &mut self,
        location: Location32<V>,
        value: impl IntoOp<I32>,
    ) -> Result<(), BuildError> {
        match location {
            Location32::Register(register) => {
                self.state.write_register(&mut self.body, register, value)
            }
            Location32::Memory(address) => {
                let address = address::resolve(&mut self.body, &mut self.state, address)?;
                let memory = self.memory.expect("a memory operand declares guest memory");
                let access = self.checked(memory, &address, Intent::Write)?;
                let value = self.body.value(value)?;
                memory.write(&mut self.body, &access, &value)
            }
        }
    }

    fn checked(
        &mut self,
        memory: Memory,
        address: &Val<I32>,
        intent: Intent,
    ) -> Result<Access<I32>, BuildError> {
        let access = memory.resolve_access::<I32>(&mut self.body, address, intent)?;
        self.body.if_(&access.fault.condition, |mut arm| {
            self.state.publish(&mut arm, &self.eip, self.completed)?;
            arm.return_(exit::page_fault(&access.fault.address, &access.fault.error))
        })?;
        Ok(access)
    }

    pub(super) fn complete(mut self) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        self.body.tail_call(self.dispatch, &[self.eip.into()])
    }
}
