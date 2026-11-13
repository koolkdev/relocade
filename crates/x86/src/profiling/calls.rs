//! Bounded call observations shared by generated recording and host draining.

use relocade_compiler::{BlockBuilder, BuildError, Mem, Program, TypedFunc, Val, I32};

const SLOTS: u32 = 1024;
const CAPACITY: u32 = SLOTS / 2;
const PROBES: u32 = 8;
const USED: u32 = 0;
const KEYS: u32 = 4;
const OVERFLOW_SLOT: u32 = SLOTS;
const COUNTS: u32 = KEYS + (SLOTS + 1) * 4;
const OCCUPIED: u32 = COUNTS + (SLOTS + 1) * 4;

/// Calls to one destination within a batch. The execution owner defines the key;
/// it must distinguish all destinations whose counts cannot be combined.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallCount {
    pub target: u32,
    pub count: u32,
}

/// A stopped CPU's call profile, retaining the slots needed to clear its batch.
/// Recording uses 1,024 slots, admits at most 512 distinct targets and probes at
/// most eight slots. Repeated targets increment their count without consuming a
/// slot. Capacity, probe or count exhaustion retains the triggering call in one
/// overflow slot and requests a flush. Do not record again before clearing that
/// batch. Its final entry can repeat an earlier target.
///
/// Initialize `relocade.callProfile` to zero before recording. Read, merge and clear
/// only while that CPU is stopped; no atomics or cross-CPU synchronization is used.
pub struct CallProfile {
    entries: Vec<(u32, CallCount)>,
}

impl CallProfile {
    /// Required initialized bytes in [`crate::abi::CALL_PROFILE`].
    pub const BYTE_LEN: usize = (OCCUPIED + (CAPACITY + 1) * 4) as usize;

    /// Reports overflow with one load, even when another execution exit took
    /// precedence over the recorder's request. Drain before recording again.
    pub fn requires_drain(mut load: impl FnMut(u32) -> u32) -> bool {
        load(COUNTS + OVERFLOW_SLOT * 4) != 0
    }

    /// Copies the occupied entries through little-endian 32-bit loads. Offsets
    /// are relative to the call-count memory, whose representation stays here.
    /// Clear this batch before resuming execution, including when it is empty.
    pub fn read(mut load: impl FnMut(u32) -> u32) -> Self {
        let used = load(USED);
        let entries = (0..used)
            .map(|index| {
                let slot = load(OCCUPIED + index * 4);
                (
                    slot,
                    CallCount {
                        target: load(KEYS + slot * 4),
                        count: load(COUNTS + slot * 4),
                    },
                )
            })
            .collect();
        Self { entries }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = CallCount> + '_ {
        self.entries.iter().map(|(_, count)| *count)
    }

    /// Clears occupied counts and resets the batch through little-endian stores.
    /// Keys need no clearing: a zero count marks an empty slot, including key zero.
    /// Do not resume generated code between reading this batch and clearing it.
    pub fn clear(self, mut store: impl FnMut(u32, u32)) {
        for (slot, _) in self.entries {
            store(COUNTS + slot * 4, 0);
        }
        store(USED, 0);
    }

    /// Declares a reusable generated recorder over [`crate::abi::CALL_PROFILE`].
    /// Every call is recorded. False requires draining before the next call; the
    /// triggering call occupies the overflow slot. Its body is built when reachable.
    pub fn declare_recorder(program: &mut Program) -> TypedFunc<fn(u32) -> bool> {
        let memory = program.import_memory(crate::abi::CALL_PROFILE);
        program.function::<fn(u32) -> bool>(move |mut body, (target,)| {
            let product = target.mul(0x9e37_79b1u32);
            let bucket = product.xor(product.unsigned().shr(16)).and(SLOTS - 1);
            body.loop_::<(I32, I32), ()>((bucket, 0), |mut body, labels, (slot, probes)| {
                let count = body.load_at::<I32>(memory, slot.mul(4), COUNTS)?;
                body.if_(count.eq(0), |mut body| {
                    let used = body.load::<I32>(memory, USED)?;
                    body.if_(used.unsigned().ge(CAPACITY), |body| {
                        record_overflow(body, memory, &target)
                    })?;
                    body.store_at::<I32>(memory, slot.mul(4), KEYS, &target)?;
                    body.store_at::<I32>(memory, slot.mul(4), COUNTS, 1)?;
                    body.store_at::<I32>(memory, used.mul(4), OCCUPIED, &slot)?;
                    body.store::<I32>(memory, USED, used.add(1))?;
                    body.return_(true)
                })?;
                let key = body.load_at::<I32>(memory, slot.mul(4), KEYS)?;
                body.if_(key.eq(&target), |mut body| {
                    body.if_(count.eq(u32::MAX), |body| {
                        record_overflow(body, memory, &target)
                    })?;
                    body.store_at::<I32>(memory, slot.mul(4), COUNTS, count.add(1))?;
                    body.return_(true)
                })?;
                body.if_(probes.eq(PROBES - 1), |body| {
                    record_overflow(body, memory, &target)
                })?;
                body.branch(&labels.again, (slot.add(1).and(SLOTS - 1), probes.add(1)))
            })?;
            body.trap()
        })
    }
}

// Retain the triggering call even if an interrupt or single-step check returns
// before the caller reports that the collector needs draining.
fn record_overflow(
    mut body: BlockBuilder<'_>,
    memory: Mem,
    target: &Val<I32>,
) -> Result<(), BuildError> {
    let used = body.load::<I32>(memory, USED)?;
    body.store::<I32>(memory, KEYS + OVERFLOW_SLOT * 4, target)?;
    body.store::<I32>(memory, COUNTS + OVERFLOW_SLOT * 4, 1)?;
    body.store_at::<I32>(memory, used.mul(4), OCCUPIED, OVERFLOW_SLOT)?;
    body.store::<I32>(memory, USED, used.add(1))?;
    body.return_(false)
}

#[cfg(test)]
mod tests;
