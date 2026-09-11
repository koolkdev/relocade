//! Checks exact stored flag records at interpreter and block publication boundaries.

use wasm86_x86::{compile_block_from_bytes, CpuState, StatusFlags};

use crate::support::{
    machine::{both, check, expected, Exit, Image, Step},
    sequences::test_sequences,
    step::TestModule,
};

use super::image;

fn retire(cpu: &mut CpuState, length: u32, ram: &'static [(u32, &'static [u8])]) -> Step<'static> {
    cpu.eip += length;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    Step {
        cpu: *cpu,
        ram,
        exit: Exit::Dispatch(cpu.eip),
    }
}

#[test]
fn later_arithmetic_overwrites_either_side_of_conditional_flags() {
    let code = [
        0x00, 0xd0, // ADD AL,DL
        0xd2, 0xe4, // SHL AH,CL
        0x0f, 0x90, 0xc3, // SETO BL observes ADD or SHL
        0x31, 0xd2, // XOR EDX,EDX replaces either source
        0x0f, 0x94, 0xc7, // SETZ BH observes XOR
    ];
    for count in [0, 1] {
        let mut image = image(&code);
        image.cpu.registers.eax = 0x4433_017f;
        image.cpu.registers.ecx = 0x8877_6600 | count;
        image.cpu.registers.edx = 0xccbb_aa01;
        let mut cpu = image.cpu;
        cpu.registers.eax = 0x4433_0180;
        cpu.flags.kind = 2;
        cpu.flags.left = 0x7f;
        cpu.flags.right = 1;
        let mut steps = vec![retire(&mut cpu, 2, &[])];
        if count == 1 {
            cpu.registers.eax = 0x4433_0280;
            cpu.flags.kind = 0;
            cpu.flags.status = StatusFlags {
                cf: 0,
                pf: 0,
                af: 0,
                zf: 0,
                sf: 0,
                of: 0,
            };
        }
        steps.push(retire(&mut cpu, 2, &[]));
        cpu.registers.ebx = if count == 0 { 0x10ff_ee01 } else { 0x10ff_ee00 };
        steps.push(retire(&mut cpu, 3, &[]));
        cpu.registers.edx = 0;
        cpu.flags.kind = 11;
        cpu.flags.left = 0;
        steps.push(retire(&mut cpu, 2, &[]));
        cpu.registers.ebx = if count == 0 { 0x10ff_0101 } else { 0x10ff_0100 };
        steps.push(retire(&mut cpu, 3, &[]));
        check(
            TestModule::interpreter(),
            "overwrite conditional shift flags",
            &image,
            &steps,
        );
        // The block only publishes XOR's final record. The interpreter already
        // published ADD's right operand and, for count one, SHL's status bytes.
        cpu.flags.right = image.cpu.flags.right;
        cpu.flags.status = image.cpu.flags.status;
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 5).unwrap());
        check(
            &block,
            "overwrite conditional shift flags before block publication",
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

const FLAGS_THEN_FAULT: &[u8] = &[
    0xd2, 0xe0, // SHL AL,CL: masked zero preserves incoming flags
    0x0f, 0x92, 0xc4, // SETC AH reads the incoming comparison
    0x66, 0x83, 0xd2, 0, // ADC DX,0 consumes its carry
    0xd0, 0xe4, // SHL AH,1
    0xb1, 1, // MOV CL,1
    0x66, 0xd3, 0xf9, // SAR CX,CL: old count one, new CL zero
    0xd2, 0xe4, // SHL AH,CL: zero preserves SAR's concrete flags
    0xc1, 0x2f, 1, // SHR dword [EDI],1
    0xc0, 0x7b, 0, 32, // SAR byte [EBX],32: zero still faults on read-only memory
];
const SHIFT_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0, 0, 0, 0x40])];

fn flags_then_fault() -> (Image, Vec<Step<'static>>) {
    let mut image = image(FLAGS_THEN_FAULT);
    image.cpu.registers.ecx = 0x8877_6620;
    image.cpu.registers.ebx = 0x5000;
    image.cpu.registers.edi = 0x4000;
    image.cpu.instruction_count = 0xffff_fffc;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x7fff, &[0x5a, 1, 0, 0, 0x80, 0x5a]);
    image.data(0x9fff, &[0x5a, 0x80, 0x5a]);
    let mut cpu = image.cpu;
    let mut steps = vec![retire(&mut cpu, 2, &[])];
    cpu.registers.eax = 0x4433_0111;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edx = 0xccbb_aa9a;
    cpu.flags.kind = 0;
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 1,
        af: 0,
        zf: 0,
        sf: 1,
        of: 0,
    };
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.registers.eax = 0x4433_0211;
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 0,
        af: 0,
        zf: 0,
        sf: 0,
        of: 0,
    };
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.ecx = 0x8877_6601;
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.ecx = 0x8877_3300;
    cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 0,
        zf: 0,
        sf: 0,
        of: 0,
    };
    steps.push(retire(&mut cpu, 3, &[]));
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 0,
        zf: 0,
        sf: 0,
        of: 1,
    };
    steps.push(retire(&mut cpu, 3, SHIFT_WRITE));
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x5000,
            error: 3,
        },
    });
    (image, steps)
}

#[test]
fn stored_flags_zero_counts_and_completed_memory_effects_survive_a_fault() {
    let (image, steps) = flags_then_fault();
    both(
        TestModule::interpreter(),
        "shift flags and fault publication",
        FLAGS_THEN_FAULT,
        9,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn shifts_and_conditional_flags_execute_in_optimizing_v8() {
    let (image, steps) = flags_then_fault();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        expected(&image, &steps),
        "interpreter",
    );
    let block =
        TestModule::new(&compile_block_from_bytes(image.cpu.eip, FLAGS_THEN_FAULT, 9).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: steps.last().unwrap().cpu,
                ram: SHIFT_WRITE,
                exit: steps.last().unwrap().exit
            }]
        ),
        "snapshot block",
    );
}

#[path = "sequences/cases.rs"]
mod cases;

test_sequences!(ordinary_flag_dependencies, cases::flag_dependencies());
