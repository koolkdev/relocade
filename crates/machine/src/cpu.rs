use relocade_x86::{
    CpuState, DescriptorTableRegister, ExecutionContext, ExecutionTarget, FlagsState, Segment,
    SegmentRegister, Segments, SystemSegmentRegister,
};
use std::{marker::PhantomData, mem::offset_of, rc::Rc};

use crate::{ConfigurationError, CpuConfig};
use relocade_wasm_runtime::{Load, MemoryValue, SharedMemory, Store, WasmSharedMemory};

/// Retained access to the complete live CPU state, independent of the backend.
/// Operations copy values and apply writes immediately; no references into Wasm
/// storage escape. The execution driver serializes guest execution and service
/// polling. Generated entries may cache fields until they publish state on exit;
/// use this resource at stopped host boundaries. [`Self::instruction_count`] also
/// permits approximate progress observations from synchronous device callbacks.
/// Clones share state and remain usable after the runner is dropped.
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

pub mod registers;
mod saved_registers;
pub use saved_registers::SavedRegisters;

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

    #[inline]
    pub fn eip(&self) -> u32 {
        self.load(offset_of!(CpuState, eip))
    }

    #[inline]
    pub fn set_eip(&self, value: u32) {
        self.store(offset_of!(CpuState, eip), value);
    }

    /// Published retirement count. During a synchronous device callback this can
    /// omit progress cached by generated execution until its next publication.
    #[inline]
    pub fn instruction_count(&self) -> u64 {
        self.load(offset_of!(CpuState, instruction_count))
    }

    #[inline]
    pub fn interrupts_enabled(&self) -> bool {
        self.load::<u8>(offset_of!(CpuState, flags.bytes.if_)) & 1 != 0
    }

    /// Changes live IF without touching arithmetic flags or creating STI's shadow.
    #[inline]
    pub fn set_interrupts_enabled(&self, enabled: bool) {
        self.store(offset_of!(CpuState, flags.bytes.if_), u8::from(enabled));
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

    /// Replaces a visible selector and its loaded cache without descriptor loading.
    ///
    /// # Panics
    /// Panics before writing if the cache violates this CPU's execution profile.
    #[track_caller]
    pub fn set_segment(&self, segment: Segment, value: SegmentRegister) {
        let profile = self.target.profile;
        assert!(
            value.is_compatible_with(profile, segment),
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

    /// Restores architectural state without changing cumulative retirement or
    /// execution controls, including the pending interrupt and instruction target.
    /// The snapshot's instruction count is informational; initialize a new CPU
    /// through [`CpuConfig`] to start a different execution history.
    ///
    /// # Errors
    /// Returns an error without writing if the snapshot violates this CPU's
    /// execution target.
    pub fn restore(&self, snapshot: &CpuState) -> Result<(), ConfigurationError> {
        crate::config::validate_cpu(self.target, snapshot)?;
        let mut restored = *snapshot;
        restored.instruction_count = self.instruction_count();
        self.write(0, &restored.to_bytes());
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
        let mut bytes = [0; N];
        self.read_into(offset, &mut bytes);
        bytes
    }

    #[inline]
    fn read_into(&self, offset: usize, bytes: &mut [u8]) {
        let address = (offset_of!(ExecutionContext, cpu) + offset) as u32;
        #[cfg(test)]
        self.reads.borrow_mut().push((address, bytes.len()));
        self.storage.read(address, bytes);
    }

    #[inline]
    fn write(&self, offset: u32, bytes: &[u8]) {
        self.storage
            .write(offset_of!(ExecutionContext, cpu) as u32 + offset, bytes);
    }
}

const fn segment_offset(segment: Segment) -> usize {
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
