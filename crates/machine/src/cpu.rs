mod context;
mod membership;
mod view;

pub use context::CpuContext;
pub use membership::CpuId;
pub(crate) use membership::Membership;
pub use view::CpuView;

use crate::{
    runtime::{
        descriptors,
        dispatch::{Dispatch, EntryKind},
        io, StateData,
    },
    CpuBindings, DispatchEntry, Error, Machine, Memory, MemoryVersion, Result, RuntimeBackend,
    RuntimeState,
};
use relocade_wasm_runtime::WasmCompiler;
use std::ops::{Deref, DerefMut};

/// One independently executable CPU belonging to a machine.
/// Owns its Wasm backend, CPU bindings and dispatch resources.
/// Guest memory is shared through the machine; host views borrow the stopped CPU.
pub struct Cpu<B: RuntimeBackend> {
    pub(crate) machine: Machine<B::SharedMemory>,
    pub(crate) backend: B,
    pub(crate) bindings: CpuBindings<B::Memory>,
    pub(crate) dispatch: Dispatch<B>,
    pub(crate) host_call_incomplete: bool,
}

impl<B: RuntimeBackend> Cpu<B> {
    pub fn id(&self) -> CpuId {
        self.bindings.id()
    }
    pub fn machine(&self) -> &Machine<B::SharedMemory> {
        &self.machine
    }

    /// Shares guest memory through a handle that can outlive or run alongside this CPU.
    pub fn memory(&self) -> Memory<B::SharedMemory> {
        self.machine.memory()
    }

    /// Borrows the published registers of this stopped CPU.
    pub fn view(&mut self) -> CpuView<'_> {
        CpuView::new(
            self.bindings.configuration(),
            self.backend.memory_bytes(&self.bindings.context),
        )
    }

    /// Borrows this stopped CPU and its shared guest memory. Other CPUs can run
    /// independently.
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext::new(
            CpuView::new(
                self.bindings.configuration(),
                self.backend.memory_bytes(&self.bindings.context),
            ),
            self.machine.memory.view(),
        )
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Cpu<B> {
    pub(crate) async fn new(
        machine: Machine<B::SharedMemory>,
        mut backend: B,
        bindings: CpuBindings<B::Memory>,
    ) -> Result<Self> {
        let mut host_imports = io::unconnected_imports(&mut backend);
        host_imports.extend(descriptors::imports(&mut backend, &bindings));
        let dispatch = Dispatch::new(
            &mut backend,
            &bindings,
            &machine.shared.runtime,
            host_imports,
        )
        .await?;
        Ok(Self {
            machine,
            backend,
            bindings,
            dispatch,
            host_call_incomplete: false,
        })
    }

    /// Links a module to this CPU and publishes its callable exports.
    /// Each export has signature `() -> i64` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    ///
    /// Capture `version` with source bytes using `Memory::snapshot`, before
    /// compilation. A changed mapping or another address space returns StaleCode.
    /// The caller must still replace captured code when guest bytes change.
    /// Modules must follow the x86 import ABI, including read-only access to the
    /// imported page table during initialization and execution.
    pub async fn install_module(
        &mut self,
        version: &MemoryVersion,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = self.backend.compiler().compile(bytes).await?;
        if !self.bindings.matches_version(&self.backend, version) {
            return Err(Error::StaleCode);
        }
        self.dispatch
            .install(
                &mut self.backend,
                &self.bindings,
                &module,
                &[],
                entries,
                EntryKind::Generated,
            )
            .await
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> impl Deref<Target = T> + '_ {
        StateData(self.backend.state())
    }

    /// Mutably borrows the environment state. Drop the borrow before running.
    pub fn state_mut(&mut self) -> impl DerefMut<Target = T> + '_ {
        StateData(self.backend.state_mut())
    }
}
