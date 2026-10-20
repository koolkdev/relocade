//! Packed legacy x86 descriptors shared by host construction and memory lookup.

use super::{
    PrivilegeLevel, Segment, SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorInfo,
    SegmentDescriptorKind, SegmentLimit,
};
use crate::{Exception, SegmentRegister};
use std::mem::size_of;

const TYPE_MASK: u8 = 0x0f;
const ACCESSED: u8 = 0x01;
const READ_WRITE: u8 = 0x02;
const CONFORMING_EXPAND_DOWN: u8 = 0x04;
const EXECUTABLE: u8 = 0x08;
const CODE_DATA: u8 = 0x10;
const DPL_MASK: u8 = 0x60;
const PRESENT: u8 = 0x80;

const LIMIT_HIGH_MASK: u8 = 0x0f;
const FLAGS_MASK: u8 = 0xf0;
const AVAILABLE: u8 = 0x10;
const DEFAULT_32: u8 = 0x40;
const PAGE_GRANULAR: u8 = 0x80;

/// Eight-byte legacy descriptor, preserving raw table attributes. Code/data
/// construction and machine-owned table lookup share its load and query rules.
/// Named fields follow the table layout; byte conversion is explicitly little endian.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Descriptor {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    limit_and_flags: u8,
    base_high: u8,
}

impl Descriptor {
    pub const BYTE_LEN: usize = size_of::<Self>();

    pub const fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        let [limit_0, limit_1, base_0, base_1, base_middle, access, limit_and_flags, base_high] =
            bytes;
        Self {
            limit_low: u16::from_le_bytes([limit_0, limit_1]),
            base_low: u16::from_le_bytes([base_0, base_1]),
            base_middle,
            access,
            limit_and_flags,
            base_high,
        }
    }
    pub const fn to_bytes(self) -> [u8; Self::BYTE_LEN] {
        let [limit_0, limit_1] = self.limit_low.to_le_bytes();
        let [base_0, base_1] = self.base_low.to_le_bytes();
        [
            limit_0,
            limit_1,
            base_0,
            base_1,
            self.base_middle,
            self.access,
            self.limit_and_flags,
            self.base_high,
        ]
    }

    pub fn base(self) -> u32 {
        u32::from(self.base_low)
            | (u32::from(self.base_middle) << 16)
            | (u32::from(self.base_high) << 24)
    }

    pub fn limit(self) -> SegmentLimit {
        let limit =
            u32::from(self.limit_low) | (u32::from(self.limit_and_flags & LIMIT_HIGH_MASK) << 16);
        if self.limit_and_flags & PAGE_GRANULAR != 0 {
            SegmentLimit::pages(limit)
        } else {
            SegmentLimit::bytes(limit)
        }
        .expect("packed limits have twenty bits")
    }

    pub fn access_rights(self) -> u32 {
        u32::from(self.access) << 8 | u32::from(self.limit_and_flags & FLAGS_MASK) << 16
    }

    /// Only code/data descriptors have the accessed flag.
    pub fn needs_accessed_write(self) -> bool {
        self.access & (CODE_DATA | ACCESSED) == CODE_DATA
    }

    /// Sets the accessed flag on a code/data descriptor, preserving all other
    /// bits. System descriptors have no accessed flag and remain unchanged.
    #[must_use]
    pub fn with_accessed(mut self) -> Self {
        if self.access & CODE_DATA != 0 {
            self.access |= ACCESSED;
        }
        self
    }

    /// Constructs an LDT descriptor for a host-owned table of at most 64 KiB.
    pub fn ldt(base: u32, limit: u16) -> Self {
        Self {
            limit_low: limit,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access: PRESENT | DPL_MASK | 2, // LDT system type.
            limit_and_flags: 0,
            base_high: (base >> 24) as u8,
        }
    }

    /// Decodes permission information at CPL=3. Presence is reported in the
    /// access rights but does not determine LAR/LSL/VERR/VERW success.
    pub fn query_user(self) -> SegmentDescriptorInfo {
        let kind = self.access & TYPE_MASK;
        let code_data = self.access & CODE_DATA != 0;
        let executable = kind & EXECUTABLE != 0;
        let conforming = code_data && executable && kind & CONFORMING_EXPAND_DOWN != 0;
        if self.access & DPL_MASK != DPL_MASK && !conforming {
            return SegmentDescriptorInfo::default();
        }
        let lar_valid = code_data || matches!(kind, 1 | 2 | 3 | 4 | 5 | 9 | 11 | 12);
        let lsl_valid = code_data || matches!(kind, 1 | 2 | 3 | 9 | 11);
        SegmentDescriptorInfo {
            lar_valid,
            lsl_valid,
            readable: code_data && (!executable || kind & READ_WRITE != 0),
            writable: code_data && !executable && kind & READ_WRITE != 0,
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
    ) -> Result<SegmentRegister, Exception> {
        if selector & !3 == 0 {
            return match destination {
                Segment::Ss | Segment::Cs => Err(Exception::GeneralProtection { error_code: 0 }),
                _ => Ok(SegmentRegister::unusable(selector)),
            };
        }
        self.code_data()
            .ok_or(Exception::GeneralProtection {
                error_code: u32::from(selector & !3),
            })?
            .resolve_user(destination, selector)
    }

    fn code_data(self) -> Option<SegmentDescriptor> {
        if self.access & CODE_DATA == 0 {
            return None;
        }
        let kind = if self.access & EXECUTABLE != 0 {
            SegmentDescriptorKind::Code {
                readable: self.access & READ_WRITE != 0,
                conforming: self.access & CONFORMING_EXPAND_DOWN != 0,
            }
        } else {
            SegmentDescriptorKind::Data {
                writable: self.access & READ_WRITE != 0,
                expand_down: self.access & CONFORMING_EXPAND_DOWN != 0,
            }
        };
        Some(SegmentDescriptor {
            base: self.base(),
            limit: self.limit(),
            kind,
            default_size: if self.limit_and_flags & DEFAULT_32 != 0 {
                SegmentDefaultSize::Bits32
            } else {
                SegmentDefaultSize::Bits16
            },
            dpl: match (self.access & DPL_MASK) >> 5 {
                0 => PrivilegeLevel::Ring0,
                1 => PrivilegeLevel::Ring1,
                2 => PrivilegeLevel::Ring2,
                _ => PrivilegeLevel::Ring3,
            },
            present: self.access & PRESENT != 0,
            available: self.limit_and_flags & AVAILABLE != 0,
        })
    }
}

impl From<SegmentDescriptor> for Descriptor {
    fn from(descriptor: SegmentDescriptor) -> Self {
        let kind = match descriptor.kind {
            SegmentDescriptorKind::Data {
                writable,
                expand_down,
            } => ACCESSED | (u8::from(writable) << 1) | (u8::from(expand_down) << 2),
            SegmentDescriptorKind::Code {
                readable,
                conforming,
            } => ACCESSED | EXECUTABLE | (u8::from(readable) << 1) | (u8::from(conforming) << 2),
        };
        let granular = descriptor.limit.is_page_granular();
        let limit = if granular {
            descriptor.limit.effective() >> 12
        } else {
            descriptor.limit.effective()
        };
        Self {
            limit_low: limit as u16,
            base_low: descriptor.base as u16,
            base_middle: (descriptor.base >> 16) as u8,
            access: kind
                | CODE_DATA
                | ((descriptor.dpl as u8) << 5)
                | (u8::from(descriptor.present) << 7),
            limit_and_flags: (limit >> 16) as u8
                | (u8::from(descriptor.available) << 4)
                | (u8::from(descriptor.default_size == SegmentDefaultSize::Bits32) << 6)
                | (u8::from(granular) << 7),
            base_high: (descriptor.base >> 24) as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    #[test]
    fn packed_fields_and_byte_conversion_preserve_the_legacy_table_layout() {
        assert_eq!(Descriptor::BYTE_LEN, 8);
        assert_eq!(
            [
                offset_of!(Descriptor, limit_low),
                offset_of!(Descriptor, base_low),
                offset_of!(Descriptor, base_middle),
                offset_of!(Descriptor, access),
                offset_of!(Descriptor, limit_and_flags),
                offset_of!(Descriptor, base_high),
            ],
            [0, 2, 4, 5, 6, 7]
        );
        // Const conversion retains every raw bit, including L in a legacy entry.
        const RAW: [u8; 8] = [0x45, 0x23, 0xab, 0x89, 0x67, 0xb6, 0xf1, 0x45];
        const DESCRIPTOR: Descriptor = Descriptor::from_bytes(RAW);
        const ENCODED: [u8; 8] = DESCRIPTOR.to_bytes();
        assert_eq!(ENCODED, RAW);
        assert_eq!(DESCRIPTOR.base(), 0x456789ab);
        assert_eq!(DESCRIPTOR.limit().effective(), 0x12345fff);
        assert_eq!(DESCRIPTOR.access_rights(), 0x00f0b600);
    }

    #[test]
    fn accessed_updates_preserve_other_bits_and_system_descriptor_types() {
        for (access, accessed) in [
            (0xf2, 0xf3), // Data.
            (0xfa, 0xfb), // Code.
            (0x72, 0x73), // Presence does not change the meaning of A.
            (0xfb, 0xfb), // Already accessed.
            (0xe2, 0xe2), // LDT type must not change to 3.
            (0xe4, 0xe4), // Call gate type must not change to a task gate.
        ] {
            let descriptor =
                Descriptor::from_bytes([0x56, 0x34, 0x23, 0x01, 0x67, access, 0xaf, 0x89]);
            let result = descriptor.with_accessed();
            assert_eq!(
                result.to_bytes(),
                [0x56, 0x34, 0x23, 0x01, 0x67, accessed, 0xaf, 0x89]
            );
            assert!(!result.needs_accessed_write());
            assert_eq!(result.with_accessed(), result);
        }
    }

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
