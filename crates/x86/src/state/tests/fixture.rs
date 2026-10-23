use crate::test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
use crate::CpuState;

#[track_caller]
pub(super) fn assert_result(
    module: &TestModule,
    initial: &CpuState,
    arguments: &[i32],
    expected: &CpuState,
    result: i64,
) {
    let input = Input {
        arguments: arguments.iter().copied().map(Argument::I32).collect(),
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        module.observe(&input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(result)]),
                snapshot: Snapshot {
                    cpu: expected.to_bytes().to_vec(),
                    guest: None,
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "arguments {arguments:?}"
    );
}
