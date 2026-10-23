//! Explicit compiled completion and precise interpreter-restart expectations.

use relocade_x86::CompiledModule;

use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::TestModule,
};

pub(crate) struct CompiledExecution {
    block: TestModule,
    interpreter: &'static TestModule,
}

impl CompiledExecution {
    pub(crate) fn new(module: &CompiledModule) -> Self {
        let target = module
            .execution_target
            .expect("a compiled x86 execution target");
        Self {
            block: TestModule::new(module),
            interpreter: TestModule::interpreter_with_target(target),
        }
    }

    /// Require the unlinked block to reach this exit without an interpreter handoff.
    #[track_caller]
    pub(crate) fn check_native(&self, name: &str, image: &Image, result: &Step<'_>) {
        assert!(!matches!(result.exit, Exit::Interpret));
        assert_eq!(
            self.block.observe(&image.input(), 1),
            expected(image, std::slice::from_ref(result)),
            "{name}: native completion",
        );
    }

    /// Require a handoff after exactly the authored prefix, then complete the
    /// excluded instruction through the matching interpreter. Each RAM patch
    /// describes only its own instruction; earlier writes remain observable.
    #[track_caller]
    pub(crate) fn check_restart(
        &self,
        name: &str,
        image: &Image,
        prefix: &[Step<'_>],
        result: &Step<'_>,
    ) {
        assert!(prefix
            .iter()
            .all(|step| matches!(step.exit, Exit::Dispatch(_))));
        let cpu = prefix.last().map_or(image.cpu, |step| step.cpu);
        let mut ram = prefix
            .iter()
            .flat_map(|step| step.ram.iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(
            self.block.observe(&image.input(), 1),
            expected(
                image,
                &[Step {
                    cpu,
                    ram: &ram,
                    exit: Exit::Interpret,
                }],
            ),
            "{name}: restart checkpoint",
        );
        ram.extend_from_slice(result.ram);
        self.check_linked(
            name,
            image,
            &Step {
                cpu: result.cpu,
                ram: &ram,
                exit: result.exit,
            },
        );
    }

    /// Check linked execution when the case does not require a particular path.
    #[track_caller]
    pub(crate) fn check_linked(&self, name: &str, image: &Image, result: &Step<'_>) {
        assert!(!matches!(result.exit, Exit::Interpret));
        assert_eq!(
            self.block
                .clone()
                .with_interpreter(self.interpreter)
                .observe(&image.input(), 1),
            expected(image, std::slice::from_ref(result)),
            "{name}: linked completion",
        );
    }
}

#[cfg(test)]
mod tests;
