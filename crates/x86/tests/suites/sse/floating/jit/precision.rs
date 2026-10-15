//! An observed masked precision flag removes repeated exactness work without hiding new faults.

use super::*;

fn sticky_precision(engine: Engine) {
    let instruction = [0xf3, 0x0f, 0x58, 0xc1]; // ADDSS: 1 + 2^-24 rounds back to 1.
    let code = [&[0x90][..], &instruction].concat();
    for observed in [0x1fa0, 0x0fa0] {
        let mut image = floating_image(&code, observed);
        lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
        lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3380_0000);
        let compiled = observed_block(&code, 2, SegmentProfile::Flat32.into(), &image.cpu);
        let linked = observed_block(&code, 2, SegmentProfile::Flat32.into(), &image.cpu)
            .with_interpreter(TestModule::interpreter());
        for live in [0x1fa0, 0x1fa5, 0xabcd_1faf, 0x1f80, 0x0fa0, 0x0f80] {
            image.cpu.simd.mxcsr = live;
            let prefix = retire(image.cpu, 1);
            let fault = live & 0x1000 == 0;
            let mut cpu = if fault {
                prefix
            } else {
                retire(prefix, instruction.len())
            };
            cpu.simd.mxcsr |= 0x20;
            let completed = Step {
                cpu,
                ram: &[],
                exit: if fault {
                    Exit::SimdFloatingPoint
                } else {
                    Exit::Dispatch(cpu.eip)
                },
            };
            let wanted = expected(&image, std::slice::from_ref(&completed));
            let mismatch =
                (live ^ observed) & 0xffc0 != 0 || (observed & 0x1000 != 0 && live & 0x20 == 0);
            let direct = if mismatch {
                Step {
                    cpu: prefix,
                    ram: &[],
                    exit: Exit::Interpret,
                }
            } else {
                completed
            };
            assert_eq!(
                engine.observe(&compiled, &image.input(), 1),
                expected(&image, &[direct]),
                "observed {observed:x}, live {live:x}"
            );
            assert_eq!(engine.observe(&linked, &image.input(), 1), wanted);
        }
    }
}

fn loaded_precision(engine: Engine) {
    let add = [0xf3, 0x0f, 0x58, 0xc1];
    let load = [0x0f, 0xae, 0x10]; // LDMXCSR [EAX]
    let code = [add.as_slice(), &load, &add].concat();
    let mut image = floating_image(&code, 0x1fa0);
    image.cpu.registers.eax = 0x4000;
    image.map(4, 0x8000, false);
    lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
    lane(&mut image.cpu.simd.xmm[1], false, 0, 0x3380_0000);
    let compiled = observed_block(&code, 3, SegmentProfile::Flat32.into(), &image.cpu);
    let linked = observed_block(&code, 3, SegmentProfile::Flat32.into(), &image.cpu)
        .with_interpreter(TestModule::interpreter());
    for loaded in [0x1f80_u32, 0x1fa5, 0x0fa0] {
        image.data(0x8000, &loaded.to_le_bytes());
        let mut prefix = retire(retire(image.cpu, add.len()), load.len());
        prefix.simd.mxcsr = loaded;
        let fault = loaded == 0x0fa0;
        let mut cpu = if fault {
            prefix
        } else {
            retire(prefix, add.len())
        };
        cpu.simd.mxcsr |= 0x20;
        let completed = Step {
            cpu,
            ram: &[],
            exit: if fault {
                Exit::SimdFloatingPoint
            } else {
                Exit::Dispatch(cpu.eip)
            },
        };
        let wanted = expected(&image, std::slice::from_ref(&completed));
        let direct = if loaded == 0x1fa5 {
            completed
        } else {
            Step {
                cpu: prefix,
                ram: &[],
                exit: Exit::Interpret,
            }
        };
        assert_eq!(
            engine.observe(&compiled, &image.input(), 1),
            expected(&image, &[direct])
        );
        assert_eq!(engine.observe(&linked, &image.input(), 1), wanted);
    }
}

fn precision_free_instructions(engine: Engine) {
    let compare = [0x0f, 0x2f, 0xc1]; // COMISS XMM0,XMM1
    let instructions: &[&[u8]] = &[
        &[0x0f, 0x2e, 0xc1],          // UCOMISS XMM0,XMM1
        &[0x0f, 0xc2, 0xd1, 0],       // CMPPS XMM2,XMM1,EQ
        &[0xf3, 0x0f, 0xc2, 0xf9, 0], // CMPSS XMM7,XMM1,EQ
        &[0x0f, 0x5d, 0xd9],          // MINPS XMM3,XMM1
        &[0xf3, 0x0f, 0x5f, 0xe1],    // MAXSS XMM4,XMM1
        &[0x0f, 0x5a, 0xe9],          // CVTPS2PD XMM5,XMM1
        &[0xf3, 0x0f, 0x5a, 0xf1],    // CVTSS2SD XMM6,XMM1
        &[0xf3, 0x0f, 0xe6, 0xc1],    // CVTDQ2PD XMM0,XMM1: exact signed-dword conversion
    ];
    for load in [false, true] {
        let mut code = compare.to_vec();
        if load {
            code.extend([0x0f, 0xae, 0x10]); // LDMXCSR [EAX]
        }
        code.extend(instructions.concat());
        let count = 1 + instructions.len() as u32 + u32::from(load);
        let mut image = floating_image(&code, 0x1fa0);
        image.cpu.registers.eax = 0x4000;
        image.map(4, 0x8000, false);
        image.data(0x8000, &0x1f85_u32.to_le_bytes());
        for (register, vector) in image.cpu.simd.xmm.iter_mut().enumerate() {
            for index in 0..4 {
                lane(
                    vector,
                    false,
                    index,
                    if register == 1 { TWO[0] } else { ONE[0] },
                );
            }
        }
        let compiled = observed_block(&code, count, SegmentProfile::Flat32.into(), &image.cpu);
        if !load {
            image.cpu.simd.mxcsr = 0x1f85;
        }
        let mut cpu = image.cpu;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = cpu.instruction_count.wrapping_add(count);
        cpu.simd.mxcsr = 0x1f85;
        comparison_flags(&mut cpu, 1);
        for index in 0..2 {
            // The source contains signed integers 0x40000000, exactly 2^30.
            lane(&mut cpu.simd.xmm[0], true, index, 0x41d0_0000_0000_0000);
        }
        cpu.simd.xmm[2] = [0; 16];
        lane(&mut cpu.simd.xmm[7], false, 0, 0);
        lane(&mut cpu.simd.xmm[4], false, 0, TWO[0]);
        for index in 0..2 {
            lane(&mut cpu.simd.xmm[5], true, index, TWO[1]);
        }
        lane(&mut cpu.simd.xmm[6], true, 0, TWO[1]);
        assert_eq!(
            engine.observe(&compiled, &image.input(), 1),
            expected(&image, &[dispatch(cpu)]),
            "precision-free operations stay compiled after clearing PE; LDMXCSR={load}"
        );
    }
}

fn precision_producing_instructions(engine: Engine) {
    for (name, code) in [
        ("ADDPS", &[0x0f, 0x58, 0xc1][..]),
        ("SQRTSS", &[0xf3, 0x0f, 0x51, 0xc1][..]),
        ("SQRTPS", &[0x0f, 0x51, 0xc1][..]),
        ("CVTTSS2SI", &[0xf3, 0x0f, 0x2c, 0xc1][..]),
        ("CVTPS2DQ", &[0x66, 0x0f, 0x5b, 0xc1][..]),
        ("CVTSD2SS", &[0xf2, 0x0f, 0x5a, 0xc1][..]),
        ("CVTPD2PS", &[0x66, 0x0f, 0x5a, 0xc1][..]),
        ("CVTSI2SS", &[0xf3, 0x0f, 0x2a, 0xc1][..]),
        ("CVTDQ2PS", &[0x0f, 0x5b, 0xc1][..]),
        ("CVTPI2PS", &[0x0f, 0x2a, 0xc1][..]),
        ("CVTTPS2PI", &[0x0f, 0x2c, 0xc1][..]),
    ] {
        let mut image = floating_image(code, 0x1fa0);
        image.cpu.simd.xmm.fill([0; 16]);
        image.cpu.registers.ecx = 0;
        image.cpu.x87.status = crate::support::x87::status(0);
        let compiled = observed_block(code, 1, SegmentProfile::Flat32.into(), &image.cpu);
        image.cpu.simd.mxcsr = 0x1f80;
        image.check_unchanged_exit(engine, &compiled, name, Exit::Interpret);
    }
}

fn cleared_precision_before_mmio(engine: Engine) {
    use crate::support::step::Event;

    let code = [0xf3, 0x0f, 0x58, 0x06, 0, 0x20]; // ADDSS XMM0,[2000] in real mode.
    let mut image = floating_image(&code, 0x1fa0);
    image.cpu.segments = Segments::real_mode();
    lane(&mut image.cpu.simd.xmm[0], false, 0, ONE[0]);
    image.data(0x8000, &0x3380_0000_u32.to_le_bytes());
    let compiled = observed_block(&code, 1, ExecutionProfile::Real16, &image.cpu);
    let linked = observed_block(&code, 1, ExecutionProfile::Real16, &image.cpu).with_interpreter(
        TestModule::interpreter_with_profile(ExecutionProfile::Real16),
    );
    image.cpu.simd.mxcsr = 0x1f80;
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
        )
    );
    let mut cpu = retire(image.cpu, code.len());
    cpu.simd.mxcsr |= 0x20;
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

#[test]
fn cleared_precision_keeps_precision_free_instructions_compiled() {
    precision_free_instructions(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_cleared_precision_keeps_precision_free_instructions_compiled() {
    precision_free_instructions(Engine::V8);
}

#[test]
fn observed_mxcsr_precision_specialization() {
    sticky_precision(Engine::Wasmtime);
    loaded_precision(Engine::Wasmtime);
    precision_producing_instructions(Engine::Wasmtime);
    cleared_precision_before_mmio(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_observed_mxcsr_precision_specialization() {
    sticky_precision(Engine::V8);
    loaded_precision(Engine::V8);
    precision_producing_instructions(Engine::V8);
    cleared_precision_before_mmio(Engine::V8);
}
