//! Code fetch and control transfers check CS before instruction effects.

mod far;

pub(crate) use far::CodeTarget;

use relocade_compiler::{BuildError, Func, Val, I1, I32};
use std::collections::BTreeMap;

use crate::{exception::Exception, memory::Intent, segment::Segment};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    /// Checks a snapshot block's complete encoding before any guest effects.
    /// The interpreter handles a denied span to preserve prefix execution and
    /// fault order, including faults earlier than the first invalid CS offset.
    pub(crate) fn guard_fetch_span(&mut self, bytes: usize) -> Result<(), BuildError> {
        let Ok(bytes) = u32::try_from(bytes) else {
            return self.guard(false);
        };
        let check = self.segments.check(
            &mut self.body,
            &Segment::Cs.into(),
            &self.eip,
            bytes,
            Intent::Fetch,
        )?;
        if let Some(denied) = check.denied {
            self.guard(denied.eq(false))?;
        }
        Ok(())
    }

    pub(crate) fn jump(&mut self, target: Val<I32>) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, None)?;
        Ok(target)
    }

    pub(crate) fn branch(
        &mut self,
        taken: Val<I1>,
        target: Val<I32>,
        fallthrough: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        self.check_code_target(&target, Some(&taken))?;
        for successor in [&target, &fallthrough] {
            if let Some(eip) = self.body.constant_bits(successor)? {
                self.successors.push(eip as u32);
            }
        }
        Ok(taken.select(target, fallthrough))
    }

    /// Uses direct tail calls for known successors in this immutable group.
    /// Every edge publishes state and checks the budget before entering its target.
    pub(crate) fn dispatch_linked(self, entries: &BTreeMap<u32, Func>) -> Result<(), BuildError> {
        let runtime = self.runtime;
        let mut successors: Vec<_> = self
            .successors
            .iter()
            .copied()
            .filter(|eip| entries.contains_key(eip))
            .collect();
        successors.sort_unstable();
        successors.dedup();
        self.finish(true, |mut body, eip| {
            if let Some(target) = body.constant_bits(eip)? {
                if successors.contains(&(target as u32)) {
                    return body.tail_call(entries[&(target as u32)], &[]);
                }
            }
            body.switch(eip, &successors, |arm, target| match target {
                Some(target) => arm.tail_call(entries[&target], &[]),
                None => runtime.dispatch(arm),
            })?;
            body.trap()
        })
    }

    fn check_code_target(
        &mut self,
        target: &Val<I32>,
        taken: Option<&Val<I1>>,
    ) -> Result<(), BuildError> {
        let check = self.segments.check(
            &mut self.body,
            &Segment::Cs.into(),
            target,
            1,
            Intent::Fetch,
        )?;
        if let Some(denied) = check.denied {
            let fault = match taken {
                Some(taken) => taken.and(denied),
                None => denied,
            };
            self.fault_if(
                fault,
                Exception::GeneralProtection {
                    error_code: 0.into(),
                },
            )?;
        }
        Ok(())
    }
}
