//! Native Wasm loop parameters and result exits.

use super::{wasm_type, Instruction, Scheduler};
use crate::body::{Block, Site, Target};

impl Scheduler<'_> {
    pub(super) fn loop_block(
        &mut self,
        initial: &[usize],
        inputs: &[usize],
        block: &Block,
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
        self.values(initial.iter().copied());
        let before = self.available.clone();
        let loop_type = self.types.control(&parameters, &results);
        self.begin_control(
            Instruction::Loop(loop_type),
            Some(Target::entry(site)),
            inputs,
        );
        // Both initial entry and backedges arrive with the complete input tuple
        // on the stack. Save in reverse order only after every input is evaluated.
        self.save_results(inputs);
        self.block(block, Some(Target::exit(site)));
        self.end_control();
        self.end_control();
        // Loop-local captures describe one iteration, not an outer definition.
        self.available = before;
    }
}
