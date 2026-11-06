//! Partial register images stay attached to the CPU that supplied them.

use super::{registers::Register, CpuStateAccess};
use relocade_x86::SegmentRegister;
use std::future::Future;

/// Captured register views, restored explicitly or by [`CpuStateAccess::with_saved_registers`].
/// Dropping the image does not write CPU state: a suspended service may have been
/// abandoned while an interrupt or another service owns the live registers.
pub struct SavedRegisters<const N: usize> {
    cpu: CpuStateAccess,
    values: [SavedRegister; N],
}

struct SavedRegister {
    register: Register,
    // Segment caches are the largest supported register image. Only the named
    // view's bytes are read or written; unused array capacity is never published.
    bytes: [u8; size_of::<SegmentRegister>()],
}

impl CpuStateAccess {
    /// Saves exactly the named register views, without flags or execution state.
    pub fn save_registers<const N: usize>(&self, registers: [Register; N]) -> SavedRegisters<N> {
        SavedRegisters {
            cpu: self.clone(),
            values: registers.map(|register| {
                let mut bytes = [0; size_of::<SegmentRegister>()];
                self.read_into(register.offset, &mut bytes[..register.bytes]);
                SavedRegister { register, bytes }
            }),
        }
    }

    /// Preserves named registers across an async operation, restoring them only
    /// when it returns. Cancelling the operation leaves the live CPU untouched.
    ///
    /// ```
    /// # use relocade_machine::{CpuContext, registers as reg};
    /// # async fn callback(call: &mut CpuContext<'_>) {
    /// call.cpu.with_saved_registers([reg::ax, reg::dx], async {
    ///     call.cpu.set_ax(0);
    ///     call.call_interrupt(0x1c).await;
    /// }).await;
    /// # }
    /// ```
    pub async fn with_saved_registers<const N: usize, F: Future>(
        &self,
        registers: [Register; N],
        future: F,
    ) -> F::Output {
        let saved = self.save_registers(registers);
        let result = future.await;
        saved.restore();
        result
    }
}

impl<const N: usize> SavedRegisters<N> {
    /// Restores the captured views on their original CPU. Segment caches remain
    /// valid because the CPU's execution target cannot change.
    pub fn restore(&self) {
        for saved in &self.values {
            self.cpu.write(
                saved.register.offset as u32,
                &saved.bytes[..saved.register.bytes],
            );
        }
    }
}
