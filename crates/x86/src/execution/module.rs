//! Shared CPU declarations and architectural entry helpers for generated modules.

use super::{calls::CallLinks, ExecutionBuilder};
use crate::{
    abi,
    memory::Memory,
    profile::EntryGuarantees,
    runtime::Runtime,
    state::{Cpu, State},
    CpuModel, ExecutionProfile, ExecutionTarget, MemoryBackend,
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
    pub(super) cpu_model: CpuModel,
    instruction_budgets: bool,
    entry_guarantees: EntryGuarantees,
    pub(super) interrupt: Option<TypedFunc<abi::InterruptSignature>>,
}

impl ExecutionModule {
    pub(crate) fn declare(
        program: &mut Program,
        target: ExecutionTarget,
        entry_guarantees: EntryGuarantees,
    ) -> Self {
        let mut module = Self::new(
            program,
            target.memory_backend,
            target.cpu_model,
            target.instruction_budgets,
            entry_guarantees,
        );
        match target.profile {
            ExecutionProfile::Real16 => module.declare_real_mode_interrupt(program),
            ExecutionProfile::Protected(_) => module.runtime.import_hosted_descriptors(program),
        }
        module
    }

    pub(crate) fn pc_interpreter(program: &mut Program) -> Self {
        let mut module = Self::new(
            program,
            MemoryBackend::PhysicalBus,
            CpuModel::default(),
            true,
            EntryGuarantees::pc(),
        );
        module.declare_real_mode_interrupt(program);
        module
    }

    fn new(
        program: &mut Program,
        memory_backend: MemoryBackend,
        cpu_model: CpuModel,
        instruction_budgets: bool,
        entry_guarantees: EntryGuarantees,
    ) -> Self {
        Self {
            cpu: Arc::new(Cpu::declare(program)),
            memory: Arc::new(Memory::declare(program, memory_backend)),
            entries: Arc::default(),
            call_links: Arc::new(CallLinks::declare(program)),
            runtime: Runtime::declare(program),
            cpu_model,
            instruction_budgets,
            entry_guarantees,
            interrupt: None,
        }
    }

    fn declare_real_mode_interrupt(&mut self, program: &mut Program) {
        // A fault in the entry helper returns directly, without another delivery attempt.
        let entry = self.clone();
        self.interrupt = Some(program.function::<abi::InterruptSignature>(
            move |mut body, (vector,)| {
                let eip = entry.cpu.read_eip(&mut body)?;
                entry.builder(body, eip)?.deliver_interrupt(vector)
            },
        ));
    }

    pub(crate) fn state(&self) -> State<'_> {
        State::for_entry(&self.cpu, &self.entry_guarantees)
            .with_instruction_budgets(self.instruction_budgets)
    }

    pub(crate) fn with_blocks(
        &self,
        entries: BTreeMap<u32, Func>,
        call_sites: BTreeMap<u32, u32>,
    ) -> Self {
        Self {
            entries: Arc::new(entries),
            call_links: Arc::new(self.call_links.with_sites(call_sites)),
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
