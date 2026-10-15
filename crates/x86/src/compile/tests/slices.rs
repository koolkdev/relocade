use std::sync::OnceLock;

use crate::support::execution::Frontend;
use crate::{
    support::{
        execution::test_frontends,
        step::{Argument, Engine, Event, Input, Outcome, TestModule},
    },
    Compiler, CpuState, SegmentProfile, SLICE_EXHAUSTED,
};

fn interpreter() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| {
        TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .with_execution_budget()
                .compile_interpreter()
                .unwrap(),
        )
    })
}

fn step() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| {
        TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .with_execution_budget()
                .compile_interpreter_step()
                .unwrap(),
        )
    })
}

fn input(code: &[u8], cpu: CpuState, budgets: &[u32]) -> Input {
    let mut input = Input::new(&cpu.to_bytes());
    input.guest.push((0x1000, code.to_vec()));
    for page in 1..4u32 {
        input
            .machine
            .push((page * 4, ((page << 12) | 3).to_le_bytes().to_vec()));
    }
    input.budgets = budgets.to_vec();
    input.observe_guest = true;
    input
}

fn run(
    engine: Engine,
    frontend: Frontend,
    code: &[u8],
    limit: u32,
    input: &Input,
) -> Vec<(u64, CpuState)> {
    let block;
    let module = match frontend {
        Frontend::Block => {
            block = TestModule::new(
                &Compiler::new(SegmentProfile::Flat32)
                    .with_execution_budget()
                    .compile_block(0x1000, code, limit)
                    .unwrap(),
            )
            .with_interpreter(interpreter());
            &block
        }
        Frontend::Interpreter => interpreter(),
    };
    let observation = engine.observe(module, input, input.budgets.len());
    observation
        .events
        .into_iter()
        .filter_map(|event| match event {
            Event::Return {
                outcome: Outcome::Returned(values),
                snapshot,
            } => {
                let [Argument::I64(result)] = values.as_slice() else {
                    panic!("expected i64 return")
                };
                Some((
                    *result as u64,
                    CpuState::from_bytes(snapshot.cpu.try_into().unwrap()),
                ))
            }
            Event::Dispatch { .. } => None,
            other => panic!("unexpected event {other:?}"),
        })
        .collect()
}

fn initial() -> CpuState {
    CpuState {
        eip: 0x1000,
        ..CpuState::default()
    }
}

mod control;
mod repetition;
