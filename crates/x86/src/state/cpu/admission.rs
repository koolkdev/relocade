//! Checks the fixed execution profile before any profile-specialized reads.

use super::Cpu;
use crate::state::exit;
use crate::{ExecutionContext, ExecutionProfile, Segment, StoredSegment};
use relocade_compiler::{BlockBuilder, BuildError, Val, I64};
use std::mem::{offset_of, size_of};

impl Cpu {
    pub(crate) fn admit_profile(
        &self,
        body: &mut BlockBuilder<'_>,
        profile: ExecutionProfile,
    ) -> Result<(), BuildError> {
        // Real16 loads construct canonical caches and the machine rejects host
        // edits that violate them. Protected loads can invalidate a fixed profile.
        let ExecutionProfile::Protected(profile) = profile else {
            return Ok(());
        };
        let mut difference: Val<I64> = 0u64.into();
        for segment in Segment::ALL {
            let facts = profile.assumptions(segment);
            let mut mask = [0u8; size_of::<StoredSegment>()];
            let mut expected = [0u8; size_of::<StoredSegment>()];
            macro_rules! field {
                ($field:ident) => {
                    if let Some(value) = facts.$field {
                        let bytes = value.to_le_bytes();
                        let start = offset_of!(StoredSegment, $field);
                        let end = start + bytes.len();
                        mask[start..end].fill(0xff);
                        expected[start..end].copy_from_slice(&bytes);
                    }
                };
            }
            field!(base);
            field!(limit);
            field!(usable);
            field!(readable);
            field!(writable);
            field!(executable);
            field!(expand_down);
            field!(big);
            // Check the serialized image in words instead of loading each byte
            // separately on every dispatch. Unconstrained fields have zero masks.
            for (word, (&mask, &expected)) in mask
                .as_chunks::<8>()
                .0
                .iter()
                .zip(expected.as_chunks::<8>().0)
                .enumerate()
            {
                let mask = u64::from_le_bytes(mask);
                if mask == 0 {
                    continue;
                }
                let expected = u64::from_le_bytes(expected);
                let address = offset_of!(ExecutionContext, cpu.segments)
                    + segment as usize * size_of::<StoredSegment>()
                    + word * 8;
                let actual = body.load::<I64>(self.memory(), address as u32)?;
                difference = difference.or(actual.xor(expected).and(mask));
            }
        }
        body.if_(difference.ne(0u64), exit::profile_changed)
    }
}
