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
    /// `relocade_x86::Segments::real_mode`; retained protected-mode caches are not admitted.
    Real16,
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
