//! Owned capabilities for one host invocation; no live state borrow survives a method.

use super::{CpuContext, HookResult, MachineCore};
use crate::{ConfigurationError, CpuView, MemoryError, MemoryView};
use relocade_wasm_runtime::WasmContext;
use relocade_x86::{CpuState, ExecutionTarget, Segment, SegmentRegister, Segments};
use std::{cell::RefCell, rc::Weak};

/// Identifies one host invocation. Clones share its validity, not a CPU borrow.
/// Access is valid only while that invocation is the active execution frame.
/// Handles retained after completion/cancellation or CPU destruction cannot access state.
pub struct Call<T: 'static = ()> {
    pub(super) core: Weak<RefCell<MachineCore<T>>>,
    pub(super) id: u64,
}

impl<T> Clone for Call<T> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            id: self.id,
        }
    }
}

impl<T: 'static> Call<T> {
    /// Runs a synchronous compound operation with scoped live state access.
    /// The result cannot borrow the context. Prefer CPU and memory methods for
    /// individual operations; this boundary supports ABI helpers and bulk work.
    pub fn with_context<R>(&self, operation: impl FnOnce(&mut CpuContext<'_, T>) -> R) -> R {
        self.with_core(|core| operation(&mut core.bindings.context(&mut core.backend)))
    }

    fn with_core<R>(&self, operation: impl FnOnce(&mut MachineCore<T>) -> R) -> R {
        let owner = self.core.upgrade().expect("host call owner was dropped");
        let mut core = owner.borrow_mut();
        assert_eq!(core.active_call, Some(self.id), "host call is not active");
        operation(&mut core)
    }

    pub fn cpu(&self) -> CallCpu<'_, T> {
        CallCpu(self)
    }

    pub fn memory(&self) -> CallMemory<'_, T> {
        CallMemory(self)
    }

    /// Returns an owned clone of the environment, without retaining a state borrow.
    /// Shared handles remain shared; ordinary structs are copied. To edit the
    /// original state, use `with_context` and its scoped `state_mut` guard.
    pub fn state(&self) -> T
    where
        T: Clone,
    {
        self.with_core(|core| core.backend.state().data.borrow().clone())
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.with_core(|core| core.bindings.config.execution_target())
    }

    /// Resumes the IRET in the real-mode host-hook stub.
    pub fn return_from_interrupt(&self) -> HookResult {
        self.with_context(super::real_mode_interrupt::return_from_interrupt)
    }
}

/// Register operations validate their invocation and borrow the CPU internally.
pub struct CallCpu<'a, T: 'static>(&'a Call<T>);

macro_rules! register {
    ($get:ident, $set:ident, $type:ty) => {
        pub fn $get(&self) -> $type {
            self.with(|cpu| cpu.$get())
        }
        pub fn $set(&self, value: $type) {
            self.with(|cpu| cpu.$set(value))
        }
    };
}

impl<T: 'static> CallCpu<'_, T> {
    fn with<R>(&self, operation: impl FnOnce(&mut CpuView<'_>) -> R) -> R {
        self.0.with_core(|core| operation(&mut core.cpu()))
    }

    register!(eax, set_eax, u32);
    register!(ecx, set_ecx, u32);
    register!(edx, set_edx, u32);
    register!(ebx, set_ebx, u32);
    register!(esp, set_esp, u32);
    register!(ebp, set_ebp, u32);
    register!(esi, set_esi, u32);
    register!(edi, set_edi, u32);
    register!(ax, set_ax, u16);
    register!(cx, set_cx, u16);
    register!(dx, set_dx, u16);
    register!(bx, set_bx, u16);
    register!(sp, set_sp, u16);
    register!(bp, set_bp, u16);
    register!(si, set_si, u16);
    register!(di, set_di, u16);
    register!(al, set_al, u8);
    register!(cl, set_cl, u8);
    register!(dl, set_dl, u8);
    register!(bl, set_bl, u8);
    register!(ah, set_ah, u8);
    register!(ch, set_ch, u8);
    register!(dh, set_dh, u8);
    register!(bh, set_bh, u8);
    register!(eip, set_eip, u32);
    register!(instruction_count, set_instruction_count, u64);
    register!(eflags, set_eflags, u32);
    register!(carry, set_carry, bool);

    pub fn segment(&self, segment: Segment) -> SegmentRegister {
        self.with(|cpu| cpu.segment(segment))
    }
    pub fn set_segment(&self, segment: Segment, value: SegmentRegister) {
        self.with(|cpu| cpu.set_segment(segment, value));
    }
    pub fn segments(&self) -> Segments {
        self.with(|cpu| cpu.segments())
    }
    pub fn snapshot(&self) -> CpuState {
        self.with(|cpu| cpu.snapshot())
    }
    pub fn restore(&self, snapshot: &CpuState) -> Result<(), ConfigurationError> {
        self.with(|cpu| cpu.restore(snapshot))
    }
}

/// Guest transfers finish inside a short invocation-checked memory borrow.
pub struct CallMemory<'a, T: 'static>(&'a Call<T>);

impl<T: 'static> CallMemory<'_, T> {
    fn with<R>(&self, operation: impl FnOnce(&mut MemoryView<'_>) -> R) -> R {
        self.0.with_core(|core| operation(&mut core.memory()))
    }
    pub fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        self.with(|memory| memory.read(address, bytes))
    }
    pub fn write(&self, address: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        self.with(|memory| memory.write(address, bytes))
    }
    pub fn read_segment(
        &self,
        segment: &SegmentRegister,
        offset: u32,
        bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        self.with(|memory| memory.read_segment(segment, offset, bytes))
    }
    pub fn write_segment(
        &self,
        segment: &SegmentRegister,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        self.with(|memory| memory.write_segment(segment, offset, bytes))
    }
}
