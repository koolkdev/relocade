//! Execution profiles shared by x86 code generation and embedded interpreter modules.
//! These values describe execution assumptions without depending on a compiler.

#![forbid(unsafe_code)]

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
