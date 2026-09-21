//! Executes one-instruction cases through the applicable entry profiles.

use super::{expectations, observation::FlagObservations, InstructionCase};
use crate::support::{
    blocks::BlockModules,
    execution::Frontend,
    step::{Engine, TestModule},
};

pub(crate) fn check(cases: &[InstructionCase], engine: Engine, frontend: Frontend) {
    assert!(
        !cases.is_empty(),
        "an instruction case group must not be empty"
    );
    let mut blocks = BlockModules::default();
    let mut flags = FlagObservations::new(engine);
    for case in cases {
        expectations::validate(case);
        let machine = case.initial.machine(&case.code);
        let initial = machine.state();
        if case.initial.flags.record().is_some() {
            if let Some(values) = case.initial.flags.logical() {
                flags.initial(
                    initial.cpu,
                    values,
                    format!("{} [{engine:?}, initial stored flags]", case.name),
                );
            }
        }
        for profile in case.profiles.for_cpu(&initial.cpu) {
            let module = match frontend {
                Frontend::Block => blocks.get(&initial.cpu, &case.code, 1, profile),
                Frontend::Interpreter => TestModule::interpreter_with_profile(profile),
            };
            let context = format!("{} [{engine:?}, {profile:?} {frontend:?}]", case.name);
            let execution = machine.run(module, engine);
            expectations::check_state(case, &initial, &execution, &context);
            flags.after(
                execution.state.cpu,
                initial.cpu,
                case.expected.flags,
                context,
            );
        }
    }
    flags.finish();
}
