//! Common inputs stay compiled; exceptional operands restart before publication.

#[path = "jit/controls.rs"]
mod controls;

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{compile_block_from_bytes, Compiler, ExecutionProfile, SegmentProfile, Segments};

fn common_inputs(engine: Engine) {
    let code = [0x0f, 0x2e, 0xc1]; // UCOMISS XMM0,XMM1
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    for (left, right, mxcsr, flags) in [
        (ONE, TWO, 0x1e00, 1),
        (QNAN, ONE, 0x1e00, 0x45),
        (DENORMAL, ZERO, 0x1e40, 0x40),
        (ONE, ONE, 0x1e03, 0x40), // Existing sticky flags do not raise a new exception.
    ] {
        let mut image = floating_image(&code, mxcsr);
        lane(&mut image.cpu.simd.xmm[0], false, 0, left[0]);
        lane(&mut image.cpu.simd.xmm[1], false, 0, right[0]);
        let mut cpu = retire(image.cpu, code.len());
        comparison_flags(&mut cpu, flags);
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(&image, &[dispatch(cpu)])
        );
    }

    let code = [0x0f, 0xc2, 0xc1, 0]; // CMPPS EQ, with a quiet unordered lane.
    let mut image = floating_image(&code, 0x1e00);
    for index in 0..4 {
        lane(&mut image.cpu.simd.xmm[0], false, index, ONE[0]);
        lane(&mut image.cpu.simd.xmm[1], false, index, ONE[0]);
    }
    lane(&mut image.cpu.simd.xmm[0], false, 0, QNAN[0]);
    lane(&mut image.cpu.simd.xmm[1], false, 0, DENORMAL[0]);
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.xmm[0] = [0xff; 16];
    lane(&mut cpu.simd.xmm[0], false, 0, 0);
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(&image, &[dispatch(cpu)])
    );

    let code = [0xf2, 0x0f, 0x5d, 0xc1]; // MINSD keeps the source's zero sign.
    let mut image = floating_image(&code, 0x1e00);
    lane(&mut image.cpu.simd.xmm[0], true, 0, ZERO[1]);
    lane(&mut image.cpu.simd.xmm[1], true, 0, NEG_ZERO[1]);
    let mut cpu = retire(image.cpu, code.len());
    lane(&mut cpu.simd.xmm[0], true, 0, NEG_ZERO[1]);
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    assert_eq!(
        engine.observe(&block, &image.input(), 1),
        expected(&image, &[dispatch(cpu)])
    );
}

fn precise_restart(engine: Engine, profile: ExecutionProfile) {
    for instruction in [
        &[0x0f, 0x2f, 0xc1][..],       // COMISS
        &[0x0f, 0xc2, 0xc1, 0][..],    // CMPPS EQ
        &[0xf2, 0x0f, 0x5d, 0xc1][..], // MINSD
    ] {
        let packed = instruction[1] == 0xc2;
        let wide = instruction[0] == 0xf2;
        let raised = if packed { 3 } else { 1 };
        // CMP EAX,ECX first, with a size override for real-mode defaults.
        let compare = match profile {
            ExecutionProfile::Real16 => &[0x66, 0x39, 0xc8][..],
            ExecutionProfile::Protected(_) => &[0x39, 0xc8][..],
        };
        let code = [compare, instruction].concat();
        let compiled = Compiler::new(profile)
            .compile_block(0x1000, &code, 2)
            .unwrap();
        let block = TestModule::new(&compiled);
        let linked = TestModule::new(&compiled)
            .with_interpreter(TestModule::interpreter_with_profile(profile));
        for masked in [false, true] {
            let mut image = floating_image(&code, if masked { 0x1f80 } else { 0x1e00 });
            if profile == ExecutionProfile::Real16 {
                image.cpu.segments = Segments::real_mode();
            }
            image.cpu.registers.eax = 0;
            image.cpu.registers.ecx = 1;
            for index in 0..if wide { 2 } else { 4 } {
                lane(&mut image.cpu.simd.xmm[0], wide, index, ONE[wide as usize]);
                lane(&mut image.cpu.simd.xmm[1], wide, index, ONE[wide as usize]);
            }
            lane(&mut image.cpu.simd.xmm[1], wide, 0, SNAN[wide as usize]);
            if packed {
                lane(&mut image.cpu.simd.xmm[0], false, 1, DENORMAL[0]);
                lane(&mut image.cpu.simd.xmm[1], false, 1, ZERO[0]);
            }
            let mut prefix = retire(image.cpu, compare.len());
            prefix.flags.status_source.kind = 9;
            prefix.flags.status_source.left = 0;
            prefix.flags.status_source.right = 1;
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: prefix,
                        ram: &[],
                        exit: Exit::Interpret
                    }]
                ),
                "restart preserves the completed CMP and leaves MXCSR/XMM unchanged"
            );

            let mut cpu = if masked {
                retire(prefix, instruction.len())
            } else {
                prefix
            };
            cpu.simd.mxcsr |= raised;
            if masked {
                if wide {
                    lane(&mut cpu.simd.xmm[0], true, 0, SNAN[1]);
                } else if packed {
                    cpu.simd.xmm[0] = [0xff; 16];
                    lane(&mut cpu.simd.xmm[0], false, 0, 0);
                    lane(&mut cpu.simd.xmm[0], false, 1, 0);
                } else {
                    comparison_flags(&mut cpu, 0x45);
                }
            }
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: if masked {
                            Exit::Dispatch(cpu.eip)
                        } else {
                            Exit::SimdFloatingPoint
                        },
                    }]
                ),
                "interpreter completes the exception response, including all packed lanes"
            );
        }
    }
}

fn real_mode_ram(engine: Engine) {
    let code = [0x0f, 0x2f, 0x06, 0, 0x20]; // COMISS XMM0,[2000]
    let compiled = Compiler::new(ExecutionProfile::Real16)
        .compile_block(0x1000, &code, 1)
        .unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter_with_profile(
        ExecutionProfile::Real16,
    ));
    for (operand, mxcsr, flags, invalid, fault) in [
        (TWO[0], 0x1f80, 1, false, false),
        (SNAN[0], 0x1f80, 0x45, true, false),
        (SNAN[0], 0x1f00, 0, true, true),
    ] {
        let mut image = floating_image(&code, mxcsr);
        image.cpu.segments = Segments::real_mode();
        lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
        image.data(0x2000, &(operand as u32).to_le_bytes());
        image.map(2, 0x2000, false);
        let mut cpu = if fault {
            image.cpu
        } else {
            retire(image.cpu, code.len())
        };
        cpu.simd.mxcsr |= u32::from(invalid);
        if !fault {
            comparison_flags(&mut cpu, flags);
        }
        let completed = Step {
            cpu,
            ram: &[],
            exit: if fault {
                Exit::SimdFloatingPoint
            } else {
                Exit::Dispatch(cpu.eip)
            },
        };
        let wanted = expected(&image, &[completed]);
        if invalid {
            let restart = Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Interpret,
            };
            assert_eq!(
                engine.observe(&block, &image.input(), 1),
                expected(&image, &[restart])
            );
        } else {
            assert_eq!(engine.observe(&block, &image.input(), 1), wanted);
        }
        assert_eq!(engine.observe(&linked, &image.input(), 1), wanted);
    }
}

#[test]
fn sse_jit_specialization() {
    common_inputs(Engine::Wasmtime);
    precise_restart(Engine::Wasmtime, SegmentProfile::Flat32.into());
    precise_restart(Engine::Wasmtime, ExecutionProfile::Real16);
    real_mode_ram(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_sse_jit_specialization() {
    common_inputs(Engine::V8);
    precise_restart(Engine::V8, SegmentProfile::Flat32.into());
    precise_restart(Engine::V8, ExecutionProfile::Real16);
    real_mode_ram(Engine::V8);
}
