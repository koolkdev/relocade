//! Arithmetic specialization restarts before effects in both execution modes.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{Compiler, ExecutionProfile, Segments};

fn compiled(engine: Engine) {
    for profile in [ExecutionProfile::Real16, SegmentProfile::Flat32.into()] {
        let instruction = [0xf2, 0x0f, 0x58, 0xc1]; // ADDSD
        let code = [&[0x90][..], &instruction].concat();
        let compiled = Compiler::new(profile)
            .compile_block(0x1000, &code, 2)
            .unwrap();
        let block = TestModule::new(&compiled);
        let linked = TestModule::new(&compiled)
            .with_interpreter(TestModule::interpreter_with_profile(profile));
        for (right, controls, raised, result, restart) in [
            (HALF_ULP[1], 0x1f80, 32, ONE[1], false),
            (HALF_ULP[1], 0x0f80, 32, ONE[1], false),
            (SNAN[1], 0x1f80, 1, QUIETED[1], true),
            (SNAN[1], 0x1f00, 1, QUIETED[1], true),
        ] {
            let mut image = floating_image(&code, controls);
            if profile == ExecutionProfile::Real16 {
                image.cpu.segments = Segments::real_mode();
            }
            lane(&mut image.cpu.simd.xmm[0], true, 0, ONE[1]);
            lane(&mut image.cpu.simd.xmm[1], true, 0, right);
            let prior = retire(image.cpu, 1);
            let fault = raised & !(controls >> 7) != 0;
            let mut cpu = if fault {
                prior
            } else {
                retire(prior, instruction.len())
            };
            cpu.simd.mxcsr |= raised;
            if !fault {
                lane(&mut cpu.simd.xmm[0], true, 0, result);
            }
            let wanted = expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: if fault {
                        Exit::SimdFloatingPoint
                    } else {
                        Exit::Dispatch(cpu.eip)
                    },
                }],
            );
            if restart {
                assert_eq!(
                    engine.observe(&block, &image.input(), 1),
                    expected(
                        &image,
                        &[Step {
                            cpu: prior,
                            ram: &[],
                            exit: Exit::Interpret
                        }]
                    )
                );
            } else {
                assert_eq!(engine.observe(&block, &image.input(), 1), wanted);
            }
            assert_eq!(engine.observe(&linked, &image.input(), 1), wanted);
        }
    }
}

#[test]
fn sse_arithmetic_compiled_and_restart() {
    compiled(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_sse_arithmetic_compiled_and_restart() {
    compiled(Engine::V8);
}
