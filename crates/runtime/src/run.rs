use crate::{
    backend::{Backend, EntryRequest, Instance},
    Error, Result, Session,
};
use wasm86_machine::Environment;
use wasm86_x86::ExecutionExit;

impl<B: Backend, D: Environment + 'static> Session<B, D> {
    /// Runs until the instruction budget is consumed at a block boundary or an
    /// execution exit occurs. Whole blocks may exceed the budget; there is no
    /// fixed overshoot or elapsed-time guarantee. A zero budget does not enter Wasm.
    /// The absolute target saturates at `u64::MAX`; counter wrap is not supported.
    pub fn run(&mut self, instruction_budget: u32) -> Result<ExecutionExit> {
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let instruction_target = self.instruction_target(instruction_budget);
        let word = self.instance.enter(EntryRequest {
            instruction_target,
            observe: false,
        })?;
        ExecutionExit::from_bits(word).ok_or(Error::InvalidExit(word))
    }

    /// Observes the initial state and completed dispatch boundaries. Returns
    /// `None` when the predicate requests a stop, or the execution's exit reason.
    /// Guest faults take precedence; a predicate stop takes precedence over an
    /// exhausted instruction budget. Observer calls do not enter guest code.
    pub fn run_until(
        &mut self,
        instruction_budget: u32,
        mut should_stop: impl FnMut(&Self) -> bool,
    ) -> Result<Option<ExecutionExit>> {
        if should_stop(self) {
            return Ok(None);
        }
        if instruction_budget == 0 {
            return Ok(Some(ExecutionExit::BudgetExhausted));
        }
        let instruction_target = self.instruction_target(instruction_budget);
        loop {
            let word = self.instance.enter(EntryRequest {
                instruction_target,
                observe: true,
            })?;
            let exit = match word {
                0 => None,
                word => Some(ExecutionExit::from_bits(word).ok_or(Error::InvalidExit(word))?),
            };
            if exit.is_some_and(|exit| exit != ExecutionExit::BudgetExhausted) {
                return Ok(exit);
            }
            if should_stop(self) {
                return Ok(None);
            }
            if exit.is_some() {
                return Ok(exit);
            }
        }
    }

    fn instruction_target(&self, budget: u32) -> u64 {
        self.cpu()
            .instruction_count
            .saturating_add(u64::from(budget))
    }
}
