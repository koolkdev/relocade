//! Architectural entry facts and memory routing shared by x86 generators and runtimes.

#![forbid(unsafe_code)]

/// Architectural mode and privilege guaranteed on entry. Both modes require VM to be clear.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExecutionMode {
    Real,
    /// Protected mode with a ring-3 code selector.
    ProtectedUser,
}

/// Guarantees about loaded segment caches, independent of the current execution mode.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SegmentAssumptions {
    /// Canonical selector-shifted bases, 64 KiB limits and 16-bit defaults.
    RealMode,
    /// Flat CS, DS, ES and SS with a 32-bit stack. FS and GS remain live.
    Flat,
}

/// The D/B attribute of CS or SS, or the upper bound of an expand-down data segment.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SegmentDefaultSize {
    Bits16,
    Bits32,
}

/// Architectural privilege levels, ordered from most to least privileged.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum PrivilegeLevel {
    Ring0,
    Ring1,
    Ring2,
    Ring3,
}

impl PrivilegeLevel {
    /// Selectors and IOPL encode a privilege level in two bits.
    pub const MASK: u8 = 0b11;
}

/// Facts the owner guarantees on every entry to generated code. An omitted fact
/// is read from the executing CPU unless another guarantee implies it. The
/// owner maintains these facts across dispatch, direct links and interpreter
/// handoffs. A guest change to a guaranteed field can invalidate old entries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntryAssumptions {
    pub mode: Option<ExecutionMode>,
    pub segments: Option<SegmentAssumptions>,
    /// Canonical real-mode caches also require CS.D to be clear.
    pub code_default_size: Option<SegmentDefaultSize>,
    /// Guarantees CR4.TSD is clear. Otherwise timestamp access reads the live control bit.
    pub cr4_tsd_clear: bool,
    /// The value of EFLAGS.IF on entry.
    pub interrupt_flag: Option<bool>,
    /// The value of EFLAGS.IOPL on entry.
    pub iopl: Option<PrivilegeLevel>,
}

impl EntryAssumptions {
    /// The physical PC interpreter reads all these fields from the CPU.
    pub const LIVE: Self = Self {
        mode: None,
        segments: None,
        code_default_size: None,
        cr4_tsd_clear: false,
        interrupt_flag: None,
        iopl: None,
    };

    /// Ordinary real mode with canonical segment caches and live interrupt flags.
    pub const REAL_MODE: Self = Self {
        mode: Some(ExecutionMode::Real),
        segments: Some(SegmentAssumptions::RealMode),
        code_default_size: Some(SegmentDefaultSize::Bits16),
        cr4_tsd_clear: true,
        ..Self::LIVE
    };

    /// Hosted ring-3 code with flat segments, 32-bit defaults, IF set and IOPL zero.
    pub const FLAT32: Self = Self {
        mode: Some(ExecutionMode::ProtectedUser),
        segments: Some(SegmentAssumptions::Flat),
        code_default_size: Some(SegmentDefaultSize::Bits32),
        cr4_tsd_clear: true,
        interrupt_flag: Some(true),
        iopl: Some(PrivilegeLevel::Ring0),
    };

    /// Hosted ring-3 code with 32-bit defaults and live segment caches.
    pub const SEGMENTED32: Self = Self {
        segments: None,
        ..Self::FLAT32
    };

    /// Hosted ring-3 code with 16-bit defaults and live segment caches.
    pub const SEGMENTED16: Self = Self {
        code_default_size: Some(SegmentDefaultSize::Bits16),
        ..Self::SEGMENTED32
    };

    /// The instruction width established by the entry facts, if any.
    pub const fn fixed_code_default_size(self) -> Option<SegmentDefaultSize> {
        match (self.code_default_size, self.segments) {
            (None, Some(SegmentAssumptions::RealMode)) => Some(SegmentDefaultSize::Bits16),
            (size, _) => size,
        }
    }
}

/// How generated x86 code resolves a linear address after checking the segment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MemoryBackend {
    /// The PC physical bus with guest paging disabled. A20 and physical page routing
    /// select RAM, ROM, MMIO or unmapped space for both instruction and data accesses.
    PhysicalBus,
    /// The host's linear page table maps guest addresses to backing pages and permissions.
    HostPageTable,
}
