mod operands;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, IntoOp, MemoryInt, Val, I1, I32};

use crate::{
    flags::{ArithmeticSource, Condition, FlagSource, LocalFlagSource},
    instruction::DecodedInstruction,
    memory::Memory,
    semantics,
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
        semantics::lower(self, decoded.instruction)?;
        self.eip = self.body.value(decoded.next_eip)?;
        self.completed += 1;
        Ok(())
    }

    pub(super) fn set_arithmetic_flags<T: MemoryInt>(
        &mut self,
        source: &ArithmeticSource<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        self.state.set_arithmetic_flags(&mut self.body, source)
    }

    pub(super) fn set_logic_flags<T: MemoryInt>(
        &mut self,
        result: &Val<T>,
    ) -> Result<(), BuildError>
    where
        FlagSource<T>: Into<LocalFlagSource>,
    {
        self.state.set_logic_flags(&mut self.body, result)
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
