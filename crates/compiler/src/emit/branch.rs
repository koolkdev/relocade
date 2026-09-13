//! Conditional exits, live edge arguments and lexical branch depths.
use wasm_encoder::{BlockType, Encode, Instruction, ValType};

use super::{LocalOp, Scheduler};
use crate::{
    control::{Region, Site, Target},
    place, Terminal, ValueKind,
};

impl Scheduler<'_> {
    // Returns true only when the immediate terminal continuation was emitted too.
    pub(super) fn conditional_branch(
        &mut self,
        condition: usize,
        taken: &Region,
        site: Site,
        continuation: Option<&Terminal>,
        fallthrough: Option<Target>,
    ) -> bool {
        let Some(Terminal::Branch { target, arguments }) = &taken.terminal else {
            unreachable!("a conditional exit owns one branch edge");
        };
        debug_assert!(taken.operations.is_empty());
        let live = self.branch_arguments(*target, arguments);
        if let Some(Terminal::Branch {
            target: otherwise,
            arguments: continued,
        }) = continuation
        {
            let continued = self.branch_arguments(*otherwise, continued);
            if live.len() == continued.len()
                && live.iter().zip(&continued).all(|(&a, &b)| {
                    place::representation(self.body, a) == place::representation(self.body, b)
                })
            {
                // Prefer the actual physical successor, independent of how the
                // caller spelled the condition. Invert its truth, not its reads.
                let inverted = Some(*target) == fallthrough && target != otherwise;
                let (branch, successor) = if inverted {
                    (*otherwise, *target)
                } else {
                    (*target, *otherwise)
                };
                self.emit_condition(condition, inverted);
                if live.is_empty() {
                    self.emit_captures(site);
                } else {
                    // The condition precedes captures and tuple evaluation. The
                    // ordinary local allocator owns this saved predicate.
                    let slot = self.placement.slot_types.len();
                    self.placement.slot_types.push(ValType::I32);
                    self.local(slot, LocalOp::Set);
                    self.emit_captures(site);
                    for argument in live {
                        self.value(argument);
                    }
                    self.local(slot, LocalOp::Get);
                }
                Instruction::BrIf(self.branch_depth(branch)).encode(&mut self.bytes);
                // A false br_if retains the tuple for the other edge.
                if Some(successor) != fallthrough {
                    self.branch_to(successor);
                }
                return true;
            }
        }
        self.emit_condition(condition, false);
        self.emit_captures(site);
        if live.is_empty() {
            Instruction::BrIf(self.branch_depth(*target)).encode(&mut self.bytes);
        } else {
            // A lone edge's arguments may trap or require snapshots. Demand them
            // only on its taken path, rather than preparing a speculative tuple.
            self.begin_control(Instruction::If(BlockType::Empty), None, &[]);
            let before = self.emitted.clone();
            self.region(taken, None);
            self.end_control();
            self.emitted = before;
        }
        false
    }

    pub(super) fn branch_arguments(&self, target: Target, arguments: &[usize]) -> Vec<usize> {
        let label = self
            .labels
            .iter()
            .rev()
            .find(|label| label.target == Some(target))
            .expect("a branch target is an enclosing control label");
        label
            .outputs
            .iter()
            .map(|&output| {
                let component = match self.body.values[output].kind {
                    ValueKind::JoinResult { component, .. }
                    | ValueKind::LoopInput { component, .. } => component,
                    _ => unreachable!("control edges name joined values"),
                };
                arguments[component]
            })
            .collect()
    }

    pub(super) fn branch_to(&mut self, target: Target) {
        Instruction::Br(self.branch_depth(target)).encode(&mut self.bytes);
    }

    fn branch_depth(&self, target: Target) -> u32 {
        let depth = self
            .labels
            .iter()
            .rev()
            .position(|label| label.target == Some(target))
            .expect("a branch target is an enclosing control label");
        u32::try_from(depth).expect("branch depth fits the Wasm index space")
    }
}
