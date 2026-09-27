//! Schedule call arguments and consume all results of each invocation together.
use super::{Instruction, LocalOp, RequestedResult, Scheduler};
use crate::body::Site;

impl Scheduler<'_> {
    pub(in crate::schedule) fn call(&mut self, site: Site) {
        let (invocation, outputs) = self.blocks.call(site);
        if outputs
            .first()
            .is_some_and(|&output| self.available[output])
        {
            return;
        }
        self.values(invocation.arguments.iter().copied());
        self.finish_call(site, None);
    }

    pub(super) fn finish_call(&mut self, site: Site, requested: Option<RequestedResult>) {
        let (invocation, outputs) = self.blocks.call(site);
        self.instructions.push(Instruction::Call(invocation.target));
        if let [output] = outputs {
            let capture = requested.as_ref().is_none_or(|result| result.capture);
            self.completed(*output, capture);
            if requested.is_none() && self.slots[*output].is_none() {
                self.instructions.push(Instruction::Drop);
            }
            return;
        }
        // One demanded component still produces the entire invocation's tuple.
        self.save_results(outputs);
        if let Some(RequestedResult {
            value,
            capture: false,
        }) = requested
        {
            let slot = self.slots[value].expect("a demanded call component is saved");
            self.local(slot, LocalOp::Get);
        }
    }
}
