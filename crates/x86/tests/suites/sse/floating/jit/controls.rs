//! Observed MXCSR controls are guarded; live status and later loads stay dynamic.

use super::*;

fn block(code: &[u8], count: u32, profile: ExecutionProfile, observed: &CpuState) -> TestModule {
    TestModule::new(
        &Compiler::new(profile)
            .specialize_on_cpu(observed)
            .compile_block(0x1000, code, count)
            .unwrap(),
    )
}

fn matching_controls(engine: Engine) {
    // Two inexact ADDSS instructions exercise both initial status variation and
    // the new PE set by the first instruction before the second control check.
    let code = [0xf3, 0x0f, 0x58, 0xc1].repeat(2);
    for rounding in 0..4 {
        let mut image = floating_image(&code, 0x1f80 | rounding << 13);
        lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
        lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3380_0000); // 2^-24, half an ulp at 1.
        let compiled = block(&code, 2, SegmentProfile::Flat32.into(), &image.cpu);
        for old_flags in [0, 1, 0x20, 0x3f, 0xabcd_0025] {
            image.cpu.simd.mxcsr = (0x1f80 | rounding << 13) | old_flags;
            let mut cpu = retire(retire(image.cpu, 4), 4);
            cpu.simd.mxcsr |= 0x20;
            lane(
                &mut cpu.simd.xmm[0],
                false,
                0,
                if rounding == 2 { 0x3f80_0002 } else { ONE[0] },
            );
            assert_eq!(
                engine.observe(&compiled, &image.input(), 1),
                expected(&image, &[dispatch(cpu)]),
                "RC {rounding}, live status/reserved bits {old_flags:x}"
            );
        }
    }

    // DAZ changes the numerical input without raising a denormal exception.
    let code = [0xf3, 0x0f, 0x58, 0xc1];
    let mut image = floating_image(&code, 0x9fc0);
    lane(&mut image.cpu.simd.xmm[0], false, 0, 0);
    lane(&mut image.cpu.simd.xmm[1], false, 0, 1);
    let compiled = block(&code, 1, SegmentProfile::Flat32.into(), &image.cpu);
    assert_eq!(
        engine.observe(&compiled, &image.input(), 1),
        expected(&image, &[dispatch(retire(image.cpu, code.len()))])
    );

    // Precision remains a compiled fault even when its mask is specialized off.
    let code = [0xf3, 0x0f, 0x2d, 0xc1]; // CVTSS2SI EAX,XMM1
    let mut image = floating_image(&code, 0x0f80);
    lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3fc0_0000); // 1.5
    let compiled = block(&code, 1, SegmentProfile::Flat32.into(), &image.cpu);
    let mut cpu = image.cpu;
    cpu.simd.mxcsr |= 0x20;
    assert_eq!(
        engine.observe(&compiled, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::SimdFloatingPoint
            }]
        )
    );
}

fn changed_controls(engine: Engine) {
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        let compare = if profile == ExecutionProfile::Real16 {
            &[0x66, 0x39, 0xc8][..]
        } else {
            &[0x39, 0xc8][..]
        };
        let add = [0xf3, 0x0f, 0x58, 0xc1];
        let code = [compare, &add].concat();
        let mut image = floating_image(&code, 0x1f80);
        if profile == ExecutionProfile::Real16 {
            image.cpu.segments = Segments::real_mode();
        }
        image.cpu.registers.eax = 0;
        image.cpu.registers.ecx = 1;
        lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
        lane(&mut image.cpu.simd.xmm[1], false, 0, TWO[0]);
        let compiled = block(&code, 2, profile, &image.cpu);
        let linked = block(&code, 2, profile, &image.cpu)
            .with_interpreter(TestModule::interpreter_with_profile(profile));
        for changed_bit in 6..16 {
            image.cpu.simd.mxcsr = (0x1f80 ^ (1 << changed_bit)) | 0x25;
            let mut prefix = retire(image.cpu, compare.len());
            prefix.flags.status_source.kind = 9;
            prefix.flags.status_source.left = 0;
            prefix.flags.status_source.right = 1;
            assert_eq!(
                engine.observe(&compiled, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: prefix,
                        ram: &[],
                        exit: Exit::Interpret
                    }]
                ),
                "changed control bit {changed_bit}, {profile:?}"
            );
            let mut cpu = retire(prefix, add.len());
            lane(&mut cpu.simd.xmm[0], false, 0, 0x4040_0000); // 3.0, exact under all controls.
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(cpu)])
            );
        }
    }
}

fn loaded_controls(engine: Engine) {
    // Establish observed facts and PE, load a runtime value that replaces both,
    // then convert a value whose result distinguishes nearest from round-up.
    let add = [0xf3, 0x0f, 0x58, 0xc1];
    let load = [0x0f, 0xae, 0x10]; // LDMXCSR [EAX]
    let convert = [0xf3, 0x0f, 0x2d, 0xc2]; // CVTSS2SI EAX,XMM2
    let code = [add.as_slice(), &load, &convert].concat();
    let mut image = floating_image(&code, 0x1f80);
    image.cpu.registers.eax = 0x4000;
    image.map(4, 0x8000, false);
    lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
    lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3380_0000);
    lane(&mut image.cpu.simd.xmm[2], false, 0, 0x3fa0_0000); // 1.25
    let compiled = block(&code, 3, SegmentProfile::Flat32.into(), &image.cpu);
    let linked = block(&code, 3, SegmentProfile::Flat32.into(), &image.cpu)
        .with_interpreter(TestModule::interpreter());
    for loaded in [0x1f80_u32, 0x1fa1, 0x5f80] {
        image.data(0x8000, &loaded.to_le_bytes());
        let mut prefix = retire(retire(image.cpu, add.len()), load.len());
        prefix.simd.mxcsr = loaded;
        let mut cpu = retire(prefix, convert.len());
        cpu.registers.eax = if loaded == 0x5f80 { 2 } else { 1 };
        cpu.simd.mxcsr |= 0x20;
        let direct = if loaded == 0x5f80 {
            Step {
                cpu: prefix,
                ram: &[],
                exit: Exit::Interpret,
            }
        } else {
            dispatch(cpu)
        };
        assert_eq!(
            engine.observe(&compiled, &image.input(), 1),
            expected(&image, &[direct])
        );
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(cpu)])
        );
    }
}

fn mismatch_before_mmio(engine: Engine) {
    use crate::support::step::Event;

    let code = [0xf3, 0x0f, 0x58, 0x06, 0, 0x20]; // ADDSS XMM0,[2000] in real mode.
    let mut image = floating_image(&code, 0x1f80);
    image.cpu.segments = Segments::real_mode();
    lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
    image.data(0x8000, &0x4000_0000_u32.to_le_bytes()); // 2.0
    let compiled = block(&code, 1, ExecutionProfile::Real16, &image.cpu);
    let linked = block(&code, 1, ExecutionProfile::Real16, &image.cpu).with_interpreter(
        TestModule::interpreter_with_profile(ExecutionProfile::Real16),
    );
    image.cpu.simd.mxcsr = 0x5f80;
    let mut input = image.input();
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    assert_eq!(
        engine.observe(&compiled, &input, 1),
        expected(
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Interpret,
            }]
        ),
        "a control mismatch must restart before reading device memory"
    );

    let mut cpu = retire(image.cpu, code.len());
    lane(&mut cpu.simd.xmm[0], false, 0, 0x4040_0000); // 3.0
    let mut wanted = expected(&image, &[dispatch(cpu)]);
    wanted.events.insert(
        0,
        Event::MmioRead {
            address: 0x2000,
            bytes: 4,
        },
    );
    assert_eq!(engine.observe(&linked, &input, 1), wanted);
}

fn flush_to_zero(engine: Engine) {
    let code = [0xf3, 0x0f, 0x59, 0xc1]; // MULSS: minimum normal * 0.5
    for (controls, result, flags) in [(0x1f80, 0x0040_0000, 0), (0x9f80, 0, 0x30)] {
        let mut image = floating_image(&code, controls);
        lane(&mut image.cpu.simd.xmm[0], false, 0, 0x0080_0000);
        lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3f00_0000);
        let compiled = block(&code, 1, SegmentProfile::Flat32.into(), &image.cpu);
        let linked = block(&code, 1, SegmentProfile::Flat32.into(), &image.cpu)
            .with_interpreter(TestModule::interpreter());
        let mut cpu = retire(image.cpu, code.len());
        lane(&mut cpu.simd.xmm[0], false, 0, result);
        cpu.simd.mxcsr |= flags;
        let direct = if flags == 0 {
            dispatch(cpu)
        } else {
            // The existing underflow restart still owns the exceptional case.
            Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Interpret,
            }
        };
        assert_eq!(
            engine.observe(&compiled, &image.input(), 1),
            expected(&image, &[direct])
        );
        assert_eq!(
            engine.observe(&linked, &image.input(), 1),
            expected(&image, &[dispatch(cpu)])
        );
    }
}

fn mmx_admission(engine: Engine) {
    let code = [0x0f, 0x2d, 0xc1]; // CVTPS2PI MM0,XMM1
    let mut image = floating_image(&code, 0x1f80);
    image.cpu.x87.status = crate::support::x87::status(0);
    let compiled = block(&code, 1, SegmentProfile::Flat32.into(), &image.cpu);
    image.cpu.simd.mxcsr = 0x5f80;
    image.check_unchanged_exit(
        engine,
        &compiled,
        "control mismatch precedes MMX destination and environment changes",
        Exit::Interpret,
    );
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    image.check_unchanged_exit(
        engine,
        &compiled,
        "pending x87 exception precedes the new MXCSR guard",
        Exit::FloatingPoint,
    );
}

#[test]
fn observed_mxcsr_controls_preserve_status_and_restart() {
    matching_controls(Engine::Wasmtime);
    changed_controls(Engine::Wasmtime);
    loaded_controls(Engine::Wasmtime);
    mismatch_before_mmio(Engine::Wasmtime);
    flush_to_zero(Engine::Wasmtime);
    mmx_admission(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_observed_mxcsr_controls_preserve_status_and_restart() {
    matching_controls(Engine::V8);
    changed_controls(Engine::V8);
    loaded_controls(Engine::V8);
    mismatch_before_mmio(Engine::V8);
    flush_to_zero(Engine::V8);
    mmx_admission(Engine::V8);
}
