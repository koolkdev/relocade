//! Bounded interpreter-entry counters and their shared memory representation.

use crate::{
    abi,
    state::{Cpu, SegmentReads},
    ExecutionExit, ExecutionProfile, Segment,
};
use relocade_compiler::{BuildError, Func, Program, I32};
use std::num::NonZeroU16;

const HEADER: u32 = 16;
const ROW_BYTES: u32 = 16;
const HASH: u32 = 0x9e37_79b1;

/// Execution identity of a hot interpreter entry, before any instruction effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HotEntry {
    pub code_base: u32,
    pub start_eip: u32,
}

/// Image of `relocade.hotness`. Counters are direct-mapped; collisions replace the
/// old identity rather than combining unrelated heat. The single pending request
/// is replaced if execution continues to another request before the host takes it.
/// This table is a scheduling hint; code publication must still validate its bytes.
pub struct HotnessTable([u8; Self::BYTE_LEN]);

impl HotnessTable {
    pub const CAPACITY: usize = 256;
    pub const BYTE_LEN: usize = HEADER as usize + Self::CAPACITY * ROW_BYTES as usize;
    /// Visits suppressed after a request, allowing declined/unsupported code to
    /// make progress and eventually be reconsidered without immediate retries.
    pub const RETRY_VISITS: u32 = 1024;

    pub fn new(threshold: Option<NonZeroU16>) -> Self {
        let mut image = Self([0; Self::BYTE_LEN]);
        image.0[..4]
            .copy_from_slice(&u32::from(threshold.map_or(0, NonZeroU16::get)).to_le_bytes());
        image
    }

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self(bytes)
    }
    pub fn as_bytes(&self) -> &[u8; Self::BYTE_LEN] {
        &self.0
    }

    pub fn threshold(&self) -> Option<NonZeroU16> {
        NonZeroU16::new(self.word(0) as u16)
    }

    /// Takes the published request without changing counters or cooldowns.
    pub fn take_request(&mut self) -> Option<HotEntry> {
        if self.word(4) == 0 {
            return None;
        }
        self.0[4..8].fill(0);
        Some(HotEntry {
            code_base: self.word(8),
            start_eip: self.word(12),
        })
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.0[offset..offset + 4].try_into().unwrap())
    }
}

pub(super) fn profile_entry(
    program: &mut Program,
    cpu: &Cpu,
    profile: ExecutionProfile,
    interpret: Func,
) -> Result<Func, BuildError> {
    let memory = program.import_memory(abi::HOTNESS);
    program.function(abi::PROFILE.signature(), |mut body| {
        let threshold = body.load_at::<I32>(memory, 0, 0)?;
        body.if_(threshold.eq(0), |disabled| {
            disabled.tail_call(interpret, &[])
        })?;
        let eip = cpu.read_eip(&mut body)?;
        let mut segments = SegmentReads::new(cpu, profile);
        let base = segments.base(&mut body, &Segment::Cs.into())?;
        let product = base.add(&eip).mul(HASH);
        let bucket = product
            .xor(product.unsigned().shr(16))
            .and(HotnessTable::CAPACITY as u32 - 1);
        let row = bucket.mul(ROW_BYTES).add(HEADER);
        let previous_base = body.load_at::<I32>(memory, &row, 0)?;
        let previous_eip = body.load_at::<I32>(memory, &row, 4)?;
        body.if_(
            previous_base.ne(&base).or(previous_eip.ne(&eip)),
            |mut replace| {
                replace.store_at::<I32>(memory, &row, 0, &base)?;
                replace.store_at::<I32>(memory, &row, 4, &eip)?;
                replace.store_at::<I32>(memory, &row, 8, 0)?;
                replace.store_at::<I32>(memory, &row, 12, 0)
            },
        )?;
        let cooldown = body.load_at::<I32>(memory, &row, 12)?;
        body.if_(cooldown.ne(0), |mut cooling| {
            cooling.store_at::<I32>(memory, &row, 12, cooldown.sub(1))?;
            cooling.tail_call(interpret, &[])
        })?;
        let count = body.load_at::<I32>(memory, &row, 8)?.add(1);
        body.store_at::<I32>(memory, &row, 8, &count)?;
        body.if_(count.unsigned().ge(threshold), |mut hot| {
            hot.store_at::<I32>(memory, &row, 8, 0)?;
            hot.store_at::<I32>(memory, &row, 12, HotnessTable::RETRY_VISITS)?;
            hot.store_at::<I32>(memory, 0, 8, &base)?;
            hot.store_at::<I32>(memory, 0, 12, &eip)?;
            hot.store_at::<I32>(memory, 0, 4, 1)?;
            hot.return_(ExecutionExit::CompileRequest.to_bits())
        })?;
        body.tail_call(interpret, &[])
    })
}
