//! Execution profiles shared by x86 code generation and embedded interpreter modules.
//! These values describe execution assumptions without depending on a compiler.

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
    /// CS.D when fixed by the caller. Otherwise decoding reads the live cache,
    /// and a compiled snapshot guards the width used to decode its bytes.
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

/// Execution assumptions under which a compiled entry may execute.
/// Compatibility does not validate instruction-fetch spans, code bytes or mappings and
/// does not perform cache invalidation. The execution owner must invalidate
/// dependent entries and dispatch links when these assumptions cease to hold.
/// The runtime owns compatibility across segment changes, including any profile
/// transition. Dispatch and interpreter entry rely on the runtime maintaining it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExecutionProfile {
    /// Protected mode at CPL3 under the selected segment assumptions, with
    /// timestamp reads enabled (CR4.TSD=0).
    Protected(SegmentProfile),
    /// Ordinary real mode: 16-bit defaults, selector-shifted bases, 64 KiB limits
    /// and host-controlled A20. The host establishes canonical caches with
    /// `relocade_x86::CpuState::real_mode`; retained protected-mode caches are not admitted.
    Real16,
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

impl MemoryBackend {
    /// The backend used by the packaged interpreter for this execution profile.
    pub const fn default_for(profile: ExecutionProfile) -> Self {
        match profile {
            ExecutionProfile::Real16 => Self::PhysicalBus,
            ExecutionProfile::Protected(_) => Self::HostPageTable,
        }
    }
}

/// Protected-mode segment assumptions fixed when an entry is compiled.
/// Used by [`crate::ExecutionProfile::Protected`]; 16-bit code defaults do not
/// select real mode. Entry validity and invalidation follow [`crate::ExecutionProfile`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SegmentProfile {
    /// Flat readable code CS, writable expand-up DS/ES/SS, 32-bit CS defaults and
    /// a 32-bit stack pointer. FS and GS have no assumptions in this profile;
    /// accesses through them use runtime segment handling.
    Flat32,
    /// 32-bit instruction defaults. Segment access and SS.B are runtime inputs.
    Segmented32,
    /// 16-bit instruction defaults. Segment access and SS.B are runtime inputs.
    Segmented16,
}

impl From<SegmentProfile> for ExecutionProfile {
    fn from(profile: SegmentProfile) -> Self {
        Self::Protected(profile)
    }
}

impl From<ExecutionProfile> for EntryAssumptions {
    fn from(profile: ExecutionProfile) -> Self {
        match profile {
            ExecutionProfile::Real16 => Self::REAL_MODE,
            ExecutionProfile::Protected(segments) => segments.into(),
        }
    }
}

impl From<SegmentProfile> for EntryAssumptions {
    fn from(profile: SegmentProfile) -> Self {
        match profile {
            SegmentProfile::Flat32 => Self::FLAT32,
            SegmentProfile::Segmented32 => Self::SEGMENTED32,
            SegmentProfile::Segmented16 => Self::SEGMENTED16,
        }
    }
}
