//! Schedule authored effects and control with live result stacks and branch depths.

use super::{wasm_type, ControlLabel, Instruction, Scheduler};
use crate::{
    body::{Block, Operation, Site, Target, Terminal},
    place,
};

impl Scheduler<'_> {
    pub(super) fn block(&mut self, block: &Block, fallthrough: Option<Target>) {
        let forwarding = match (&block.terminal, block.operations.last()) {
            (Some(Terminal::Branch { target, arguments }), Some(operation)) => {
                let arguments = self.branch_arguments(*target, arguments);
                let outputs = self.live_outputs(operation);
                arguments.len() == outputs.len()
                    && arguments.iter().zip(outputs).all(|(&argument, output)| {
                        place::representation(self.body, argument) == output
                    })
            }
            _ => false,
        };
        for (index, operation) in block.operations.iter().enumerate() {
            let site = Site {
                block: block.id,
                index,
            };
            if let Operation::BranchIf { condition, taken } = operation {
                let continuation = block
                    .terminal
                    .as_ref()
                    .filter(|_| index + 1 == block.operations.len());
                if self.conditional_branch(*condition, taken, site, continuation, fallthrough) {
                    return;
                }
                continue;
            }
            let outputs = self.live_outputs(operation);
            let result_types: Vec<_> = outputs
                .iter()
                .map(|&id| wasm_type(self.body.values[id].ty))
                .collect();
            let block_type = self.types.block(&result_types);
            if let Operation::Switch { cases, .. } = operation {
                self.open_switch(cases.len(), block_type, site, &outputs);
            }
            // Keep the selector on the stack while common values are captured.
            match operation {
                Operation::If { condition, .. } => self.condition(*condition, false),
                Operation::Switch { selector, .. } => self.values([*selector]),
                _ => {}
            }
            self.captures(site);
            match operation {
                Operation::BranchIf { .. } => unreachable!("conditional exits are scheduled above"),
                Operation::Nop | Operation::Load { .. } => {}
                Operation::Fence => self.instructions.push(Instruction::Fence),
                Operation::Atomic { access, output } => {
                    self.values(access.inputs());
                    self.instructions
                        .push(Instruction::Atomic(access.map(|_| ())));
                    if let Some(output) = *output {
                        self.save_results(&[output]);
                    }
                }
                Operation::Call { invocation, .. } => {
                    if self.effects[invocation.target.0].must_execute() {
                        self.call(site);
                    }
                }
                Operation::Store { location, value } => {
                    self.values([location.base, *value]);
                    self.instructions
                        .push(Instruction::Store(location.map(|_| ())));
                }
                Operation::Block { block, .. } => {
                    let before = self.available.clone();
                    let target = Target::exit(site);
                    if outputs.is_empty() && block.exits_to(target).next().is_none() {
                        // An unreferenced unit block needs no Wasm label. Outward
                        // exits must still skip the enclosing body's continuation.
                        self.block(block, None);
                    } else {
                        self.begin_control(Instruction::Block(block_type), Some(target), &outputs);
                        self.block(block, Some(target));
                        self.end_control();
                    }
                    // An outward exit can skip any capture in this child. Only
                    // the joined outputs are available after the block.
                    self.available = before;
                }
                Operation::Loop {
                    initial,
                    inputs,
                    block,
                    ..
                } => {
                    self.loop_block(initial, inputs, block, site, &outputs);
                }
                Operation::If {
                    branch,
                    else_branch,
                    ..
                } => {
                    self.begin_control(
                        Instruction::If(block_type),
                        Some(Target::exit(site)),
                        &outputs,
                    );
                    let before_arm = self.available.clone();
                    self.block(branch, Some(Target::exit(site)));
                    self.available.clone_from(&before_arm);
                    if let Some(other) = else_branch {
                        self.instructions.push(Instruction::Else);
                        self.block(other, Some(Target::exit(site)));
                    }
                    self.end_control();
                    self.available = before_arm;
                }
                Operation::Switch { cases, default, .. } => {
                    self.switch(cases, default, site);
                }
            }
            if !(forwarding && index + 1 == block.operations.len()) {
                self.save_results(&outputs);
            }
        }
        if let Some(terminal) = &block.terminal {
            if let Terminal::Branch { target, arguments } = terminal {
                if !forwarding {
                    self.values(self.branch_arguments(*target, arguments));
                }
                if Some(*target) != fallthrough {
                    self.branch_to(*target);
                }
                return;
            }
            self.values(terminal.inputs().iter().copied());
            self.instructions.push(match terminal {
                Terminal::Branch { .. } => {
                    unreachable!("control transfers follow their own path and result shape")
                }
                Terminal::Return(_) => Instruction::Return,
                Terminal::Trap => Instruction::Trap,
                Terminal::TailCall(invocation) => Instruction::TailCall(invocation.target),
            });
        }
    }

    fn live_outputs(&self, operation: &Operation) -> Vec<usize> {
        operation
            .branch_outputs()
            .iter()
            .copied()
            .filter(|&id| self.slots[id].is_some())
            .collect()
    }

    pub(super) fn begin_control(
        &mut self,
        instruction: Instruction,
        target: Option<Target>,
        outputs: &[usize],
    ) {
        self.instructions.push(instruction);
        self.labels.push(ControlLabel {
            target,
            outputs: outputs.to_vec(),
        });
    }

    pub(super) fn end_control(&mut self) {
        self.labels
            .pop()
            .expect("an ending control has an open label");
        self.instructions.push(Instruction::End);
    }
}
