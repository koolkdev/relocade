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
pub use relocade_x86_profile::SegmentDefaultSize;
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
