use super::super::{Cpu, State};
use crate::exception::Exception;
use crate::flags::Flag;
use crate::test_step::{
    Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule,
};
use crate::{CompiledModule, CpuState, Gpr32};
use wasm86_compiler::{Program, Signature, Type, I1, I32};

fn conditional_fault(completed: u32) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1, Type::I32, Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let denied = body.parameter::<I1>(0)?;
                let restart_eip = body.parameter::<I32>(1)?;
                let linear_address = body.parameter::<I32>(2)?;
                let mut state = State::new(&cpu);
                let eax = state.read_register(&mut body, Gpr32::Eax)?;
                state.write_register(&mut body, Gpr32::Eax, eax.add(1))?;
                state.write_flag(&mut body, Flag::DF, false)?;
                body.if_(denied, |fault_body| {
                    state.fault(
                        fault_body,
                        &restart_eip,
                        completed,
                        Exception::PageFault {
                            linear_address,
                            error_code: 3.into(),
                        },
                    )
                })?;
                state.write_register(&mut body, Gpr32::Eax, 42)?;
                state.write_register(&mut body, Gpr32::Edi, 77)?;
                state.write_flag(&mut body, Flag::DF, true)?;
                state.publish(&mut body, restart_eip.add(2), completed + 1)?;
                body.return_(7)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_fault_publication(engine: Engine) {
    let mut initial = CpuState {
        eip: 0x1000,
        instruction_count: 0xffff_ffff,
        ..CpuState::filled(0xa5)
    };
    initial.flags.status_source.kind = 9;
    initial.registers.eax = 0xffff_ffff;
    for (completed, fault_count, continued_count) in [(0, 0xffff_ffff, 0), (2, 1, 2)] {
        let module = TestModule::new(&conditional_fault(completed));
        for denied in [false, true] {
            let mut expected = initial;
            let result = if denied {
                expected.registers.eax = 0;
                expected.flags.bytes.df = 0;
                expected.eip = 0x8000_1000;
                expected.instruction_count = fault_count;
                0x0004_0003_f123_4567_i64
            } else {
                expected.registers.eax = 42;
                expected.registers.edi = 77;
                expected.flags.bytes.df = 1;
                expected.eip = 0x8000_1002;
                expected.instruction_count = continued_count;
                7
            };
            let input = Input {
                arguments: vec![
                    Argument::I32(i32::from(denied)),
                    Argument::I32(0x8000_1000_u32 as i32),
                    Argument::I32(0xf123_4567_u32 as i32),
                ],
                ..Input::new(&initial.to_bytes())
            };
            assert_eq!(
                engine.observe(&module, &input, 1),
                Observation {
                    events: vec![Event::Return {
                        outcome: Outcome::Returned(vec![Argument::I64(result)]),
                        snapshot: Snapshot {
                            cpu: expected.to_bytes().to_vec(),
                            guest: None
                        },
                    }],
                    guest_unchanged: true,
                    machine_unchanged: true,
                },
                "completed {completed}, denied {denied}"
            );
        }
    }
}

#[test]
fn faults_publish_their_boundary_and_preserve_the_continuation_in_wasmtime() {
    check_fault_publication(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn faults_publish_their_boundary_and_preserve_the_continuation_in_v8() {
    check_fault_publication(Engine::V8);
}
