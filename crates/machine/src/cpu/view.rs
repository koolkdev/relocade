use relocade_x86::{
    CpuState, DescriptorTableRegister, FlagsState, Segment, SegmentRegister, Segments,
    SystemSegmentRegister,
};
use std::mem::offset_of;

use crate::{ConfigurationError, CpuConfig, CpuOffset, CpuStorage};

/// A live view of published CPU state. Each register access transfers only its
/// own bytes; there is no cached snapshot or deferred writeback.
/// Guest execution must remain stopped for the lifetime of this view.
pub struct CpuView<'a, S: CpuStorage + ?Sized = dyn CpuStorage + 'a> {
    config: &'a CpuConfig,
    storage: &'a mut S,
}

macro_rules! registers {
    ($type:ty; $(($get:ident, $set:ident, $field:ident, $byte:expr)),* $(,)?) => {
        $(
            pub fn $get(&self) -> $type {
                self.storage.load(CpuOffset((offset_of!(CpuState, registers.$field) + $byte) as u32))
            }

            pub fn $set(&mut self, value: $type) {
                self.storage.store(
                    CpuOffset((offset_of!(CpuState, registers.$field) + $byte) as u32),
                    value,
                );
            }
        )*
    };
}

impl<'a, S: CpuStorage + ?Sized> CpuView<'a, S> {
    /// Borrows initialized CPU storage compatible with the execution target.
    pub fn new(config: &'a CpuConfig, storage: &'a mut S) -> Self {
        Self { config, storage }
    }

    registers!(u32;
        (eax, set_eax, eax, 0),
        (ecx, set_ecx, ecx, 0),
        (edx, set_edx, edx, 0),
        (ebx, set_ebx, ebx, 0),
        (esp, set_esp, esp, 0),
        (ebp, set_ebp, ebp, 0),
        (esi, set_esi, esi, 0),
        (edi, set_edi, edi, 0),
    );

    registers!(u16;
        (ax, set_ax, eax, 0),
        (cx, set_cx, ecx, 0),
        (dx, set_dx, edx, 0),
        (bx, set_bx, ebx, 0),
        (sp, set_sp, esp, 0),
        (bp, set_bp, ebp, 0),
        (si, set_si, esi, 0),
        (di, set_di, edi, 0),
    );

    registers!(u8;
        (al, set_al, eax, 0),
        (cl, set_cl, ecx, 0),
        (dl, set_dl, edx, 0),
        (bl, set_bl, ebx, 0),
        (ah, set_ah, eax, 1),
        (ch, set_ch, ecx, 1),
        (dh, set_dh, edx, 1),
        (bh, set_bh, ebx, 1),
    );

    pub fn eip(&self) -> u32 {
        self.storage
            .load(CpuOffset(offset_of!(CpuState, eip) as u32))
    }

    pub fn set_eip(&mut self, value: u32) {
        self.storage
            .store(CpuOffset(offset_of!(CpuState, eip) as u32), value);
    }

    pub fn instruction_count(&self) -> u64 {
        self.storage
            .load(CpuOffset(offset_of!(CpuState, instruction_count) as u32))
    }

    pub fn set_instruction_count(&mut self, value: u64) {
        self.storage.store(
            CpuOffset(offset_of!(CpuState, instruction_count) as u32),
            value,
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
        self.storage.write(
            CpuOffset(offset_of!(CpuState, flags) as u32),
            &flags.to_bytes(),
        );
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
        let profile = self.config.execution_target().profile;
        assert!(
            profile.is_segment_compatible(segment, &value),
            "segment {segment:?} violates execution profile {profile:?}"
        );
        self.storage
            .write(CpuOffset(segment_offset(segment) as u32), &value.to_bytes());
    }

    pub fn segments(&self) -> Segments {
        Segments::from_bytes(self.read(offset_of!(CpuState, segments)))
    }

    pub fn gdtr(&self) -> DescriptorTableRegister {
        DescriptorTableRegister::from_bytes(self.read(offset_of!(CpuState, gdtr)))
    }

    pub fn set_gdtr(&mut self, value: DescriptorTableRegister) {
        self.storage.write(
            CpuOffset(offset_of!(CpuState, gdtr) as u32),
            &value.to_bytes(),
        );
    }

    pub fn idtr(&self) -> DescriptorTableRegister {
        DescriptorTableRegister::from_bytes(self.read(offset_of!(CpuState, idtr)))
    }

    pub fn set_idtr(&mut self, value: DescriptorTableRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_idtr(self.config.execution_target().profile, value)?;
        self.storage.write(
            CpuOffset(offset_of!(CpuState, idtr) as u32),
            &value.to_bytes(),
        );
        Ok(())
    }

    pub fn ldtr(&self) -> SystemSegmentRegister {
        SystemSegmentRegister::from_bytes(self.read(offset_of!(CpuState, ldtr)))
    }

    pub fn set_ldtr(&mut self, value: SystemSegmentRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_system_segment(value, true)?;
        self.storage.write(
            CpuOffset(offset_of!(CpuState, ldtr) as u32),
            &value.to_bytes(),
        );
        Ok(())
    }

    pub fn tr(&self) -> SystemSegmentRegister {
        SystemSegmentRegister::from_bytes(self.read(offset_of!(CpuState, tr)))
    }

    pub fn set_tr(&mut self, value: SystemSegmentRegister) -> Result<(), ConfigurationError> {
        crate::config::validate_system_segment(value, false)?;
        self.storage.write(
            CpuOffset(offset_of!(CpuState, tr) as u32),
            &value.to_bytes(),
        );
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
        crate::config::validate_cpu(self.config.execution_target(), snapshot)?;
        self.storage.write(CpuOffset(0), &snapshot.to_bytes());
        Ok(())
    }

    fn read<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut bytes = [0; N];
        self.storage.read(CpuOffset(offset as u32), &mut bytes);
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
