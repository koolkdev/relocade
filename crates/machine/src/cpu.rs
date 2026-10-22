mod context;
mod membership;
mod view;

pub use context::CpuContext;
pub use membership::CpuId;
pub(crate) use membership::Membership;
pub use view::CpuView;

use crate::{
    runtime::{dispatch::Dispatch, io, StateData},
    CpuBindings, DispatchEntry, Machine, Memory, Result, RuntimeBackend, RuntimeState,
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
        if bindings.configuration().execution_target().profile
            != relocade_x86::ExecutionProfile::Real16
        {
            return Err(crate::ConfigurationError::UnsupportedProfile.into());
        }
        let host_imports = io::unconnected_imports(&mut backend);
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
    /// The caller must preserve the module's execution assumptions and replace
    /// captured code when guest bytes change.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = self.backend.compiler().compile(bytes).await?;
        self.dispatch
            .install(&mut self.backend, &self.bindings, &module, &[], entries)
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
