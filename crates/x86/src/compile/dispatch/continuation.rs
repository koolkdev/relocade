//! A suspended callable retains its return independently of address replacement.

use super::Dispatch;
use crate::{state::Cpu, Segment};
use wasm86_compiler::{BlockBuilder, BuildError, I32, I64};

const RESUME_SLOT: u32 = 0;
const EIP: u32 = 4;
const CS_BASE: u32 = 8;
const CS_SELECTOR: u32 = 12;
const RETIRED: u32 = 16;

impl Dispatch {
    /// Retain a callable until its instruction retires or the execution address
    /// changes. A fault/yield can then retry its continuation without replaying
    /// earlier effects. Normal instructions need no continuation bookkeeping.
    pub(in crate::compile) fn suspend(
        &self,
        body: &mut BlockBuilder<'_>,
        cpu: &Cpu,
        slot: u32,
    ) -> Result<(), BuildError> {
        let eip = cpu.read_eip(body)?;
        let cs = cpu.read_segment(body, &Segment::Cs.into())?;
        let retired = cpu.read_instruction_count(body)?;
        body.store_at::<I32>(self.memory, EIP, 0, eip)?;
        body.store_at::<I32>(self.memory, CS_BASE, 0, cs.base)?;
        body.store_at::<I32>(
            self.memory,
            CS_SELECTOR,
            0,
            cs.selector.unsigned().extend::<I32>(),
        )?;
        body.store_at::<I64>(self.memory, RETIRED, 0, retired)?;
        body.store_at::<I32>(self.memory, RESUME_SLOT, 0, slot)
    }

    pub(super) fn resume(&self, body: &mut BlockBuilder<'_>, cpu: &Cpu) -> Result<(), BuildError> {
        let resume = body.load_at::<I32>(self.memory, RESUME_SLOT, 0)?;
        body.if_(resume.ne(0), |mut suspended| {
            let eip = cpu.read_eip(&mut suspended)?;
            let cs = cpu.read_segment(&mut suspended, &Segment::Cs.into())?;
            let previous_eip = suspended.load_at::<I32>(self.memory, EIP, 0)?;
            let previous_base = suspended.load_at::<I32>(self.memory, CS_BASE, 0)?;
            let previous_selector = suspended.load_at::<I32>(self.memory, CS_SELECTOR, 0)?;
            let previous_retired = suspended.load_at::<I64>(self.memory, RETIRED, 0)?;
            let retired = cpu.read_instruction_count(&mut suspended)?;
            suspended.if_(
                previous_eip
                    .eq(&eip)
                    .and(previous_base.eq(&cs.base))
                    .and(previous_selector.eq(cs.selector.unsigned().extend::<I32>()))
                    .and(previous_retired.eq(retired)),
                |resuming| self.enter(resuming, &resume),
            )?;
            suspended.store_at::<I32>(self.memory, RESUME_SLOT, 0, 0u32)
        })
    }
}
