use relocade_x86::{
    CpuState, DescriptorTableRegister, Exception, ExecutionContext, ExecutionTarget, FlagsState,
    Segment, SegmentRegister, Segments, SystemSegmentRegister,
};
use std::{marker::PhantomData, mem::offset_of, rc::Rc};

use crate::{ConfigurationError, CpuConfig};
use relocade_wasm_runtime::{Load, MemoryValue, SharedMemory, Store, WasmSharedMemory};

/// Retained access to the complete live CPU state, independent of the backend.
/// Operations copy values and apply writes immediately; no references into Wasm
/// storage escape. The execution driver serializes guest execution and service
/// polling. Generated entries may cache fields until they publish state on exit;
/// use this resource at stopped host boundaries, not from synchronous guest
/// device callbacks. Clones share state and remain usable after the runner is dropped.
/// The handle stays on the execution thread even when its backing supports sharing.
///
/// ```compile_fail
/// use relocade_machine::CpuStateAccess;
/// fn send_state(cpu: CpuStateAccess) {
///     std::thread::spawn(move || cpu.set_ax(1));
/// }
/// ```
#[derive(Clone)]
pub struct CpuStateAccess {
    target: ExecutionTarget,
    storage: SharedMemory,
    execution_thread: PhantomData<Rc<()>>,
    #[cfg(test)]
    reads: std::cell::RefCell<Vec<(u32, usize)>>,
}

macro_rules! register {
    ($get:ident, $set:ident, $type:ty, $field:ident, $byte:expr) => {
        #[inline]
        pub fn $get(&self) -> $type {
            self.load(offset_of!(CpuState, registers.$field) + $byte)
        }

        #[inline]
        pub fn $set(&self, value: $type) {
            self.store(offset_of!(CpuState, registers.$field) + $byte, value);
        }
    };
}

impl CpuStateAccess {
    /// The execution profile and CPU model represented by these registers.
    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    /// Retains an initialized image compatible with the execution target.
    #[inline]
    pub(crate) fn new(config: &CpuConfig, storage: SharedMemory) -> Self {
        Self {
            target: config.execution_target(),
            storage,
            execution_thread: PhantomData,
            #[cfg(test)]
            reads: Default::default(),
        }
    }

    pub(crate) fn set_instruction_target(&self, target: u64) {
        self.storage.store(
            offset_of!(ExecutionContext, instruction_target) as u32,
            target,
        );
    }

    register!(eax, set_eax, u32, eax, 0);
    register!(ecx, set_ecx, u32, ecx, 0);
    register!(edx, set_edx, u32, edx, 0);
    register!(ebx, set_ebx, u32, ebx, 0);
    register!(esp, set_esp, u32, esp, 0);
    register!(ebp, set_ebp, u32, ebp, 0);
    register!(esi, set_esi, u32, esi, 0);
    register!(edi, set_edi, u32, edi, 0);
    register!(ax, set_ax, u16, eax, 0);
    register!(cx, set_cx, u16, ecx, 0);
    register!(dx, set_dx, u16, edx, 0);
    register!(bx, set_bx, u16, ebx, 0);
    register!(sp, set_sp, u16, esp, 0);
    register!(bp, set_bp, u16, ebp, 0);
    register!(si, set_si, u16, esi, 0);
    register!(di, set_di, u16, edi, 0);
    register!(al, set_al, u8, eax, 0);
    register!(cl, set_cl, u8, ecx, 0);
    register!(dl, set_dl, u8, edx, 0);
    register!(bl, set_bl, u8, ebx, 0);
    register!(ah, set_ah, u8, eax, 1);
    register!(ch, set_ch, u8, ecx, 1);
    register!(dh, set_dh, u8, edx, 1);
    register!(bh, set_bh, u8, ebx, 1);

    #[inline]
    pub fn eip(&self) -> u32 {
        self.load(offset_of!(CpuState, eip))
    }

    #[inline]
    pub fn set_eip(&self, value: u32) {
        self.store(offset_of!(CpuState, eip), value);
    }

    #[inline]
    pub fn instruction_count(&self) -> u64 {
        self.load(offset_of!(CpuState, instruction_count))
    }

    #[inline]
    pub fn set_instruction_count(&self, value: u64) {
        self.store(offset_of!(CpuState, instruction_count), value);
    }

    /// Returns represented EFLAGS bits, resolving any lazy arithmetic flags.
    pub fn eflags(&self) -> u32 {
        FlagsState::from_bytes(self.read(offset_of!(CpuState, flags))).eflags()
    }

    /// Replaces represented flags. This is host editing, not a privilege-checked
    /// POPFD operation; fixed and unrepresented bits are ignored.
    pub fn set_eflags(&self, value: u32) {
        let mut flags = FlagsState::from_bytes(self.read(offset_of!(CpuState, flags)));
        flags.set_eflags(value);
        self.write(offset_of!(CpuState, flags) as u32, &flags.to_bytes());
    }

    pub fn carry(&self) -> bool {
        self.eflags() & 1 != 0
    }

    /// Preserves the other flags, including those held in a lazy source.
    pub fn set_carry(&self, value: bool) {
        self.set_eflags((self.eflags() & !1) | u32::from(value));
    }

    pub fn segment(&self, segment: Segment) -> SegmentRegister {
        SegmentRegister::from_bytes(self.read(segment_offset(segment)))
    }

    #[inline]
    pub fn segment_base(&self, segment: Segment) -> u32 {
        self.load(segment_offset(segment) + offset_of!(SegmentRegister, base))
    }

    /// Publishes a linear code position through the live CS alias, including
    /// an invalid offset when reporting #GP. Base and limit are consecutive
    /// little-endian words; one transfer reads both for this state operation.
    #[inline]
    pub(crate) fn continue_at(&self, address: u32) -> Result<(), Exception> {
        let bounds: u64 = self.load(offset_of!(CpuState, segments.cs.base));
        let offset = address.wrapping_sub(bounds as u32);
        self.set_eip(offset);
        if offset > (bounds >> 32) as u32 {
            Err(Exception::GeneralProtection { error_code: 0 })
        } else {
            Ok(())
        }
    }

    /// Replaces a visible selector and its loaded cache without descriptor loading.
    ///
    /// # Panics
    /// Panics before writing if the cache violates this CPU's execution profile.
    #[track_caller]
    pub fn set_segment(&self, segment: Segment, value: SegmentRegister) {
        let profile = self.target.profile;
        assert!(
            profile.is_segment_compatible(segment, &value),
            "segment {segment:?} violates execution profile {profile:?}"
        );
        self.write(segment_offset(segment) as u32, &value.to_bytes());
    }

    pub fn segments(&self) -> Segments {
        Segments::from_bytes(self.read(offset_of!(CpuState, segments)))
    }

    pub fn gdtr(&self) -> DescriptorTableRegister {
        DescriptorTableRegister::from_bytes(self.read(offset_of!(CpuState, gdtr)))
    }

    pub fn set_gdtr(&self, value: DescriptorTableRegister) {
        self.write(offset_of!(CpuState, gdtr) as u32, &value.to_bytes());
    }

    pub fn idtr(&self) -> DescriptorTableRegister {
        DescriptorTableRegister::from_bytes(self.read(offset_of!(CpuState, idtr)))
    }

    pub fn set_idtr(&self, value: DescriptorTableRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_idtr(self.target.profile, value)?;
        self.write(offset_of!(CpuState, idtr) as u32, &value.to_bytes());
        Ok(())
    }

    pub fn ldtr(&self) -> SystemSegmentRegister {
        SystemSegmentRegister::from_bytes(self.read(offset_of!(CpuState, ldtr)))
    }

    pub fn set_ldtr(&self, value: SystemSegmentRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_system_segment(value, true)?;
        self.write(offset_of!(CpuState, ldtr) as u32, &value.to_bytes());
        Ok(())
    }

    pub fn tr(&self) -> SystemSegmentRegister {
        SystemSegmentRegister::from_bytes(self.read(offset_of!(CpuState, tr)))
    }

    pub fn set_tr(&self, value: SystemSegmentRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_system_segment(value, false)?;
        self.write(offset_of!(CpuState, tr) as u32, &value.to_bytes());
        Ok(())
    }

    /// Copies the complete CPU backing image, including x87 and SIMD state.
    pub fn snapshot(&self) -> CpuState {
        CpuState::from_bytes(self.read(0))
    }

    /// Restores a compatible CPU snapshot without changing the execution budget.
    ///
    /// # Errors
    /// Returns an error without writing if the snapshot violates this CPU's
    /// execution target.
    pub fn restore(&self, snapshot: &CpuState) -> Result<(), ConfigurationError> {
        crate::config::validate_cpu(self.target, snapshot)?;
        self.write(0, &snapshot.to_bytes());
        Ok(())
    }

    #[inline]
    fn load<V: MemoryValue>(&self, offset: usize) -> V
    where
        SharedMemory: Load<V>,
    {
        let address = (offset_of!(ExecutionContext, cpu) + offset) as u32;
        #[cfg(test)]
        self.reads.borrow_mut().push((address, size_of::<V>()));
        self.storage.load(address)
    }

    #[inline]
    fn store<V: MemoryValue>(&self, offset: usize, value: V)
    where
        for<'a> &'a SharedMemory: Store<V>,
    {
        self.storage
            .store((offset_of!(ExecutionContext, cpu) + offset) as u32, value);
    }

    #[inline]
    fn read<const N: usize>(&self, offset: usize) -> [u8; N] {
        let address = (offset_of!(ExecutionContext, cpu) + offset) as u32;
        #[cfg(test)]
        self.reads.borrow_mut().push((address, N));
        let mut bytes = [0; N];
        self.storage.read(address, &mut bytes);
        bytes
    }

    #[inline]
    fn write(&self, offset: u32, bytes: &[u8]) {
        self.storage
            .write(offset_of!(ExecutionContext, cpu) as u32 + offset, bytes);
    }
}

fn segment_offset(segment: Segment) -> usize {
    match segment {
        Segment::Es => offset_of!(CpuState, segments.es),
        Segment::Cs => offset_of!(CpuState, segments.cs),
        Segment::Ss => offset_of!(CpuState, segments.ss),
        Segment::Ds => offset_of!(CpuState, segments.ds),
        Segment::Fs => offset_of!(CpuState, segments.fs),
        Segment::Gs => offset_of!(CpuState, segments.gs),
    }
}

#[cfg(test)]
mod tests;
