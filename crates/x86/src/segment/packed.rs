//! Packed legacy x86 descriptors shared by host construction and memory lookup.

use super::{
    PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorInfo,
    SegmentDescriptorKind, SegmentLimit,
};
use crate::{Exception, StoredSegment};

/// Eight-byte legacy descriptor, preserving raw table attributes. Code/data
/// construction and machine-owned table lookup share its load and query rules.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Descriptor([u8; 8]);

impl Descriptor {
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }
    pub const fn to_bytes(self) -> [u8; 8] {
        self.0
    }

    pub fn base(self) -> u32 {
        u32::from_le_bytes([self.0[2], self.0[3], self.0[4], self.0[7]])
    }

    pub fn limit(self) -> SegmentLimit {
        let limit = u32::from(u16::from_le_bytes([self.0[0], self.0[1]]))
            | (u32::from(self.0[6] & 15) << 16);
        if self.0[6] & 0x80 != 0 {
            SegmentLimit::pages(limit)
        } else {
            SegmentLimit::bytes(limit)
        }
        .expect("packed limits have twenty bits")
    }

    pub fn access_rights(self) -> u32 {
        u32::from(self.0[5]) << 8 | u32::from(self.0[6] & 0xf0) << 16
    }

    /// Only code/data descriptors have the accessed flag.
    pub fn needs_accessed_write(self) -> bool {
        self.0[5] & 0x11 == 0x10
    }

    /// Constructs an LDT descriptor for a host-owned table of at most 64 KiB.
    pub fn ldt(base: u32, limit: u16) -> Self {
        let [b0, b1, b2, b3] = base.to_le_bytes();
        let [l0, l1] = limit.to_le_bytes();
        Self([l0, l1, b0, b1, b2, 0xe2, 0, b3])
    }

    /// Decodes permission information at CPL=3. Presence is reported in the
    /// access rights but does not determine LAR/LSL/VERR/VERW success.
    pub fn query_user(self) -> SegmentDescriptorInfo {
        let access = self.0[5];
        let kind = access & 15;
        let code_data = access & 0x10 != 0;
        let conforming = code_data && kind & 12 == 12;
        if access & 0x60 != 0x60 && !conforming {
            return SegmentDescriptorInfo::default();
        }
        let lar_valid = code_data || matches!(kind, 1 | 2 | 3 | 4 | 5 | 9 | 11 | 12);
        let lsl_valid = code_data || matches!(kind, 1 | 2 | 3 | 9 | 11);
        SegmentDescriptorInfo {
            lar_valid,
            lsl_valid,
            readable: code_data && (kind & 8 == 0 || kind & 2 != 0),
            writable: code_data && kind & 10 == 2,
            access_rights: if lar_valid { self.access_rights() } else { 0 },
            limit: if lsl_valid {
                self.limit().effective()
            } else {
                0
            },
        }
    }

    /// Checks a direct code/data load at CPL=3 without changing the descriptor
    /// or loaded cache. Null selectors ignore the descriptor bytes entirely.
    /// Gates and privilege transitions are not implemented.
    pub fn resolve_user_segment(
        self,
        destination: Segment,
        selector: u16,
    ) -> Result<StoredSegment, Exception> {
        if selector & !3 == 0 {
            return match destination {
                Segment::Ss | Segment::Cs => Err(Exception::GeneralProtection { error_code: 0 }),
                _ => Ok(StoredSegment::unusable(selector)),
            };
        }
        self.code_data()
            .ok_or(Exception::GeneralProtection {
                error_code: u32::from(selector & !3),
            })?
            .resolve_user(destination, selector)
    }

    fn code_data(self) -> Option<SegmentDescriptor> {
        let access = self.0[5];
        if access & 0x10 == 0 {
            return None;
        }
        let kind = if access & 8 != 0 {
            SegmentDescriptorKind::Code {
                readable: access & 2 != 0,
                conforming: access & 4 != 0,
            }
        } else {
            SegmentDescriptorKind::Data {
                writable: access & 2 != 0,
                expand_down: access & 4 != 0,
            }
        };
        Some(SegmentDescriptor {
            base: self.base(),
            limit: self.limit(),
            kind,
            default_size: if self.0[6] & 0x40 != 0 {
                SegmentDefaultSize::Bits32
            } else {
                SegmentDefaultSize::Bits16
            },
            dpl: match (access >> 5) & 3 {
                0 => PrivilegeLevel::Ring0,
                1 => PrivilegeLevel::Ring1,
                2 => PrivilegeLevel::Ring2,
                _ => PrivilegeLevel::Ring3,
            },
            present: access & 0x80 != 0,
            available: self.0[6] & 0x10 != 0,
        })
    }
}

impl From<SegmentDescriptor> for Descriptor {
    fn from(descriptor: SegmentDescriptor) -> Self {
        let kind = match descriptor.kind {
            SegmentDescriptorKind::Data {
                writable,
                expand_down,
            } => 1 | (u8::from(writable) << 1) | (u8::from(expand_down) << 2),
            SegmentDescriptorKind::Code {
                readable,
                conforming,
            } => 9 | (u8::from(readable) << 1) | (u8::from(conforming) << 2),
        };
        let [b0, b1, b2, b3] = descriptor.base.to_le_bytes();
        let granular = descriptor.limit.is_page_granular();
        let limit = if granular {
            descriptor.limit.effective() >> 12
        } else {
            descriptor.limit.effective()
        };
        let [l0, l1, l2, _] = limit.to_le_bytes();
        Self([
            l0,
            l1,
            b0,
            b1,
            b2,
            kind | 0x10 | ((descriptor.dpl as u8) << 5) | (u8::from(descriptor.present) << 7),
            l2 | (u8::from(descriptor.available) << 4)
                | (u8::from(descriptor.default_size == SegmentDefaultSize::Bits32) << 6)
                | (u8::from(granular) << 7),
            b3,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_and_queries_retain_architectural_descriptor_bits() {
        let descriptor = Descriptor::from(SegmentDescriptor::new(
            0x12345678,
            SegmentLimit::bytes(0x34567).unwrap(),
            SegmentDescriptorKind::Data {
                writable: true,
                expand_down: false,
            },
            SegmentDefaultSize::Bits32,
        ));
        assert_eq!(
            descriptor.to_bytes(),
            [0x67, 0x45, 0x78, 0x56, 0x34, 0xf3, 0x43, 0x12]
        );
        assert_eq!(descriptor.base(), 0x12345678);
        assert_eq!(descriptor.limit().effective(), 0x34567);
        assert_eq!(descriptor.query_user().access_rights, 0x0040f300);
        let raw = Descriptor::from_bytes([0xff, 0xff, 0, 0, 0, 0x72, 0x8f, 0]);
        assert_eq!(raw.query_user().access_rights, 0x00807200); // A=0, P=0, G=1.
        assert_eq!(raw.query_user().limit, u32::MAX);
        assert!(raw.query_user().writable);
        assert!(raw.needs_accessed_write());
        assert_eq!(
            raw.resolve_user_segment(Segment::Ds, 0x23),
            Err(Exception::SegmentNotPresent { error_code: 0x20 })
        );
    }

    #[test]
    fn system_type_eligibility_is_distinct_from_code_data_permissions() {
        for (access, lar, lsl) in [
            (0xe0, false, false),
            (0xe2, true, true),
            (0xe4, true, false),
            (0xe5, true, false),
            (0xe9, true, true),
            (0xeb, true, true),
            (0xec, true, false),
            (0xee, false, false),
        ] {
            let descriptor = Descriptor::from_bytes([0x34, 0x12, 0, 0, 0, access, 0, 0]);
            let query = descriptor.query_user();
            assert_eq!(
                (
                    query.lar_valid,
                    query.lsl_valid,
                    query.readable,
                    query.writable
                ),
                (lar, lsl, false, false)
            );
            assert_eq!(
                descriptor.resolve_user_segment(Segment::Ds, 0x23),
                Err(Exception::GeneralProtection { error_code: 0x20 })
            );
        }
    }
}
