mod operands;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, IntoOp, MemoryInt, Val, I1, I32};

use crate::{
    flags::{Condition, FlagSource, LocalFlagSource},
    instruction::{self, DecodedInstruction},
    memory::Memory,
    state::{Cpu, State},
};

/// Builds one execution path. State definitions and progress describe completed
/// instructions; a fault publishes that boundary before the current effects.
pub(super) struct ExecutionBuilder<'body, 'cpu> {
    body: FunctionBuilder<'body>,
    state: State<'cpu>,
    memory: Option<Memory>,
    dispatch: Func,
    eip: Val<I32>,
    completed: u32,
}

impl<'body, 'cpu> ExecutionBuilder<'body, 'cpu> {
    pub(super) fn new(
        body: FunctionBuilder<'body>,
        cpu: &'cpu Cpu,
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
        instruction::lower(self, decoded.instruction)?;
        self.eip = self.body.value(decoded.next_eip)?;
        self.completed += 1;
        Ok(())
    }

    pub(super) fn set_flags<T: MemoryInt>(
        &mut self,
        source: FlagSource<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        self.state.set_flags(&mut self.body, source)
    }

    pub(super) fn condition(&mut self, condition: Condition) -> Result<Val<I1>, BuildError> {
        self.state.condition(&mut self.body, condition)
    }

    pub(super) fn complete(mut self) -> Result<(), BuildError> {
        self.state
            .publish(&mut self.body, &self.eip, self.completed)?;
        self.body.tail_call(self.dispatch, &[self.eip.into()])
    }
}
