//! Masked field values shared by admission, guards and retained state.

use super::{Address, Location, LocationKey, SsaType, StateFields};
use relocade_compiler::{BlockBuilder, BuildError, IntType, Val, I1, I16, I32, I64, I8};

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

    /// Compares the constrained bits covered by this full or partial CPU record.
    pub(crate) fn matches_record(self, bytes: &[u8], offset: usize) -> bool {
        let field = self.location.offset as usize;
        let start = offset.max(field);
        let end = (offset + bytes.len()).min(field + self.location.bytes as usize);
        (start..end).all(|at| {
            let mask = self.mask.to_le_bytes()[at - field];
            bytes[at - offset] & mask == self.expected.to_le_bytes()[at - field]
        })
    }

    pub(crate) fn within_record(self, offset: usize, bytes: usize) -> bool {
        let start = self.location.offset as usize;
        start >= offset && start + self.location.bytes as usize <= offset + bytes
    }

    /// Reads the current retained value, including unpublished guest writes.
    pub(crate) fn matches(
        self,
        body: &mut BlockBuilder<'_>,
        fields: &mut StateFields,
    ) -> Result<Val<I1>, BuildError> {
        match self.location.bytes {
            1 => self.matches_field::<I8>(body, fields),
            2 => self.matches_field::<I16>(body, fields),
            4 => self.matches_field::<I32>(body, fields),
            8 => self.matches_field::<I64>(body, fields),
            _ => unreachable!("state patterns have scalar storage widths"),
        }
    }

    fn matches_field<T: SsaType + IntType>(
        self,
        body: &mut BlockBuilder<'_>,
        fields: &mut StateFields,
    ) -> Result<Val<I1>, BuildError> {
        let value = fields.read(body, Location::<T>::new(self.location.offset))?;
        let mask = Val::<I64>::from(self.mask).truncate::<T>();
        let masked = value.and(mask);
        let expected = Val::<I64>::from(self.expected).truncate::<T>();
        Ok(masked.eq(expected))
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

impl<T: SsaType + IntType> Location<T> {
    /// Read the requested bits only when the supplied patterns establish the full mask.
    pub(crate) fn known_bits(
        self,
        patterns: impl Iterator<Item = StatePattern>,
        mask: impl Into<u32>,
    ) -> Option<u32> {
        let field = StatePattern::bits(self, u64::from(mask.into()), 0);
        let (mut known, mut value) = (0, 0);
        for pattern in patterns.filter(|pattern| pattern.location == field.location) {
            known |= pattern.mask;
            value |= pattern.expected;
        }
        (known & field.mask == field.mask).then_some((value & field.mask) as u32)
    }
}
