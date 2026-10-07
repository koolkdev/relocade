//! Independently selectable frontends, with the same cases in both engines.

use wasm86_x86::{CpuState, ExecutionProfile};

use super::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, TestModule},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Frontend {
    Block,
    Interpreter,
}

/// Executes a straight-line sequence against complete CPU and memory images.
/// Each step describes one instruction and only the RAM changes made there.
/// Runtime decoding observes every step. A snapshot block observes its final
/// exit or stops at a handoff, checking the complete independently authored
/// prefix state. Linked interpreter completion is covered by integration tests.
pub(crate) struct ImageSequences {
    engine: Engine,
    frontend: Frontend,
    profile: ExecutionProfile,
    blocks: BlockModules,
}

impl ImageSequences {
    pub(crate) fn new(
        engine: Engine,
        frontend: Frontend,
        profile: impl Into<ExecutionProfile>,
    ) -> Self {
        Self {
            engine,
            frontend,
            profile: profile.into(),
            blocks: BlockModules::default(),
        }
    }

    pub(crate) fn check(&mut self, name: &str, code: &[u8], image: &Image, steps: &[Step<'_>]) {
        let last = steps.last().expect("a sequence contains an instruction");
        let mut wanted = expected(image, steps);
        let (module, invocations) = match self.frontend {
            Frontend::Block => {
                // The composed final snapshot retains writes from earlier
                // instructions, including overwritten or restored bytes.
                let final_events = if matches!(last.exit, Exit::Dispatch(_)) {
                    2
                } else {
                    1
                };
                wanted.events.drain(..wanted.events.len() - final_events);
                (
                    self.blocks
                        .get(&image.cpu, code, steps.len() as u32, self.profile),
                    1,
                )
            }
            Frontend::Interpreter => (
                TestModule::interpreter_with_profile(self.profile),
                steps.len(),
            ),
        };
        let actual = self.engine.observe(module, &image.input(), invocations);
        if let (Frontend::Block, Some(Event::Interpret { snapshot })) =
            (self.frontend, actual.events.first())
        {
            let cpu = CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap());
            let completed = cpu
                .instruction_count
                .wrapping_sub(image.cpu.instruction_count) as usize;
            assert!(
                completed < steps.len(),
                "{name}: handoff must precede an instruction"
            );
            let prefix = &steps[..completed];
            assert!(prefix
                .iter()
                .all(|step| matches!(step.exit, Exit::Dispatch(_))));
            let cpu = prefix.last().map_or(image.cpu, |step| step.cpu);
            let ram = prefix
                .iter()
                .flat_map(|step| step.ram.iter().copied())
                .collect::<Vec<_>>();
            wanted = expected(
                image,
                &[Step {
                    cpu,
                    ram: &ram,
                    exit: Exit::Interpret,
                }],
            );
        }
        assert_eq!(actual, wanted, "{name}: {:?}", self.frontend);
    }
}

macro_rules! test_frontends {
    ($group:ident, $cases:expr, $check:path) => {
        $crate::support::execution::test_frontends!(@tests $group,
            |engine, frontend| $check(&($cases), engine, frontend));
    };
    ($group:ident, $check:path) => {
        $crate::support::execution::test_frontends!(@tests $group,
            |engine, frontend| $check(engine, frontend));
    };
    (@tests $group:ident, $run:expr) => {
        mod $group {
            use super::*;
            use $crate::support::{execution::Frontend, step::Engine};

            #[test]
            fn block() {
                ($run)(Engine::Wasmtime, Frontend::Block);
            }

            #[test]
            fn interpreter() {
                ($run)(Engine::Wasmtime, Frontend::Interpreter);
            }

            #[test]
            #[ignore = "requires Node.js; run the explicit V8 lane"]
            fn v8_block() {
                ($run)(Engine::V8, Frontend::Block);
            }

            #[test]
            #[ignore = "requires Node.js; run the explicit V8 lane"]
            fn v8_interpreter() {
                ($run)(Engine::V8, Frontend::Interpreter);
            }
        }
    };
}

pub(crate) use test_frontends;
