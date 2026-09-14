//! Host descriptors and protected-mode user-level load rules.

use crate::{Exception, StoredSegment};

use super::{Segment, SegmentAttributes, SegmentDefaultSize, SegmentKind};

/// Architectural privilege levels, ordered from most to least privileged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PrivilegeLevel {
    Ring0,
    Ring1,
    Ring2,
    Ring3,
}

/// Code/data descriptor types supported by user-mode segment resolution.
/// Conformance affects loading a code descriptor, not ordinary cached accesses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentDescriptorKind {
    Data { writable: bool, expand_down: bool },
    Code { readable: bool, conforming: bool },
}

/// A host-managed descriptor, separate from any CPU's loaded segment cache.
/// Limits are inclusive effective byte limits; this is not the packed x86
/// descriptor format. System descriptors and gates are outside this model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentDescriptor {
    pub base: u32,
    pub limit: u32,
    pub kind: SegmentDescriptorKind,
    pub default_size: SegmentDefaultSize,
    pub dpl: PrivilegeLevel,
    pub present: bool,
}

impl SegmentDescriptor {
    /// Constructs a present descriptor with DPL=3.
    pub const fn new(
        base: u32,
        limit: u32,
        kind: SegmentDescriptorKind,
        default_size: SegmentDefaultSize,
    ) -> Self {
        Self {
            base,
            limit,
            kind,
            default_size,
            dpl: PrivilegeLevel::Ring3,
            present: true,
        }
    }

    pub(super) fn resolve_user(
        &self,
        destination: Segment,
        selector: u16,
    ) -> Result<StoredSegment, Exception> {
        use SegmentDescriptorKind::{Code, Data};
        let accessible = match (destination, self.kind) {
            (Segment::Ss, Data { writable: true, .. }) => {
                self.dpl == PrivilegeLevel::Ring3 && selector & 3 == 3
            }
            (Segment::Cs, Code { conforming, .. }) => {
                conforming || self.dpl == PrivilegeLevel::Ring3
            }
            (Segment::Ss | Segment::Cs, _) => false,
            (
                _,
                Code {
                    readable: false, ..
                },
            ) => false,
            (
                _,
                Code {
                    conforming: true, ..
                },
            ) => true,
            (_, _) => self.dpl == PrivilegeLevel::Ring3,
        };
        let error_code = u32::from(selector & !3);
        if !accessible {
            return Err(Exception::GeneralProtection { error_code });
        }
        if !self.present {
            return Err(if destination == Segment::Ss {
                Exception::StackFault { error_code }
            } else {
                Exception::SegmentNotPresent { error_code }
            });
        }
        let kind = match self.kind {
            Data {
                writable,
                expand_down,
            } => SegmentKind::Data {
                writable,
                expand_down,
            },
            Code { readable, .. } => SegmentKind::Code { readable },
        };
        Ok(StoredSegment {
            base: self.base,
            limit: self.limit,
            selector: if destination == Segment::Cs {
                selector | 3
            } else {
                selector
            },
            attributes: SegmentAttributes::new(kind, self.default_size),
        })
    }
}
