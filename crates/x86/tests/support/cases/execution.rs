//! Executes one-instruction cases through the applicable entry profiles.

use std::collections::HashMap;
use wasm86_x86::{compile_block_from_bytes, SegmentProfile};
use wasmparser::Validator;

use super::{expectations, observation::FlagObservations, Frontends, InstructionCase};
pub(super) use crate::support::step::Engine;
use crate::support::step::TestModule;

pub(super) fn check(cases: &[InstructionCase], engine: Engine) {
    assert!(
        !cases.is_empty(),
        "an instruction case group must not be empty"
    );
    let mut blocks = HashMap::new();
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
        let block = matches!(case.frontends, Frontends::All).then(|| {
            blocks
                .entry((case.initial.eip, case.code.to_vec()))
                .or_insert_with(|| {
                    let module = compile_block_from_bytes(case.initial.eip, &case.code, 1)
                        .unwrap_or_else(|error| {
                            panic!("{}: compiling the case: {error:?}", case.name)
                        });
                    Validator::new()
                        .validate_all(&module.bytes)
                        .unwrap_or_else(|error| {
                            panic!("{}: validating the block: {error}", case.name)
                        });
                    TestModule::new(&module)
                })
        });
        let flat_interpreter = matches!(case.frontends, Frontends::All)
            .then(|| ("flat interpreter", TestModule::interpreter()));
        for (frontend, module) in std::iter::once((
            "segmented interpreter",
            TestModule::interpreter_with_profile(SegmentProfile::Segmented32),
        ))
        .chain(flat_interpreter)
        .chain(block.map(|module| ("block", &*module)))
        {
            let context = format!("{} [{engine:?}, {frontend}]", case.name);
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
