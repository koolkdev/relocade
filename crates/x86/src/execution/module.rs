//! Shared CPU declarations and architectural entry helpers for generated modules.

use super::{calls::CallLinks, ExecutionBuilder};
use crate::{
    abi, memory::Memory, runtime::Runtime, state::Cpu, ExecutionProfile, ExecutionTarget,
    StateAssumption,
};
use relocade_compiler::{BlockBuilder, BuildError, Func, Program, TypedFunc, Val, I32};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
pub(crate) struct ExecutionModule {
    pub(crate) cpu: Arc<Cpu>,
    pub(crate) memory: Arc<Memory>,
    pub(super) entries: Arc<BTreeMap<u32, Func>>,
    pub(super) call_links: Arc<CallLinks>,
    pub(super) runtime: Runtime,
    pub(super) target: ExecutionTarget,
    pub(super) control_assumptions: Arc<[StateAssumption]>,
    pub(super) segment_assumptions: Arc<[StateAssumption]>,
    pub(super) interrupt: Option<TypedFunc<abi::InterruptSignature>>,
}

impl ExecutionModule {
    pub(crate) fn declare(program: &mut Program, target: ExecutionTarget) -> Self {
        let mut module = Self {
            cpu: Arc::new(Cpu::declare(program)),
            memory: Arc::new(Memory::declare(program, target.profile)),
            entries: Arc::default(),
            call_links: Arc::new(CallLinks::declare(program)),
            runtime: Runtime::declare(program),
            target,
            control_assumptions: crate::profile::control_assumptions(target.profile).into(),
            segment_assumptions: crate::segment::profile::entry_assumptions(target.profile).into(),
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

    /// Retains the supplied facts, including the CS default used by decoding.
    /// Guarded observations are local to their continuing paths and are excluded.
    pub(crate) fn state_assumptions(&self) -> Vec<StateAssumption> {
        self.control_assumptions
            .iter()
            .chain(self.segment_assumptions.iter())
            .copied()
            .collect()
    }

    pub(crate) fn with_function(
        &self,
        entries: BTreeMap<u32, Func>,
        call_sites: BTreeMap<u32, u32>,
    ) -> Self {
        Self {
            entries: Arc::new(entries),
            call_links: Arc::new(self.call_links.for_function(call_sites)),
            ..self.clone()
        }
    }

    pub(crate) fn builder<'body>(
        &self,
        body: BlockBuilder<'body>,
        start: impl Into<Val<I32>>,
    ) -> Result<ExecutionBuilder<'body, '_>, BuildError> {
        ExecutionBuilder::new(body, self, start)
    }
}
