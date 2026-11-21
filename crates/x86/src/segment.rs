//! Segment identities, access checks, and host descriptor-table resolution.

mod access;
pub(crate) mod cache;
mod descriptor;
mod limit;
mod packed;
pub(crate) mod profile;
mod query;
mod selection;
mod tables;

pub(crate) use access::SegmentAccess;
pub(crate) use cache::SegmentCacheValues;
pub use descriptor::{PrivilegeLevel, SegmentDescriptor, SegmentDescriptorKind};
pub use limit::SegmentLimit;
pub use packed::Descriptor;
pub use profile::SegmentProfile;
pub use query::{SegmentDescriptorInfo, SegmentDescriptorInfoValues};
pub(crate) use selection::SegmentSelection;
pub use tables::DescriptorTables;

/// Segment registers in x86 encoding order, independent of their backing offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Segment {
    Es,
    Cs,
    Ss,
    Ds,
    Fs,
    Gs,
}

impl Segment {
    pub const ALL: [Self; 6] = [Self::Es, Self::Cs, Self::Ss, Self::Ds, Self::Fs, Self::Gs];
}

/// The D/B attribute, independent of a segment's base and effective limit.
/// It selects CS instruction defaults, SS stack-pointer width, or the upper
/// bound of an expand-down data segment. It does not size an ordinary data segment.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SegmentDefaultSize {
    Bits16,
    Bits32,
}

impl SegmentDefaultSize {
    pub(crate) fn for_profile(profile: crate::ExecutionProfile) -> Self {
        match profile {
            crate::ExecutionProfile::Protected(
                SegmentProfile::Flat32 | SegmentProfile::Segmented32,
            ) => Self::Bits32,
            crate::ExecutionProfile::Protected(SegmentProfile::Segmented16)
            | crate::ExecutionProfile::Real16 => Self::Bits16,
        }
    }
}
