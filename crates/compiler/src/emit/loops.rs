//! Native Wasm loop parameters, result exits and backedge local lifetimes.
use wasm_encoder::Instruction;

use super::{wasm_type, Scheduler};
use crate::control::{Region, Site, Target};

impl Scheduler<'_> {
    pub(super) fn loop_region(
        &mut self,
        initial: &[usize],
        inputs: &[usize],
        region: &Region,
        site: Site,
        outputs: &[usize],
    ) {
        let results: Vec<_> = outputs
            .iter()
            .map(|&id| wasm_type(self.body.values[id].ty))
            .collect();
        let parameters: Vec<_> = inputs
            .iter()
            .map(|&id| wasm_type(self.body.values[id].ty))
            .collect();
        let result_type = self.types.block(&results);
        self.begin_control(
            Instruction::Block(result_type),
            Some(Target::exit(site)),
            outputs,
        );
        for &seed in initial {
            self.value(seed);
        }
        let before = self.emitted.clone();
        let loop_type = self.types.control(&parameters, &results);
        self.begin_control(
            Instruction::Loop(loop_type),
            Some(Target::entry(site)),
            inputs,
        );
        let start = self.events.len();
        // Both initial entry and backedges arrive with the complete input tuple
        // on the stack. Save in reverse order only after every input is evaluated.
        for &input in inputs.iter().rev() {
            self.completed(input, true);
        }
        self.region(region, Some(Target::exit(site)));
        self.end_control();
        self.loop_ranges.push((start, self.events.len()));
        self.end_control();
        // Loop-local captures describe one iteration, not an outer definition.
        self.emitted = before;
    }
}
