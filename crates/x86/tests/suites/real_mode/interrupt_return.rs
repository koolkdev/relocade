use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{Event, TestModule},
};
use wasm86_x86::FlagBytes;

fn restored_flags(cpu: &mut CpuState, bit: u8, word: bool) {
    cpu.flags.status_source.kind = 0;
    cpu.flags.bytes = FlagBytes {
        cf: bit,
        pf: bit,
        af: bit,
        zf: bit,
        sf: bit,
        of: bit,
        tf: bit,
        df: bit,
        nt: bit,
        if_: bit,
        iopl: 3 * bit,
        ..cpu.flags.bytes
    };
    if !word {
        cpu.flags.bytes.ac = bit;
        cpu.flags.bytes.id = bit;
    }
}

fn returns(engine: Engine, frontend: Frontend) {
    let mut cases = sequences(engine, frontend);
    for (code, width) in [(&[0x64, 0x67, 0xcf][..], 2), (&[0x66, 0xcf][..], 4)] {
        for (offset, selector, flags, bit) in
            [(0_u32, 0_u16, 0_u32, 0), (0xffff, 0xffff, u32::MAX, 1)]
        {
            let mut image = image(code);
            let mut frame = offset.to_le_bytes()[..width].to_vec();
            frame.extend(selector.to_le_bytes());
            if width == 4 {
                frame.extend([0xa5, 0x5a]);
            }
            frame.extend(&flags.to_le_bytes()[..width]);
            image.cpu.segments.ss = cache(Segment::Ss, 0x2000);
            image.cpu.registers.esp = 0xabcd_0000 | (0x10000 - frame.len() as u32);
            image.cpu.flags.status_source.kind = 9;
            image.cpu.flags.status_source.left = 7;
            image.cpu.flags.status_source.right = 8;
            image.cpu.flags.bytes.nt = 0xff ^ bit;
            image.map(0x2f, 0x8000, false);
            image.data(0x9000 - frame.len() as u32, &frame);
            let mut cpu = retired(&image, code.len());
            cpu.eip = offset;
            cpu.registers.esp = 0xabcd_0000;
            cpu.segments.cs = cache(Segment::Cs, selector);
            restored_flags(&mut cpu, bit, width == 2);
            cases.check(
                "IRET ignores entry NT, restores a real CS and wraps SP after a complete frame",
                code,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(offset),
                }],
            );
        }
    }
}
test_frontends!(complete_return_frames, returns);

fn stack_bounds(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (code, sp) in [(&[0xcf][..], 0xfffb), (&[0x66, 0xcf][..], 0xfff5)] {
        let mut image = image(code);
        image.cpu.registers.esp = 0xabcd_0000 | sp;
        let mut input = image.input();
        input.mmio_pages = vec![(0xf, 0x8000)];
        input.observe_mmio = true;
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(
            engine.observe(module, &input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::StackFault { error: 0 },
                }]
            ),
            "the complete IRET frame must fit SS before any MMIO read"
        );
    }
}
test_frontends!(complete_stack_capacity, stack_bounds);

fn mmio_fields(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    let code = [0x66, 0x67, 0xcf];
    for offset in [0xffff_u32, 0x10000, u32::MAX] {
        let mut image = image(&code);
        image.cpu.segments.ss = cache(Segment::Ss, 0x1000);
        image.cpu.registers.esp = 0xabcd_2000;
        image.data(0x8000, &offset.to_le_bytes());
        image.data(0x8004, &[0x20, 0, 0xa5, 0x5a, 0, 0x22, 0xff, 0xff]);
        let mut input = image.input();
        input.mmio_pages = vec![(0x12, 0x8000)];
        input.observe_mmio = true;
        let (cpu, exit, reads) = if offset == 0xffff {
            let mut cpu = retired(&image, code.len());
            cpu.eip = offset;
            cpu.segments.cs = cache(Segment::Cs, 0x20);
            cpu.registers.esp = 0xabcd_200c;
            restored_flags(&mut cpu, 0, false);
            cpu.flags.bytes.if_ = 1;
            cpu.flags.bytes.iopl = 2;
            cpu.flags.bytes.ac = 1;
            cpu.flags.bytes.id = 1;
            (cpu, Exit::Dispatch(offset), 3)
        } else {
            (image.cpu, Exit::GeneralProtection { error: 0 }, 1)
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit,
            }],
        );
        wanted.events.splice(
            0..0,
            [
                Event::MmioRead {
                    address: 0x12000,
                    bytes: 4,
                },
                Event::MmioRead {
                    address: 0x12004,
                    bytes: 2,
                },
                Event::MmioRead {
                    address: 0x12008,
                    bytes: 4,
                },
            ]
            .into_iter()
            .take(reads),
        );
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        assert_eq!(
            engine.observe(module, &input, 1),
            wanted,
            "IRETD validates EIP before CS/FLAGS and transfers only the selector's low word"
        );
    }
}
test_frontends!(mmio_order_and_widths, mmio_fields);

fn next_entry(engine: Engine) {
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    for target_mapped in [false, true] {
        let mut image = image(&[0xcf]);
        image.cpu.registers.esp = 0xabcd_2000;
        image.map(2, 0x8000, true);
        image.data(0x8000, &[0x20, 0, 0, 0x10, 0, 0x22]);
        if target_mapped {
            image.map(0x10, 0x9000, false);
            image.data(0x9020, &[0x66, 0x9c]); // PUSHFD in the new CS.
        }
        let mut returned = retired(&image, 1);
        returned.eip = 0x20;
        returned.segments.cs = cache(Segment::Cs, 0x1000);
        returned.registers.esp = 0xabcd_2006;
        restored_flags(&mut returned, 0, true);
        returned.flags.bytes.if_ = 1;
        returned.flags.bytes.iopl = 2;
        let mut next = returned;
        let (ram, exit) = if target_mapped {
            next.eip += 2;
            next.instruction_count += 1;
            next.registers.esp = 0xabcd_2002;
            (
                &[(0x8002, &[2, 0x22, 0x24, 0][..])][..],
                Exit::Dispatch(next.eip),
            )
        } else {
            (&[][..], Exit::Other(0x0008_00ff_0000_0020))
        };
        assert_eq!(
            engine.observe(module, &image.input(), 2),
            expected(
                &image,
                &[
                    Step {
                        cpu: returned,
                        ram: &[],
                        exit: Exit::Dispatch(returned.eip)
                    },
                    Step {
                        cpu: next,
                        ram,
                        exit
                    },
                ]
            ),
            "target fetch follows the committed return and sees its CS/FLAGS"
        );
    }
}

#[test]
fn target_fetch_uses_the_committed_return() {
    next_entry(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_target_fetch_uses_the_committed_return() {
    next_entry(Engine::V8);
}
