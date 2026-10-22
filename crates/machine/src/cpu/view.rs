use relocade_x86::{
    CpuState, DescriptorTableRegister, ExecutionTarget, FlagsState, Segment, SegmentRegister,
    Segments, SystemSegmentRegister,
};
use std::mem::offset_of;

use crate::{ConfigurationError, CpuConfig, CpuStorage};

/// A live view of published CPU state. Each register access transfers only its
/// own bytes; there is no cached snapshot or deferred writeback.
/// Guest execution must remain stopped for the lifetime of this view.
pub struct CpuView<'a, S: CpuStorage + ?Sized = dyn CpuStorage + 'a> {
    target: ExecutionTarget,
    storage: &'a mut S,
}

macro_rules! register {
    ($get:ident, $set:ident, $type:ty, $field:ident, $byte:expr) => {
        pub fn $get(&self) -> $type {
            <$type>::from_le_bytes(self.read(offset_of!(CpuState, registers.$field) + $byte))
        }

        pub fn $set(&mut self, value: $type) {
            self.storage.write_cpu(
                (offset_of!(CpuState, registers.$field) + $byte) as u32,
                &value.to_le_bytes(),
            );
        }
    };
}

impl<'a, S: CpuStorage + ?Sized> CpuView<'a, S> {
    /// Borrows initialized CPU storage compatible with the execution target.
    pub fn new(config: &CpuConfig, storage: &'a mut S) -> Self {
        Self::for_target(config.execution_target(), storage)
    }

    pub(crate) fn for_target(target: ExecutionTarget, storage: &'a mut S) -> Self {
        Self { target, storage }
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

    pub fn eip(&self) -> u32 {
        u32::from_le_bytes(self.read(offset_of!(CpuState, eip)))
    }

    pub fn set_eip(&mut self, value: u32) {
        self.storage
            .write_cpu(offset_of!(CpuState, eip) as u32, &value.to_le_bytes());
    }

    pub fn instruction_count(&self) -> u64 {
        u64::from_le_bytes(self.read(offset_of!(CpuState, instruction_count)))
    }

    pub fn set_instruction_count(&mut self, value: u64) {
        self.storage.write_cpu(
            offset_of!(CpuState, instruction_count) as u32,
            &value.to_le_bytes(),
        );
    }

    /// Returns represented EFLAGS bits, resolving any lazy arithmetic flags.
    pub fn eflags(&self) -> u32 {
        FlagsState::from_bytes(self.read(offset_of!(CpuState, flags))).eflags()
    }

    /// Replaces represented flags. This is host editing, not a privilege-checked
    /// POPFD operation; fixed and unrepresented bits are ignored.
    pub fn set_eflags(&mut self, value: u32) {
        let mut flags = FlagsState::from_bytes(self.read(offset_of!(CpuState, flags)));
        flags.set_eflags(value);
        self.storage
            .write_cpu(offset_of!(CpuState, flags) as u32, &flags.to_bytes());
    }

    pub fn carry(&self) -> bool {
        self.eflags() & 1 != 0
    }

    /// Preserves the other flags, including those held in a lazy source.
    pub fn set_carry(&mut self, value: bool) {
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
    pub fn set_segment(&mut self, segment: Segment, value: SegmentRegister) {
        let profile = self.target.profile;
        assert!(
            profile.is_segment_compatible(segment, &value),
            "segment {segment:?} violates execution profile {profile:?}"
        );
        self.storage
            .write_cpu(segment_offset(segment) as u32, &value.to_bytes());
    }

    pub fn segments(&self) -> Segments {
        Segments::from_bytes(self.read(offset_of!(CpuState, segments)))
    }

    pub fn gdtr(&self) -> DescriptorTableRegister {
        DescriptorTableRegister::from_bytes(self.read(offset_of!(CpuState, gdtr)))
    }

    pub fn set_gdtr(&mut self, value: DescriptorTableRegister) {
        self.storage
            .write_cpu(offset_of!(CpuState, gdtr) as u32, &value.to_bytes());
    }

    pub fn idtr(&self) -> DescriptorTableRegister {
        DescriptorTableRegister::from_bytes(self.read(offset_of!(CpuState, idtr)))
    }

    pub fn set_idtr(&mut self, value: DescriptorTableRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_idtr(self.target.profile, value)?;
        self.storage
            .write_cpu(offset_of!(CpuState, idtr) as u32, &value.to_bytes());
        Ok(())
    }

    pub fn ldtr(&self) -> SystemSegmentRegister {
        SystemSegmentRegister::from_bytes(self.read(offset_of!(CpuState, ldtr)))
    }

    pub fn set_ldtr(&mut self, value: SystemSegmentRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_system_segment(value, true)?;
        self.storage
            .write_cpu(offset_of!(CpuState, ldtr) as u32, &value.to_bytes());
        Ok(())
    }

    pub fn tr(&self) -> SystemSegmentRegister {
        SystemSegmentRegister::from_bytes(self.read(offset_of!(CpuState, tr)))
    }

    pub fn set_tr(&mut self, value: SystemSegmentRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_system_segment(value, false)?;
        self.storage
            .write_cpu(offset_of!(CpuState, tr) as u32, &value.to_bytes());
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
    pub fn restore(&mut self, snapshot: &CpuState) -> Result<(), ConfigurationError> {
        crate::config::validate_cpu(self.target, snapshot)?;
        self.storage.write_cpu(0, &snapshot.to_bytes());
        Ok(())
    }

    fn read<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut bytes = [0; N];
        self.storage.read_cpu(offset as u32, &mut bytes);
        bytes
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
