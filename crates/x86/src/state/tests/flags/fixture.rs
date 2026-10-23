use crate::test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
use crate::CpuState;

pub(super) fn initial_cpu() -> CpuState {
    let mut cpu = CpuState::filled(0xa5);
    cpu.flags.status_source.kind = 9;
    cpu.flags.status_source.left = 7;
    cpu.flags.status_source.right = 8;
    cpu.eip = 0x1000;
    cpu.instruction_count = u64::MAX;
    cpu
}

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

/// Code-body position of the exported state consumer, independent of helper order.
pub(super) fn consumer_index(bytes: &[u8]) -> usize {
    use wasmparser::{ExternalKind, Parser, Payload, TypeRef};
    let mut imports = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                imports += section
                    .into_iter()
                    .filter(|import| matches!(import.as_ref().unwrap().ty, TypeRef::Func(_)))
                    .count();
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.kind == ExternalKind::Func && export.name == "run" {
                        return export.index as usize - imports;
                    }
                }
            }
            _ => {}
        }
    }
    panic!("state fixture exports its consumer as run")
}

pub(super) fn consumer_ops(bytes: &[u8]) -> Vec<wasmparser::Operator<'_>> {
    use wasmparser::{Parser, Payload, Validator};
    Validator::new().validate_all(bytes).unwrap();
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body),
            _ => None,
        })
        .nth(consumer_index(bytes))
        .unwrap()
        .get_operators_reader()
        .unwrap()
        .into_iter()
        .map(Result::unwrap)
        .collect()
}
