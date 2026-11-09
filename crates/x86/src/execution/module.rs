//! Shared CPU declarations and architectural entry helpers for generated modules.

use super::ExecutionBuilder;
use crate::{abi, memory::Memory, runtime::Runtime, state::Cpu, ExecutionProfile, ExecutionTarget};
use relocade_compiler::{BlockBuilder, BuildError, Program, TypedFunc, Val, I32};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ExecutionModule {
    pub(crate) cpu: Arc<Cpu>,
    pub(crate) memory: Arc<Memory>,
    pub(super) runtime: Runtime,
    pub(super) target: ExecutionTarget,
    pub(super) interrupt: Option<TypedFunc<abi::InterruptSignature>>,
}

impl ExecutionModule {
    pub(crate) fn declare(program: &mut Program, target: ExecutionTarget) -> Self {
        let mut module = Self {
            cpu: Arc::new(Cpu::declare(program)),
            memory: Arc::new(Memory::declare(program, target.profile)),
            runtime: Runtime::declare(program),
            target,
            interrupt: None,
        };
        if target.profile == ExecutionProfile::Real16 {
            // Entry does not execute instructions. Its failures return directly,
            // so its builder has no delivery target of its own.
            let entry = module.clone();
            module.interrupt = Some(program.function::<abi::InterruptSignature>(
                move |mut body, (vector,)| {
                    let eip = entry.cpu.read_eip(&mut body)?;
                    entry.builder(body, eip)?.deliver_interrupt(vector)
                },
            ));
        }
        module
    }

    pub(crate) fn builder<'body>(
        &self,
        body: BlockBuilder<'body>,
        start: impl Into<Val<I32>>,
    ) -> Result<ExecutionBuilder<'body, '_>, BuildError> {
        ExecutionBuilder::new(body, self, start)
    }
}
