use crate::{storage::Storage, wasm::WasmCaller};
use wasm86_machine::{Cpu, MachineConfig, MachineStorage, Memory};

#[cfg(test)]
mod tests;

/// Access to the calling CPU and shared guest memory during a host call.
/// This view borrows existing storage; CPU and memory edits apply immediately.
pub struct HostCall<'a> {
    config: &'a MachineConfig,
    storage: &'a mut dyn MachineStorage,
}

impl HostCall<'_> {
    /// Borrows the CPU that entered this host call.
    pub fn cpu(&mut self) -> Cpu<'_> {
        Cpu::new(self.config, self.storage)
    }

    /// Borrows the machine's shared guest memory through its configured map.
    pub fn memory(&mut self) -> Memory<'_> {
        Memory::new(self.config, self.storage)
    }
}

/// Lends access to the calling CPU and shared guest memory to a synchronous
/// host handler, using the same live views as `Runtime::machine`.
///
/// The binding supplies the calling CPU's execution-context memory, the machine
/// configuration and guest memory. Memory handles are moved into this call;
/// their contents stay in the engine. Guest code must publish CPU state before
/// the call and observe changes afterward.
/// Ordinary port/MMIO imports do not provide that boundary. Guest reentry is
/// unsupported. Segment edits and snapshot restores validate before writing.
///
/// Returns the handler's result directly. Earlier CPU and memory edits remain
/// applied if the handler returns an error.
pub fn with_host_call<C: WasmCaller, R>(
    config: &MachineConfig,
    caller: &mut C,
    context: C::Memory,
    guest: C::Memory,
    handler: impl FnOnce(&mut HostCall<'_>) -> R,
) -> R {
    let mut storage = Storage {
        access: caller,
        context,
        guest,
    };
    handler(&mut HostCall {
        config,
        storage: &mut storage,
    })
}
