//! Segment identities, access checks, and host descriptor-table resolution.

mod access;
mod descriptor;
mod limit;
mod packed;
pub(crate) mod profile;
mod query;
mod selection;
mod tables;

use relocade_compiler::{Val, I1, I16, I32};

pub(crate) use access::SegmentAccess;
pub use descriptor::{PrivilegeLevel, SegmentDescriptor, SegmentDescriptorKind};
pub use limit::SegmentLimit;
pub use packed::Descriptor;
pub use profile::SegmentProfile;
pub use query::{SegmentDescriptorInfo, SegmentDescriptorInfoValues};
pub(crate) use selection::SegmentSelection;
pub use tables::DescriptorTables;

/// Values of the loaded cache. Permission fields are logical booleans;
/// their backing bytes and the visible selector remain separate state fields.
#[derive(Clone)]
pub(crate) struct SegmentValues {
    pub(crate) base: Val<I32>,
    pub(crate) limit: Val<I32>,
    pub(crate) selector: Val<I16>,
    pub(crate) usable: Val<I1>,
    pub(crate) readable: Val<I1>,
    pub(crate) writable: Val<I1>,
    pub(crate) executable: Val<I1>,
    pub(crate) expand_down: Val<I1>,
    pub(crate) big: Val<I1>,
}

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

pub use relocade_x86_profile::SegmentDefaultSize;
