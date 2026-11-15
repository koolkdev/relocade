//! Masked state requirements shared by code generation and entry admission.

use super::ExecutionContext;
use crate::ssa::{Location, StateFields};
use relocade_compiler::{BlockBuilder, BuildError, I16, I32, I64, I8};
use std::mem::{offset_of, size_of};

/// A requirement on scalar bytes in the CPU snapshot, in its encoded byte order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct StateAssumption {
    offset: usize,
    byte_len: u8,
    mask: u64,
    expected: u64,
}

impl StateAssumption {
    /// The field selector supplies its storage type without reading a snapshot.
    pub(crate) fn field<Record, T: Into<u64>>(
        offset: usize,
        _field: fn(&Record) -> &T,
        expected: T,
    ) -> Self {
        Self::new(offset, size_of::<T>(), u64::MAX, expected.into())
    }

    pub(crate) fn bits<Record, T: Into<u64>>(
        offset: usize,
        _field: fn(&Record) -> &T,
        mask: T,
        expected: T,
    ) -> Self {
        Self::new(offset, size_of::<T>(), mask.into(), expected.into())
    }

    fn new(offset: usize, byte_len: usize, mask: u64, expected: u64) -> Self {
        assert!(matches!(byte_len, 1 | 2 | 4 | 8));
        let field_bits = byte_len as u32 * u8::BITS;
        let field_mask = u64::MAX >> (u64::BITS - field_bits);
        Self {
            offset,
            byte_len: byte_len as u8,
            mask: mask & field_mask,
            expected: expected & mask & field_mask,
        }
    }

    pub(crate) fn matches_record(self, record: &[u8], record_offset: usize) -> bool {
        let field_start = self.offset - record_offset;
        let field_end = field_start + usize::from(self.byte_len);
        let mut encoded = [0; size_of::<u64>()];
        encoded[..usize::from(self.byte_len)].copy_from_slice(&record[field_start..field_end]);
        u64::from_le_bytes(encoded) & self.mask == self.expected
    }

    /// Admission and state-change owners must maintain this requirement.
    pub(crate) fn apply(
        self,
        body: &mut BlockBuilder<'_>,
        fields: &mut StateFields,
    ) -> Result<(), BuildError> {
        let offset = (offset_of!(ExecutionContext, cpu) + self.offset) as u32;
        match self.byte_len {
            1 => fields.assume_bits(
                body,
                Location::<I8>::new(offset),
                self.mask as u32,
                self.expected as u32,
            ),
            2 => fields.assume_bits(
                body,
                Location::<I16>::new(offset),
                self.mask as u32,
                self.expected as u32,
            ),
            4 => fields.assume_bits(
                body,
                Location::<I32>::new(offset),
                self.mask as u32,
                self.expected as u32,
            ),
            8 => fields.assume_bits(body, Location::<I64>::new(offset), self.mask, self.expected),
            _ => unreachable!("assumptions have scalar storage widths"),
        }
    }
}
