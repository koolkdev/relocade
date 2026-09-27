//! Conditional exits, live edge arguments and lexical branch depths.
use wasm_encoder::{BlockType, ValType};

use super::{Instruction, LocalOp, Scheduler};
use crate::{
    body::{Block, Site, Target, Terminal, ValueDefinition},
    place,
};

impl Scheduler<'_> {
    // Returns true only when the immediate terminal continuation was scheduled too.
    pub(super) fn conditional_branch(
        &mut self,
        condition: usize,
        taken: &Block,
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
                self.condition(condition, inverted);
                if live.is_empty() {
                    self.captures(site);
                } else {
                    // The condition precedes captures and tuple evaluation. The
                    // ordinary local allocator owns this saved predicate.
                    let slot = self.temporary(ValType::I32);
                    self.local(slot, LocalOp::Set);
                    self.captures(site);
                    self.values(live);
                    self.local(slot, LocalOp::Get);
                }
                self.instructions
                    .push(Instruction::BranchIf(self.branch_depth(branch)));
                // A false br_if retains the tuple for the other edge.
                if Some(successor) != fallthrough {
                    self.branch_to(successor);
                }
                return true;
            }
        }
        self.condition(condition, false);
        self.captures(site);
        if live.is_empty() {
            self.instructions
                .push(Instruction::BranchIf(self.branch_depth(*target)));
        } else {
            // A lone edge's arguments may trap or require snapshots. Demand them
            // only on its taken path, rather than preparing a speculative tuple.
            self.begin_control(Instruction::If(BlockType::Empty), None, &[]);
            let before = self.available.clone();
            self.block(taken, None);
            self.end_control();
            self.available = before;
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
                let component = match self.body.values[output].definition {
                    ValueDefinition::JoinResult { component, .. }
                    | ValueDefinition::LoopInput { component, .. } => component,
                    _ => unreachable!("control edges name joined values"),
                };
                arguments[component]
            })
            .collect()
    }

    pub(super) fn branch_to(&mut self, target: Target) {
        self.instructions
            .push(Instruction::Branch(self.branch_depth(target)));
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
