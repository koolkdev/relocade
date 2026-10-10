//! EFLAGS comparisons retain x87 condition codes and share exception responses.

#[path = "x87_compare_flags/jit.rs"]
mod jit;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{complete_x87, dispatch, set_control, stack_image, write_value},
};
use wasm86_x86::{CpuState, SegmentProfile};

const LEADING: u64 = 1 << 63;
const ONE: (u64, u16) = (LEADING, 0x3fff);
const QNAN: (u64, u16) = (0xc000_0000_0000_0042, 0x7fff);
const SNAN: (u64, u16) = (LEADING + 1, 0xffff);

fn initial_image(code: &[u8]) -> Image {
    let mut image = stack_image(code, 7, 0);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.cf = 1;
    image.cpu.flags.bytes.pf = 0;
    image.cpu.flags.bytes.zf = 0;
    image.cpu.flags.bytes.of = 1;
    image.cpu.flags.bytes.sf = 1;
    image.cpu.flags.bytes.af = 1;
    // Unchanged condition codes retain their backing bytes, including unused bits.
    image.cpu.x87.status.c0 = 0xa5;
    image.cpu.x87.status.c2 = 0xa4;
    image.cpu.x87.status.c3 = 0xa7;
    write_value(&mut image.cpu, 7, ONE);
    write_value(&mut image.cpu, 0, ONE);
    image
}

fn completed(mut cpu: CpuState, opcode: u16, flags: u8, exceptions: u16, pops: u8) -> CpuState {
    cpu = complete_x87(cpu, 2, opcode);
    cpu.flags.status_source.kind = 0;
    cpu.flags.bytes.cf = flags & 1;
    cpu.flags.bytes.pf = (flags >> 2) & 1;
    cpu.flags.bytes.zf = (flags >> 6) & 1;
    cpu.flags.bytes.of = 0;
    cpu.flags.bytes.sf = 0;
    cpu.flags.bytes.af = 0;
    cpu.x87.status.c1 = 0;
    cpu.x87.status.invalid |= (exceptions & 1) as u8;
    cpu.x87.status.denormal |= ((exceptions >> 1) & 1) as u8;
    cpu.x87.status.stack_fault |= ((exceptions >> 6) & 1) as u8;
    cpu.x87.status.error_summary |= ((exceptions >> 7) & 1) as u8;
    cpu.x87.status.busy |= ((exceptions >> 15) & 1) as u8;
    if pops != 0 {
        cpu.x87.tag_word |= 3 << (cpu.x87.status.top * 2);
        cpu.x87.status.top = (cpu.x87.status.top + 1) & 7;
    }
    cpu
}

fn register_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (instruction, pops) in [
        ([0xdb, 0xf2], 0), // FCOMI ST2
        ([0xdf, 0xf2], 1), // FCOMIP ST2
        ([0xdb, 0xea], 0), // FUCOMI ST2
        ([0xdf, 0xea], 1), // FUCOMIP ST2
    ] {
        for (left, right, flags) in [
            (ONE, (LEADING, 0x4000), 1),
            (ONE, ONE, 0x40),
            ((LEADING + 1, 0x3fff), ONE, 0),
            ((0, 0x8000), (0, 0), 0x40),
            ((LEADING, 1), (LEADING, 0x7ffe), 1),
        ] {
            let mut image = initial_image(&instruction);
            set_control(&mut image.cpu.x87.control, 0x0c7f);
            write_value(&mut image.cpu, 7, left);
            write_value(&mut image.cpu, 1, right);
            let opcode = (u16::from(instruction[0] & 7) << 8) | u16::from(instruction[1]);
            let result = completed(image.cpu, opcode, flags, 0, pops);
            checks.check(
                "exact ordering and wrapping pop",
                &instruction,
                &image,
                &[dispatch(result)],
            );
        }
    }
    for code in [[0xdb, 0xf0], [0xdf, 0xf0], [0xdb, 0xe8], [0xdf, 0xe8]] {
        let image = initial_image(&code);
        let opcode = (u16::from(code[0] & 7) << 8) | u16::from(code[1]);
        let result = completed(image.cpu, opcode, 0x40, 0, u8::from(code[0] == 0xdf));
        checks.check(
            "self comparison reads before popping",
            &code,
            &image,
            &[dispatch(result)],
        );
    }
}

fn exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (instruction, pops, unordered) in [
        ([0xdb, 0xf1], 0, false),
        ([0xdf, 0xf1], 1, false),
        ([0xdb, 0xe9], 0, true),
        ([0xdf, 0xe9], 1, true),
    ] {
        for (left, right, empty, exception, relation) in [
            (ONE, QNAN, 0, u16::from(!unordered), 0x45),
            (SNAN, ONE, 0, 1, 0x45),
            (ONE, (1, 0x3fff), 0, 1, 0x45),
            ((0, 0x7fff), ONE, 0, 1, 0x45),
            ((1, 0), ONE, 0, 2, 1),
            (ONE, (1, 0), 0, 2, 0),
            ((1, 0), (1, 0), 0, 2, 0x40),
            (QNAN, (1, 0), 0, u16::from(!unordered), 0x45),
            (SNAN, (1, 0), 0, 1, 0x45),
            (ONE, (1, 0), 1, 0x41, 0x45),
            (ONE, ONE, 2, 0x41, 0x45),
            (SNAN, QNAN, 3, 0x41, 0x45),
        ] {
            for control in [0x037f, 0x037e, 0x037d, 0x037c] {
                let code = [instruction.as_slice(), &[0x9b]].concat();
                let mut image = initial_image(&code);
                set_control(&mut image.cpu.x87.control, control);
                // This starting relation differs from every comparison result.
                image.cpu.flags.bytes.cf = 0;
                image.cpu.flags.bytes.pf = 1;
                image.cpu.flags.bytes.zf = 1;
                write_value(&mut image.cpu, 7, left);
                write_value(&mut image.cpu, 0, right);
                if empty & 1 != 0 {
                    image.cpu.x87.tag_word |= 0xc000;
                }
                if empty & 2 != 0 {
                    image.cpu.x87.tag_word |= 3;
                }
                let pending = exception & !control & 3 != 0;
                let opcode = (u16::from(instruction[0] & 7) << 8) | u16::from(instruction[1]);
                let result = completed(
                    image.cpu,
                    opcode,
                    relation,
                    exception | if pending { 0x8080 } else { 0 },
                    if pending { 0 } else { pops },
                );
                let mut waited = result;
                let exit = if pending {
                    Exit::FloatingPoint
                } else {
                    waited.eip += 1;
                    waited.instruction_count = waited.instruction_count.wrapping_add(1);
                    Exit::Dispatch(waited.eip)
                };
                checks.check(
                    "operand exceptions publish flags and suppress unmasked pops",
                    &code,
                    &image,
                    &[
                        dispatch(result),
                        Step {
                            cpu: waited,
                            ram: &[],
                            exit,
                        },
                    ],
                );
            }
        }
    }
}

fn live_flags(engine: Engine, frontend: Frontend) {
    let code = [
        0xdb, 0xe9, 0x0f, 0x92, 0xc3, 0x0f, 0x94, 0xc1, 0x0f, 0x9a, 0xc2,
    ];
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, flags, below, equal, unordered) in [
        ((LEADING, 0x4000), 0, 0, 0, 0),
        ((LEADING, 0xbfff), 1, 1, 0, 0),
        (ONE, 0x40, 0, 1, 0),
        (QNAN, 0x45, 1, 1, 1),
    ] {
        let mut image = initial_image(&code);
        write_value(&mut image.cpu, 7, left);
        let result = completed(image.cpu, 0x03e9, flags, 0, 0);
        let mut steps = vec![dispatch(result)];
        let mut cpu = result;
        for (register, value) in [(3, below), (1, equal), (2, unordered)] {
            let destination = match register {
                3 => &mut cpu.registers.ebx,
                1 => &mut cpu.registers.ecx,
                _ => &mut cpu.registers.edx,
            };
            *destination = (*destination & !0xff) | value;
            cpu.eip += 3;
            cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
            steps.push(dispatch(cpu));
        }
        checks.check(
            "comparison flags feed integer conditions directly",
            &code,
            &image,
            &steps,
        );
    }
}

fn stored_flags_and_pending(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for pending in [false, true] {
        let code = [0xdf, 0xf1];
        let mut image = initial_image(&code);
        // Dword SUB 0 - 1 has CF=1, PF=1, ZF=0. Concrete bytes are stale.
        image.cpu.flags.status_source.kind = 9;
        image.cpu.flags.status_source.left = 0;
        image.cpu.flags.status_source.right = 1;
        set_control(&mut image.cpu.x87.control, 0x037e);
        write_value(&mut image.cpu, 0, QNAN);
        if pending {
            image.cpu.x87.status.invalid = 1;
            image.cpu.x87.status.error_summary = 1;
            image.cpu.x87.status.busy = 1;
            checks.check(
                "entry exception leaves all flag storage untouched",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::FloatingPoint,
                }],
            );
        } else {
            let result = completed(image.cpu, 0x07f1, 0x45, 0x8081, 0);
            checks.check(
                "unmasked invalid replaces logical recipe flags",
                &code,
                &image,
                &[dispatch(result)],
            );
        }
    }
}

test_frontends!(forms, register_forms);
test_frontends!(operand_exceptions, exceptions);
test_frontends!(conditions, live_flags);
test_frontends!(previous_flags, stored_flags_and_pending);

#[test]
fn complete_encodings() {
    for code in [[0xdb, 0xf3], [0xdf, 0xf3], [0xdb, 0xeb], [0xdf, 0xeb]] {
        crate::support::encoding::check_length(&code);
    }
    crate::support::encoding::check_length(&[0x66, 0xdb, 0xe9]);
}
