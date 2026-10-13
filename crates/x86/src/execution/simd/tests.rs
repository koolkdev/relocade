use super::*;
use crate::{
    runtime::Runtime,
    state::Cpu,
    test_step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CompiledModule, CpuState, ExecutionProfile, SegmentDefaultSize, SegmentProfile,
};
use wasm86_compiler::{Program, Signature, Type, I1};

fn exception_module() -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let runtime = Runtime::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1, Type::I1],
                results: vec![Type::I64],
            },
            |body| {
                let exceptions = SimdExceptions {
                    invalid: body.parameter::<I1>(0)?,
                    denormal: body.parameter::<I1>(1)?,
                };
                let mut execution = ExecutionBuilder::new(
                    body,
                    &cpu,
                    None,
                    runtime,
                    0x1000,
                    ExecutionProfile::Protected(SegmentProfile::Flat32),
                )?;
                let (nop, _) =
                    crate::decode::snapshot(&[0x90], 0x1000, SegmentDefaultSize::Bits32).unwrap();
                execution.execute(nop)?;
                execution.record_simd_exceptions(&exceptions)?;
                execution.write_xmm(RegisterCode::from_code(0), 0_u128.into())?;
                execution.complete(|body, _| body.return_(7))
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        execution_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn exceptions(engine: Engine) {
    let module = TestModule::new(&exception_module());
    for (mxcsr, invalid, denormal, expected_mxcsr, fault) in [
        (0x1f80, false, false, 0x1f80, false),
        (0x1f80, true, false, 0x1f81, false),
        (0x1f80, false, true, 0x1f82, false),
        (0x1f80, true, true, 0x1f83, false),
        (0x1f00, true, false, 0x1f01, true),
        (0x1e80, false, true, 0x1e82, true),
        (0x1f00, true, true, 0x1f03, true),
        (0x1e00, true, true, 0x1e03, true),
        (0x0003, false, false, 0x0003, false),
        (0x1e81, true, false, 0x1e81, false),
        (0xa5a5_9fc4, true, false, 0xa5a5_9fc5, false),
    ] {
        let mut initial = CpuState {
            eip: 0x1000,
            instruction_count: 7,
            ..CpuState::default()
        };
        initial.simd.mxcsr = mxcsr;
        initial.simd.xmm[0] = [0xa5; 16];
        let mut expected = initial;
        expected.eip = 0x1001;
        expected.instruction_count = 8;
        expected.simd.mxcsr = expected_mxcsr;
        if !fault {
            expected.simd.xmm[0] = [0; 16];
        }
        let input = Input {
            arguments: vec![
                Argument::I32(invalid.into()),
                Argument::I32(denormal.into()),
            ],
            ..Input::new(&initial.to_bytes())
        };
        assert_eq!(
            engine.observe(&module, &input, 1),
            Observation {
                events: vec![Event::Return {
                    outcome: Outcome::Returned(vec![Argument::I64(if fault {
                        0x0200_0000_0000_0000
                    } else {
                        7
                    })]),
                    snapshot: Snapshot {
                        cpu: expected.to_bytes().to_vec(),
                        guest: None,
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            },
            "MXCSR {mxcsr:#x}, invalid {invalid}, denormal {denormal}"
        );
    }
}

#[test]
fn simd_status_precedes_faults_and_destination_updates() {
    exceptions(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_simd_status_precedes_faults_and_destination_updates() {
    exceptions(Engine::V8);
}
