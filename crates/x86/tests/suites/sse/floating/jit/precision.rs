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
fn observed_mxcsr_precision_specialization() {
    sticky_precision(Engine::Wasmtime);
    loaded_precision(Engine::Wasmtime);
    cleared_precision_before_mmio(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_observed_mxcsr_precision_specialization() {
    sticky_precision(Engine::V8);
    loaded_precision(Engine::V8);
    cleared_precision_before_mmio(Engine::V8);
}
