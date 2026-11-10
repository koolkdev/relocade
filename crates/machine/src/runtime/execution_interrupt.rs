//! Execution interrupts in the existing shared execution backing.

use relocade_wasm_runtime::SharedMemory;
use relocade_x86::ExecutionContext;
use std::mem::offset_of;

const OFFSET: u32 = offset_of!(ExecutionContext, interrupt_requested) as u32;

/// Requests a return from guest execution to its runner, independently of guest IF.
/// This does not deliver an x86 interrupt or create a guest interrupt frame.
/// Clones retain only the shared allocation, without CPU access or a backend.
/// Native handles can cross threads; browser handles use the existing worker
/// message transport. No access to the rest of the CPU is exposed.
///
/// This signal does not wake a pending Rust future or run device work. Its owner
/// must arrange those separately and coordinate acknowledgement with ready work.
#[derive(Clone, crate::worker::Transfer)]
pub struct ExecutionInterrupt(SharedMemory);

impl ExecutionInterrupt {
    pub(super) fn new(storage: SharedMemory) -> Self {
        Self(storage)
    }

    /// Requests an execution interrupt after making the associated work ready.
    /// Repeated requests coalesce until acknowledged, independently of guest IF.
    pub fn request(&self) {
        self.0.atomic_store(OFFSET, 1_u8);
    }

    /// Observes a pending request without acknowledging it. This is a hint for
    /// scheduling before guest entry; a producer can request again at any time.
    pub fn is_requested(&self) -> bool {
        self.0.atomic_load::<u8>(OFFSET) != 0
    }

    /// Atomically acknowledges prior requests, returning whether any were pending.
    /// Clear before examining runnable work, or track racing wakes separately.
    /// Clearing after draining without that
    /// coordination could erase a concurrent producer's request. The work queues
    /// remain the source of truth.
    pub fn take(&self) -> bool {
        self.0.atomic_swap(OFFSET, 0_u8) != 0
    }
}

impl crate::worker::WakeTarget for ExecutionInterrupt {
    fn wake(&self) {
        self.request();
    }

    fn acknowledge(&self) {
        self.take();
    }
}
