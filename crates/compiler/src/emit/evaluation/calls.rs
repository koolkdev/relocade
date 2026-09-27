//! Schedule call arguments and consume all results of each invocation together.
use super::{Evaluation, LocalOp, RequestedResult, ValuePlanner};
use crate::body::Site;

impl ValuePlanner<'_> {
    pub(in crate::emit) fn call(&mut self, site: Site) -> Vec<Evaluation> {
        let (invocation, outputs) = self.blocks.call(site);
        if outputs
            .first()
            .is_some_and(|&output| self.available[output])
        {
            return Vec::new();
        }
        let mut plan = self.values(invocation.arguments.iter().copied());
        self.finish_call(site, None, &mut plan);
        plan
    }

    pub(super) fn finish_call(
        &mut self,
        site: Site,
        requested: Option<RequestedResult>,
        plan: &mut Vec<Evaluation>,
    ) {
        let (invocation, outputs) = self.blocks.call(site);
        plan.push(Evaluation::Call(invocation.target));
        if let [output] = outputs {
            let capture = requested.as_ref().is_none_or(|result| result.capture);
            self.completed(*output, capture, plan);
            if requested.is_none() && !self.has_local(*output) {
                plan.push(Evaluation::Drop);
            }
            return;
        }
        // One demanded component still produces the entire invocation's tuple.
        self.consume_results(outputs, plan);
        if let Some(RequestedResult {
            value,
            capture: false,
        }) = requested
        {
            let slot = self.slots[value].expect("a demanded call component is saved");
            plan.push(Evaluation::Local {
                slot,
                operation: LocalOp::Get,
            });
        }
    }
}
