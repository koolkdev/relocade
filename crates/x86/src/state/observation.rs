//! Read-only logical flag observations for instruction test cases.

use crate::FlagBytes;
use wasm86_compiler::{BuildError, Program, Signature, Type};

use crate::flags::FlagMask;
use crate::CompiledModule;

use super::Cpu;

/// Reads CF, PF, AF, ZF, SF and OF from the supplied CPU backing without
/// normalizing or publishing that backing. The host receives six Boolean results.
pub(crate) fn compile_flag_observer() -> Result<CompiledModule, BuildError> {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let observer = program.function(
        Signature {
            parameters: vec![],
            results: vec![Type::I1; 6],
        },
        |mut body| {
            let flags = cpu.read_flags(&mut body, FlagMask::STATUS)?;
            body.return_(
                flags
                    .into_iter()
                    .map(|flag| flag.expect("the complete flag mask requests every flag"))
                    .collect::<Vec<_>>(),
            )
        },
    )?;
    program.export("observe_flags", observer)?;
    Ok(CompiledModule {
        segment_profile: None,
        bytes: program.compile()?,
        entry: "observe_flags".into(),
    })
}

#[test]
fn observer_returns_all_six_logical_flags_without_changing_backing_bytes() {
    use crate::test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
    use crate::CpuState;

    struct Case {
        name: &'static str,
        kind: u8,
        left: u32,
        right: u32,
        stored_status: [u8; 6],
        expected: [i32; 6],
    }
    let module = TestModule::new(&compile_flag_observer().unwrap());
    for case in [
        Case {
            name: "concrete flags use their low bits",
            kind: 0,
            left: 0x1234_5678,
            right: 0x8765_4321,
            stored_status: [0xfe, 0x80, 0x7e, 0xff, 0xfc, 0x81],
            expected: [0, 0, 0, 1, 0, 1],
        },
        Case {
            name: "byte 255+1 supplies carry and auxiliary carry",
            kind: 2,
            left: 0x1234_56ff,
            right: 0x8765_4301,
            stored_status: [0xfe, 0xfe, 0xfe, 0xfe, 0xff, 0xff],
            expected: [1, 1, 1, 1, 0, 0],
        },
        Case {
            name: "word 0x8000-1 supplies overflow and auxiliary carry",
            kind: 5,
            left: 0x1234_8000,
            right: 0x8765_0001,
            stored_status: [0xff, 0xfe, 0xfe, 0xff, 0xff, 0xfe],
            expected: [0, 1, 1, 0, 0, 1],
        },
        Case {
            name: "dword logic uses its result and the zero auxiliary policy",
            kind: 11,
            left: 0x8000_0000,
            right: 0x8765_4321,
            stored_status: [0xff, 0xfe, 0xff, 0xff, 0xfe, 0xff],
            expected: [0, 1, 0, 0, 1, 0],
        },
    ] {
        let mut cpu = CpuState::filled(0xa5);
        cpu.flags.status_source.kind = case.kind;
        cpu.flags.status_source.left = case.left;
        cpu.flags.status_source.right = case.right;
        let [cf, pf, af, zf, sf, of] = case.stored_status;
        cpu.flags.bytes = FlagBytes {
            cf,
            pf,
            af,
            zf,
            sf,
            of,
            ..cpu.flags.bytes
        };
        let bytes = cpu.to_bytes();
        assert_eq!(
            module.observe(&Input::new(&bytes), 1),
            Observation {
                events: vec![Event::Return {
                    outcome: Outcome::Returned(
                        case.expected.into_iter().map(Argument::I32).collect(),
                    ),
                    snapshot: Snapshot {
                        cpu: bytes.to_vec(),
                        guest: None,
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            },
            "{}",
            case.name,
        );
    }
}
