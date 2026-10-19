use super::*;
use crate::{
    runtime::Runtime,
    state::Cpu,
    test_step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CompiledModule, Compiler, CpuState, SegmentDefaultSize, SegmentProfile,
};
use wasm86_compiler::{Program, Signature, Type};

fn exception_module(fallback_enabled: bool) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let runtime = Runtime::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
            |body| {
                let raised = body.parameter::<I32>(0)?;
                let exceptions = SimdExceptions {
                    invalid: raised.and(1).ne(0),
                    denormal: raised.and(2).ne(0),
                    zero_divide: raised.and(4).ne(0),
                    overflow: raised.and(8).ne(0),
                    underflow: raised.and(16).ne(0),
                    precision: raised.and(32).ne(0),
                };
                let mut execution = ExecutionBuilder::new(
                    body,
                    &cpu,
                    None,
                    runtime,
                    0x1000,
                    Compiler::new(SegmentProfile::Flat32).target(),
                )?;
                if fallback_enabled {
                    execution = execution.with_interpreter_fallback();
                }
                let (nop, _) =
                    crate::decode::snapshot(&[0x90], 0x1000, SegmentDefaultSize::Bits32).unwrap();
                let successor = execution.execute(nop)?;
                execution.record_simd_exceptions(&exceptions)?;
                execution.write_xmm(RegisterCode::from_code(0), 0_u128.into())?;
                execution.complete(successor, |body, _| body.return_(7))
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        execution_target: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn exceptions(engine: Engine, fallback_enabled: bool) {
    let module = TestModule::new(&exception_module(fallback_enabled));
    for (mxcsr, raised, expected_mxcsr, fault) in [
        (0x1f80, 0, 0x1f80, false),
        (0x1f80, 1, 0x1f81, false),
        (0x1f80, 2, 0x1f82, false),
        (0x1f80, 3, 0x1f83, false),
        (0x1f80, 4, 0x1f84, false),
        (0x1d80, 4, 0x1d84, true),
        (0x1d80, 0x3c, 0x1d84, true), // Pre-computation zero divide suppresses result flags.
        (0x1d80, 7, 0x1d87, true),    // All pre-computation lane conditions are collected.
        (0x1f80, 0x24, 0x1fa4, false),
        (0x1f00, 1, 0x1f01, true),
        (0x1e80, 2, 0x1e82, true),
        (0x1f00, 3, 0x1f03, true),
        (0x1e00, 3, 0x1e03, true),
        (0x0003, 0, 0x0003, false),
        (0x1e81, 1, 0x1e81, false),
        (0xa5a5_9fc4, 1, 0xa5a5_9fc5, false),
        (0x1f80, 0x20, 0x1fa0, false), // Masked precision stays compiled too.
        (0x1f80, 0x38, 0x1fb8, false),
        (0x0f80, 0x20, 0x0fa0, true),  // Unmasked precision.
        (0x1b80, 0x28, 0x1ba8, true),  // Unmasked overflow retains computed PE.
        (0x1780, 0x10, 0x1790, true),  // Exact tiny result, underflow unmasked.
        (0x1f00, 0x21, 0x1f01, true),  // Invalid suppresses a sibling lane's PE.
        (0x1e80, 0x3a, 0x1e82, true),  // Denormal suppresses post-computation flags.
        (0x1f80, 0x21, 0x1fa1, false), // Masked invalid permits a sibling's PE.
        (0x0f81, 0, 0x0f81, false),    // Old flags cannot raise a new exception.
    ] {
        // With fallback enabled, operand and range conditions restart in the
        // interpreter. Exact and inexact results must complete in the compiled path.
        if fallback_enabled && raised & 0x1f != 0 {
            continue;
        }
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
            arguments: vec![Argument::I32(raised)],
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
            "MXCSR {mxcsr:#x}, raised {raised:#x}"
        );
    }
}

#[test]
fn simd_status_precedes_faults_and_destination_updates() {
    exceptions(Engine::Wasmtime, false);
    exceptions(Engine::Wasmtime, true);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_simd_status_precedes_faults_and_destination_updates() {
    exceptions(Engine::V8, false);
    exceptions(Engine::V8, true);
}
