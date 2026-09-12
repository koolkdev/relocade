use std::collections::HashMap;
use wasm86_x86::compile_block_from_bytes;
use wasmparser::Validator;

use super::{Checkpoint, SequenceCase};
use crate::support::{
    cases::{
        expectations::{self, Boundary},
        observation::FlagObservations,
        ExpectedExit, ExpectedFlags, ExpectedState, FlagExpectation, Flags,
    },
    step::{Engine, TestModule},
};

pub(super) fn check(cases: &[SequenceCase], engine: Engine) {
    assert!(!cases.is_empty(), "a sequence group must not be empty");
    let mut blocks = HashMap::new();
    let mut flags = FlagObservations::new(engine);
    for case in cases {
        validate(case);
        let mut code = case
            .checkpoints
            .iter()
            .flat_map(|step| step.code.iter().copied())
            .collect::<Vec<_>>();
        code.extend_from_slice(&case.trailing_code);
        let limit = case.checkpoints.len() as u32 + case.trailing_instructions;
        let machine = case.initial.machine(&code);
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
        let executions =
            machine.run_many(TestModule::interpreter(), engine, case.checkpoints.len());
        let mut before = &initial;
        let mut final_state = FinalExpectation::new(case);
        for (index, (checkpoint, execution)) in case.checkpoints.iter().zip(&executions).enumerate()
        {
            let context = format!(
                "{} [{engine:?}, interpreter, checkpoint {}]",
                case.name,
                index + 1
            );
            let boundary = boundary(checkpoint, before.cpu.eip);
            expectations::check_checkpoint(
                &checkpoint.expected,
                boundary,
                before,
                execution,
                &context,
            );
            flags.after(
                execution.state.cpu,
                before.cpu,
                checkpoint.expected.flags,
                context,
            );
            final_state.append(checkpoint);
            before = &execution.state;
        }
        let block = blocks
            .entry((case.initial.eip, code.clone(), limit))
            .or_insert_with(|| {
                let module = compile_block_from_bytes(case.initial.eip, &code, limit)
                    .unwrap_or_else(|error| {
                        panic!("{}: compiling the sequence: {error:?}", case.name)
                    });
                Validator::new()
                    .validate_all(&module.bytes)
                    .unwrap_or_else(|error| {
                        panic!("{}: validating the sequence: {error}", case.name)
                    });
                TestModule::new(&module)
            });
        let execution = machine.run(block, engine);
        let context = format!("{} [{engine:?}, block]", case.name);
        final_state.complete_flags();
        expectations::check_checkpoint(
            &final_state.expected,
            final_state.boundary,
            &initial,
            &execution,
            &context,
        );
        flags.after(
            execution.state.cpu,
            initial.cpu,
            final_state.expected.flags,
            context,
        );
    }
    flags.finish();
}

fn validate(case: &SequenceCase) {
    assert!(
        !case.name.is_empty() && !case.checkpoints.is_empty(),
        "a named sequence needs checkpoints"
    );
    expectations::validate_initial(&case.name, &case.initial);
    let mut logical = case.initial.flags.logical().is_some();
    for (index, checkpoint) in case.checkpoints.iter().enumerate() {
        assert!(
            !checkpoint.code.is_empty() && checkpoint.code.len() <= 15,
            "{}: each checkpoint contains one instruction of at most fifteen bytes",
            case.name
        );
        expectations::validate_expected(&case.name, &checkpoint.expected);
        if let ExpectedFlags::Logical { values, .. } = checkpoint.expected.flags {
            assert!(logical || !values.values().iter().any(|rule| matches!(rule, FlagExpectation::Preserved)),
                "{}: opaque initial flags must be replaced before logical preservation can be checked", case.name);
            logical = true;
        }
        assert!(
            !case.preserve_flags || checkpoint.expected.flags.preserves_record(),
            "{}: a preserving-flags sequence must preserve the record at every checkpoint",
            case.name
        );
        assert!(
            index + 1 == case.checkpoints.len()
                || matches!(checkpoint.expected.exit, ExpectedExit::Fallthrough),
            "{}: a fault or branch ends the sequence's snapshot block",
            case.name
        );
    }
    assert!(
        case.trailing_code.is_empty() == (case.trailing_instructions == 0),
        "{}: trailing code needs its instruction count",
        case.name
    );
    if !case.trailing_code.is_empty() {
        assert!(
            !matches!(
                case.checkpoints.last().unwrap().expected.exit,
                ExpectedExit::Fallthrough
            ),
            "{}: an unexecuted suffix requires a final fault or branch",
            case.name
        );
    }
}

fn boundary(checkpoint: &Checkpoint, entry: u32) -> Boundary {
    match checkpoint.expected.exit {
        ExpectedExit::Fallthrough => Boundary {
            eip: entry.wrapping_add(checkpoint.code.len() as u32),
            retired: 1,
        },
        ExpectedExit::Dispatch(target) => Boundary {
            eip: target,
            retired: 1,
        },
        ExpectedExit::DivideError | ExpectedExit::PageFault { .. } => Boundary {
            eip: entry,
            retired: 0,
        },
    }
}

/// Compose only authored effects. Unknown result bits stay unknown until an
/// explicit later write supplies them; no instruction semantics are evaluated.
struct FinalExpectation {
    expected: ExpectedState,
    boundary: Boundary,
    flags: [Option<bool>; 6],
    preserve_record: bool,
}

impl FinalExpectation {
    fn new(case: &SequenceCase) -> Self {
        Self {
            expected: ExpectedState::new(ExpectedFlags::Preserved),
            boundary: Boundary {
                eip: case.initial.eip,
                retired: 0,
            },
            flags: case
                .initial
                .flags
                .logical()
                .map_or([None; 6], |flags| flags.values().map(Some)),
            preserve_record: true,
        }
    }

    fn append(&mut self, checkpoint: &Checkpoint) {
        let next = boundary(checkpoint, self.boundary.eip);
        self.boundary.eip = next.eip;
        self.boundary.retired += next.retired;
        self.expected.exit = checkpoint.expected.exit;
        if let Some(direction) = checkpoint.expected.direction_flag {
            self.expected.direction_flag = Some(direction);
        }
        for &(register, value) in &checkpoint.expected.registers {
            self.expected.registers.retain(|(old, _)| *old != register);
            self.expected.registers.push((register, value));
        }
        self.expected
            .memory
            .extend(checkpoint.expected.memory.clone());
        self.preserve_record &= checkpoint.expected.flags.preserves_record();
        if let ExpectedFlags::Logical { values, .. } = checkpoint.expected.flags {
            for (value, rule) in self.flags.iter_mut().zip(values.values()) {
                match rule {
                    FlagExpectation::Set => *value = Some(true),
                    FlagExpectation::Clear => *value = Some(false),
                    FlagExpectation::Undefined => *value = None,
                    FlagExpectation::Preserved => {}
                }
            }
        }
    }

    fn complete_flags(&mut self) {
        if self.preserve_record {
            self.expected.flags = ExpectedFlags::Preserved;
        } else {
            let rule = |value| match value {
                Some(true) => FlagExpectation::Set,
                Some(false) => FlagExpectation::Clear,
                None => FlagExpectation::Undefined,
            };
            self.expected.flags = ExpectedFlags::Logical {
                values: Flags {
                    cf: rule(self.flags[0]),
                    pf: rule(self.flags[1]),
                    af: rule(self.flags[2]),
                    zf: rule(self.flags[3]),
                    sf: rule(self.flags[4]),
                    of: rule(self.flags[5]),
                },
                preserve_record: false,
            };
        }
    }
}
