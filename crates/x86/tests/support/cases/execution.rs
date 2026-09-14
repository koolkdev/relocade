//! Executes one-instruction cases through the applicable entry profiles.

use super::{expectations, observation::FlagObservations, InstructionCase};
pub(super) use crate::support::step::Engine;
use crate::support::{blocks::BlockModules, step::TestModule};

pub(super) fn check(cases: &[InstructionCase], engine: Engine) {
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
            let block = blocks.get(&initial.cpu, &case.code, 1, profile);
            for (frontend, module) in [
                ("interpreter", TestModule::interpreter_with_profile(profile)),
                ("block", block),
            ] {
                let context = format!("{} [{engine:?}, {profile:?} {frontend}]", case.name);
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
    }
    flags.finish();
}
