//! Structured control emission, live result stacks and lexical branch depths.
use wasm_encoder::{Encode, Instruction};

use super::{wasm_type, ControlLabel, Scheduler};
use crate::{
    control::{Region, Site, Target},
    place, Operation, Terminal,
};

impl Scheduler<'_> {
    pub(super) fn region(&mut self, region: &Region, fallthrough: Option<Target>) {
        let forwarding = match (&region.terminal, region.operations.last()) {
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
        for (index, operation) in region.operations.iter().enumerate() {
            let site = Site {
                region: region.id,
                index,
            };
            if let Operation::BranchIf { condition, taken } = operation {
                let continuation = region
                    .terminal
                    .as_ref()
                    .filter(|_| index + 1 == region.operations.len());
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
                Operation::If { condition, .. } => self.emit_condition(*condition, false),
                Operation::Switch { selector, .. } => self.value(*selector),
                _ => {}
            }
            self.emit_captures(site);
            match operation {
                Operation::BranchIf { .. } => unreachable!("conditional exits emit above"),
                Operation::Nop | Operation::Load(_) => {}
                Operation::Call { invocation, .. } => {
                    if self.effects[invocation.target.0].must_execute() {
                        self.authored_call(site);
                    }
                }
                Operation::Store { location, value } => {
                    self.value(location.base);
                    self.value(*value);
                    let argument = self.memory_argument(*location);
                    match location.bytes {
                        1 => Instruction::I32Store8(argument),
                        2 => Instruction::I32Store16(argument),
                        4 => Instruction::I32Store(argument),
                        8 => Instruction::I64Store(argument),
                        _ => unreachable!("memory locations have a supported byte size"),
                    }
                    .encode(&mut self.bytes);
                }
                Operation::Block { region, .. } => {
                    let before = self.emitted.clone();
                    let target = Target::exit(site);
                    if outputs.is_empty() && region.exits_to(target).next().is_none() {
                        // An unreferenced unit block needs no Wasm label. Outward
                        // exits must still skip the enclosing body's continuation.
                        self.region(region, None);
                    } else {
                        self.begin_control(Instruction::Block(block_type), Some(target), &outputs);
                        self.region(region, Some(target));
                        self.end_control();
                    }
                    // An outward exit can skip any capture in this child. Only
                    // the joined outputs are available after the block.
                    self.emitted = before;
                }
                Operation::Loop {
                    initial,
                    inputs,
                    region,
                    ..
                } => {
                    self.loop_region(initial, inputs, region, site, &outputs);
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
                    let before_arm = self.emitted.clone();
                    self.region(branch, Some(Target::exit(site)));
                    self.emitted.clone_from(&before_arm);
                    if let Some(other) = else_branch {
                        Instruction::Else.encode(&mut self.bytes);
                        self.region(other, Some(Target::exit(site)));
                    }
                    self.end_control();
                    self.emitted = before_arm;
                }
                Operation::Switch { cases, default, .. } => {
                    self.switch(cases, default, site);
                }
            }
            if !(forwarding && index + 1 == region.operations.len()) {
                // The last result is at the top of the Wasm operand stack.
                for &output in outputs.iter().rev() {
                    self.completed(output, true);
                }
            }
        }
        if let Some(terminal) = &region.terminal {
            if let Terminal::Branch { target, arguments } = terminal {
                if !forwarding {
                    for argument in self.branch_arguments(*target, arguments) {
                        self.value(argument);
                    }
                }
                if Some(*target) != fallthrough {
                    self.branch_to(*target);
                }
                return;
            }
            for &value in terminal.inputs() {
                self.value(value);
            }
            match terminal {
                Terminal::Branch { .. } => {
                    unreachable!("control transfers follow their own path and result shape")
                }
                Terminal::Return(_) => Instruction::Return,
                Terminal::Trap => Instruction::Unreachable,
                Terminal::TailCall(invocation) => Instruction::ReturnCall(
                    self.functions[invocation.target.0]
                        .expect("a tail-call target has a function index"),
                ),
            }
            .encode(&mut self.bytes);
        }
    }

    pub(super) fn emit_captures(&mut self, site: Site) {
        if let Some(captures) = self.placement.captures.get(&site) {
            for index in 0..captures.len() {
                let id = self.placement.captures[&site][index];
                if !self.emitted[id] {
                    self.evaluate(id, true);
                }
            }
        }
    }

    fn live_outputs(&self, operation: &Operation) -> Vec<usize> {
        operation
            .branch_outputs()
            .iter()
            .copied()
            .filter(|&id| self.placement.slots[id].is_some())
            .collect()
    }

    pub(super) fn begin_control(
        &mut self,
        instruction: Instruction<'_>,
        target: Option<Target>,
        outputs: &[usize],
    ) {
        instruction.encode(&mut self.bytes);
        self.labels.push(ControlLabel {
            target,
            outputs: outputs.to_vec(),
        });
    }

    pub(super) fn end_control(&mut self) {
        self.labels
            .pop()
            .expect("an ending control has an open label");
        Instruction::End.encode(&mut self.bytes);
    }
}
