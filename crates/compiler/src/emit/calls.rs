//! Invocation emission and ordered consumption of its result stack.
use wasm_encoder::{Encode, Instruction};

use super::{LocalOp, Scheduler};
use crate::control::Site;

impl Scheduler<'_> {
    pub(super) fn authored_call(&mut self, site: Site) {
        let (invocation, outputs) = self.body.call(site);
        if outputs.first().is_some_and(|&output| self.emitted[output]) {
            return;
        }
        for &argument in &invocation.arguments {
            self.value(argument);
        }
        self.finish_call(site, None);
    }

    pub(super) fn finish_call(&mut self, site: Site, requested: Option<(usize, bool)>) {
        let (invocation, outputs) = self.body.call(site);
        self.call(invocation.target);
        if let [output] = outputs {
            let capture = requested.is_none_or(|(_, capture)| capture);
            self.completed(*output, capture);
            if requested.is_none() && self.placement.slots[*output].is_none() {
                Instruction::Drop.encode(&mut self.bytes);
            }
            return;
        }
        // The final result is at the top of the Wasm stack. Every component
        // belongs to this invocation even when only one result is demanded.
        for &output in outputs.iter().rev() {
            if self.placement.slots[output].is_some() {
                self.completed(output, true);
            } else {
                Instruction::Drop.encode(&mut self.bytes);
                self.emitted[output] = true;
            }
        }
        if let Some((output, false)) = requested {
            let slot = self.placement.slots[output].expect("a demanded call component is saved");
            self.local(slot, LocalOp::Get);
        }
    }
}
