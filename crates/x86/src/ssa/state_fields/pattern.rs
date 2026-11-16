//! Masked field values shared by admission, guards and retained state.

use super::{Address, Location, LocationKey, SsaType, StateFields};
use relocade_compiler::{BlockBuilder, BuildError, IntType, I16, I32, I64, I8};

/// Describes bits of one scalar field, independently of how they are established.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct StatePattern {
    pub(super) location: LocationKey,
    pub(super) mask: u64,
    pub(super) expected: u64,
}

impl StatePattern {
    pub(crate) fn field<T: SsaType + IntType>(location: Location<T>, expected: u64) -> Self {
        Self::bits(location, u64::MAX, expected)
    }

    pub(crate) fn bits<T: SsaType + IntType>(
        location: Location<T>,
        mask: u64,
        expected: u64,
    ) -> Self {
        let Address::Fixed(offset) = location.address else {
            panic!("state patterns require a fixed field")
        };
        let mask = mask & (u64::MAX >> (u64::BITS - T::BYTES * u8::BITS));
        Self {
            location: LocationKey {
                offset,
                bytes: T::BYTES,
            },
            mask,
            expected: expected & mask,
        }
    }

    pub(crate) fn matches_record(self, record: &[u8], record_offset: usize) -> bool {
        let start = self.location.offset as usize - record_offset;
        let bytes = self.location.bytes as usize;
        let mut encoded = [0; size_of::<u64>()];
        encoded[..bytes].copy_from_slice(&record[start..start + bytes]);
        u64::from_le_bytes(encoded) & self.mask == self.expected
    }

    /// The caller establishes these bits by admission or a guard on this path.
    pub(crate) fn apply(
        self,
        body: &mut BlockBuilder<'_>,
        fields: &mut StateFields,
    ) -> Result<(), BuildError> {
        let offset = self.location.offset;
        match self.location.bytes {
            1 => fields.assume_bits(body, Location::<I8>::new(offset), self.mask, self.expected),
            2 => fields.assume_bits(body, Location::<I16>::new(offset), self.mask, self.expected),
            4 => fields.assume_bits(body, Location::<I32>::new(offset), self.mask, self.expected),
            8 => fields.assume_bits(body, Location::<I64>::new(offset), self.mask, self.expected),
            _ => unreachable!("state patterns have scalar storage widths"),
        }
    }
}
