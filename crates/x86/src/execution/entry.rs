//! Host ABI entries adapt external inputs to shared architectural operations.

use super::ExecutionModule;
use crate::{abi, ExecutionExitValues};
use relocade_compiler::{BlockBuilder, BuildError, Program, Val, I16, I32};

impl ExecutionModule {
    pub(crate) fn export_entries(&self, program: &mut Program) -> Result<(), BuildError> {
        if let Some(interrupt) = self.interrupt {
            abi::INTERRUPT.export(program, interrupt)?;
            let mut entry = self.clone();
            entry.interrupt = None; // Entry faults return without another delivery attempt.
            let far_call = program.function::<abi::FarCallSignature>(
                move |body, (segment, offset, stack_bytes)| {
                    entry.enter_far_call(body, segment, offset, stack_bytes)
                },
            );
            abi::FAR_CALL.export(program, far_call)?;
        }
        Ok(())
    }

    fn enter_far_call(
        &self,
        mut body: BlockBuilder<'_>,
        selector: Val<I16>,
        offset: Val<I16>,
        stack_bytes: Val<I16>,
    ) -> Result<(), BuildError> {
        let return_ip = self.cpu.read_eip(&mut body)?;
        let mut execution = self.builder(body, return_ip.clone())?;
        let target = execution.call_far(
            offset,
            selector,
            return_ip,
            stack_bytes.unsigned().extend::<I32>(),
        )?;
        execution.state.publish(&mut execution.body, target, 0)?;
        execution
            .body
            .return_(ExecutionExitValues::far_call_entered())
    }
}
