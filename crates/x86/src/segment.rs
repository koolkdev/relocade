//! Segment identities, loaded caches, and host descriptor-table resolution.

mod access;
mod descriptor;
mod limit;
mod profile;
mod query;
mod selection;
mod tables;

use wasm86_compiler::{Val, I1, I16, I32};

pub use descriptor::{PrivilegeLevel, SegmentDescriptor, SegmentDescriptorKind};
pub use limit::SegmentLimit;
pub use profile::SegmentProfile;
pub use query::SegmentDescriptorInfo;
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

/// The D/B attribute, independent of a segment's base and effective limit.
/// It selects CS instruction defaults, SS stack-pointer width, or the upper
/// bound of an expand-down data segment. It does not size an ordinary data segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentDefaultSize {
    Bits16,
    Bits32,
}
