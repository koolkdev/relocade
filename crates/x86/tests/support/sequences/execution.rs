use super::{Checkpoint, SequenceCase};
use crate::support::{
    blocks::BlockModules,
    cases::{
        expectations::{self, Boundary},
        observation::FlagObservations,
        ExpectedExit, ExpectedFlags, ExpectedState, FlagExpectation, Flags,
    },
    execution::Frontend,
    step::{Engine, TestModule},
};

pub(crate) fn check(cases: &[SequenceCase], engine: Engine, frontend: Frontend) {
    assert!(!cases.is_empty(), "a sequence group must not be empty");
    let mut blocks = BlockModules::default();
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
        for profile in case.profiles.for_cpu(&initial.cpu) {
            if matches!(frontend, Frontend::Block) {
                let block = blocks.get(&initial.cpu, &code, limit, profile);
                let execution = machine.run(block, engine);
                let context = format!("{} [{engine:?}, {profile:?} block]", case.name);
                let mut final_state = FinalExpectation::new(case);
                for checkpoint in &case.checkpoints {
                    final_state.append(checkpoint);
                }
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
                continue;
            }
            let executions = machine.run_many(
                TestModule::interpreter_with_profile(profile),
                engine,
                case.checkpoints.len(),
            );
            let mut before = &initial;
            for (index, (checkpoint, execution)) in
                case.checkpoints.iter().zip(&executions).enumerate()
            {
                let context = format!(
                    "{} [{engine:?}, {profile:?} interpreter, checkpoint {}]",
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
                before = &execution.state;
            }
        }
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
        ExpectedExit::DivideError
        | ExpectedExit::BoundRangeExceeded
        | ExpectedExit::GeneralProtection { .. }
        | ExpectedExit::StackFault { .. }
        | ExpectedExit::PageFault { .. } => Boundary {
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
        for &(flag, value) in &checkpoint.expected.direct_flags {
            self.expected.expect_direct_flag(flag, value);
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

#[cfg(test)]
mod tests {
    use super::{FinalExpectation, SequenceCase};
    use crate::flags::Flag;
    use crate::support::sequences::Checkpoint;

    #[test]
    fn direct_flag_composition_keeps_untouched_values_and_replaces_only_the_same_flag() {
        let case = SequenceCase::preserving_flags("direct flag composition");
        let mut final_state = FinalExpectation::new(&case);
        final_state.append(
            &Checkpoint::preserving_flags(&[0x9d])
                .expect_direct_flag(Flag::AC, true)
                .expect_direct_flag(Flag::ID, false),
        );
        final_state.append(
            &Checkpoint::preserving_flags(&[0x66, 0x9d])
                .expect_direct_flag(Flag::TF, false)
                .expect_direct_flag(Flag::DF, true)
                .expect_direct_flag(Flag::NT, true),
        );
        final_state
            .append(&Checkpoint::preserving_flags(&[0xfc]).expect_direct_flag(Flag::DF, false));
        final_state.complete_flags();
        assert_eq!(final_state.expected.direct_flags.len(), 5);
        for expected in [
            (Flag::AC, true),
            (Flag::ID, false),
            (Flag::TF, false),
            (Flag::DF, false),
            (Flag::NT, true),
        ] {
            assert!(final_state.expected.direct_flags.contains(&expected));
        }
        assert!(final_state.expected.flags.preserves_record());
    }
}
