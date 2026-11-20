//! Entry guarantees and memory routing shared by x86 code generation and embedded interpreters.

#![forbid(unsafe_code)]

/// Architectural mode and privilege established by the execution owner.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExecutionMode {
    Real,
    /// Protected mode with a ring-3 code selector.
    ProtectedUser,
}

/// Guarantees about the loaded segment caches, independent of instruction width.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SegmentAssumptions {
    /// Ordinary real-mode caches, including selector-shifted bases and 64 KiB limits.
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

/// Facts guaranteed on entry to generated code. CPU admission checks the same
/// facts that instruction decoding and execution may fold into constants.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EntryAssumptions {
    pub mode: ExecutionMode,
    /// Guaranteed segment cache layout. With `None`, cache fields stay live
    /// except for CS.D when [`Self::code_default_size`] fixes it.
    pub segments: Option<SegmentAssumptions>,
    /// An explicit guarantee for CS.D. Canonical real-mode caches also imply 16 bits.
    /// When CS.D is live, the interpreter reads it and a snapshot guards its decode width.
    pub code_default_size: Option<SegmentDefaultSize>,
}

impl EntryAssumptions {
    pub const REAL_MODE: Self = Self {
        mode: ExecutionMode::Real,
        segments: Some(SegmentAssumptions::RealMode),
        code_default_size: Some(SegmentDefaultSize::Bits16),
    };
    pub const FLAT32: Self = Self {
        mode: ExecutionMode::ProtectedUser,
        segments: Some(SegmentAssumptions::Flat),
        code_default_size: Some(SegmentDefaultSize::Bits32),
    };
    pub const SEGMENTED32: Self = Self {
        segments: None,
        ..Self::FLAT32
    };
    pub const SEGMENTED16: Self = Self {
        code_default_size: Some(SegmentDefaultSize::Bits16),
        ..Self::SEGMENTED32
    };
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
