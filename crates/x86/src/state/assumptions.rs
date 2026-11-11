//! Scalar entry conditions shared by generated predicates and host admission.

use std::mem::{offset_of, size_of};

use relocade_compiler::{AtLeast, BlockBuilder, BuildError, MemoryInt, Val, I1, I16, I32, I64, I8};

use crate::{
    segment::SegmentAssumptions,
    ssa::{Location, StateFields},
    ExecutionProfile, Segment,
};

use super::{access::CpuField, CpuState, ExecutionContext, SegmentRegister, State};

/// A masked equality over a scalar in the little-endian CPU backing image.
/// An admitted entry requires `(field & mask) == expected` throughout its use.
/// Guarded observations are checked by generated code and are not entry assumptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateAssumption {
    /// Byte offset within [`CpuState::to_bytes`], independent of execution controls.
    pub offset: u32,
    /// Width of the field in bytes: 1, 2, 4 or 8.
    pub bytes: u8,
    pub mask: u64,
    pub expected: u64,
}

impl CpuState {
    /// Checks supplied entry assumptions against this snapshot. This does not
    /// establish instruction support, code validity or later invalidation.
    pub fn satisfies(&self, assumptions: &[StateAssumption]) -> bool {
        let bytes = self.to_bytes();
        assumptions
            .iter()
            .all(|assumption| assumption.matches(&bytes))
    }
}

impl StateAssumption {
    fn matches(&self, image: &[u8; CpuState::BYTE_LEN]) -> bool {
        if !matches!(self.bytes, 1 | 2 | 4 | 8) {
            return false;
        }
        let start = self.offset as usize;
        let Some(field) = image.get(start..start.saturating_add(usize::from(self.bytes))) else {
            return false;
        };
        let mut bytes = [0; 8];
        bytes[..field.len()].copy_from_slice(field);
        u64::from_le_bytes(bytes) & self.mask == self.expected
    }

    fn field<T: CpuField>(
        offset: u32,
        _field: fn(&CpuState) -> &T,
        mask: u64,
        expected: u64,
    ) -> Self {
        Self {
            offset,
            bytes: size_of::<T>() as u8,
            mask,
            expected,
        }
    }

    pub(super) fn predicate<T: MemoryInt>(&self, value: &Val<T>) -> Val<I1>
    where
        I64: AtLeast<T>,
    {
        debug_assert_eq!(u32::from(self.bytes), T::BYTES);
        value
            .and(Val::<I64>::from(self.mask).truncate::<T>())
            .eq(Val::<I64>::from(self.expected).truncate::<T>())
    }

    fn bind(
        &self,
        body: &mut BlockBuilder<'_>,
        fields: &mut StateFields,
    ) -> Result<Val<I1>, BuildError> {
        let offset = offset_of!(ExecutionContext, cpu) as u32 + self.offset;
        macro_rules! bind {
            ($ty:ty) => {
                self.predicate(&fields.read(body, Location::<$ty>::new(offset))?)
            };
        }
        Ok(match self.bytes {
            1 => bind!(I8),
            2 => bind!(I16),
            4 => bind!(I32),
            8 => bind!(I64),
            _ => unreachable!("entry conditions name scalar CPU fields"),
        })
    }

    fn controls(profile: ExecutionProfile) -> Vec<Self> {
        macro_rules! condition {
            ($($field:ident).+, $mask:expr, $expected:expr) => {
                Self::field(
                    offset_of!(CpuState, $($field).+) as u32,
                    |cpu| &cpu.$($field).+,
                    $mask,
                    $expected,
                )
            };
        }
        let protected = matches!(profile, ExecutionProfile::Protected(_));
        let mut conditions = vec![
            condition!(cr0, u32::MAX.into(), 0x30 | u64::from(protected)),
            condition!(cr4, u32::MAX.into(), 0x600),
            condition!(flags.bytes.vm, 1, 0),
        ];
        if protected {
            conditions.push(condition!(segments.cs.selector, 3, 3));
        }
        conditions
    }

    pub(super) fn segment(profile: ExecutionProfile, segment: Segment) -> Vec<Self> {
        let assumptions = match profile {
            ExecutionProfile::Real16 => SegmentAssumptions {
                limit: Some(0xffff),
                big: Some(0),
                ..SegmentAssumptions::default()
            },
            ExecutionProfile::Protected(profile) => {
                SegmentAssumptions::for_profile(profile, segment)
            }
        };
        let mut conditions = Vec::new();
        macro_rules! field {
            ($field:ident, $mask:expr) => {
                if let Some(expected) = assumptions.$field {
                    conditions.push(Self::field(
                        offset_of!(CpuState, segments.es.$field) as u32
                            + segment as u32 * size_of::<SegmentRegister>() as u32,
                        |cpu| &cpu.segments.es.$field,
                        $mask,
                        expected.into(),
                    ));
                }
            };
        }
        field!(base, u32::MAX.into());
        field!(limit, u32::MAX.into());
        field!(usable, u8::MAX.into());
        field!(readable, u8::MAX.into());
        field!(writable, u8::MAX.into());
        field!(executable, u8::MAX.into());
        field!(expand_down, u8::MAX.into());
        field!(big, u8::MAX.into());
        conditions
    }

    /// Retains all supplied conditions, including fields unused by an entry.
    pub(crate) fn for_profile(profile: ExecutionProfile) -> Vec<Self> {
        let mut conditions = Self::controls(profile);
        conditions.extend(
            Segment::ALL
                .into_iter()
                .flat_map(|segment| Self::segment(profile, segment)),
        );
        conditions
    }
}

impl State<'_> {
    pub(crate) fn assume_entry_state(
        &mut self,
        body: &mut BlockBuilder<'_>,
        profile: ExecutionProfile,
    ) -> Result<(), BuildError> {
        let mut predicate = Val::<I1>::from(true);
        for assumption in StateAssumption::controls(profile) {
            predicate = predicate.and(assumption.bind(body, &mut self.fields)?);
        }
        body.assume(predicate)
    }
}
