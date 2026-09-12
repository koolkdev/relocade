//! Checks the bit model across pending carry chains and exact fault publication records.

use crate::support::cases::Flags;
use wasm86_x86::compile_block_from_bytes;
use wasm86_x86::FlagBytes;

use crate::support::{
    machine::{check, expected as observe_expected, Exit, Image, Step},
    step::TestModule,
};

use super::{bit_at_a_time_model, image, retire, Operation, OPERATIONS};

const MEMORY_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0, 0, 0, 0])];

fn flags_then_fault(operation: Operation, count: u8) -> (Vec<u8>, Image, Vec<Step<'static>>) {
    let rotate_modrm = 0xc4 | (operation.extension() << 3);
    let code = [
        0x00,
        0xd0, // ADD AL,DL produces zero, carry and auxiliary carry
        0xd2,
        rotate_modrm, // RCL/RCR AH,CL
        0x0f,
        0x94,
        0xc0, // SETZ AL reads the preserved ADD result
        0x0f,
        0x92,
        0xc2, // SETC DL reads the carry-ring result
        0x0f,
        0x90,
        0xc6, // SETO DH reads the resulting overflow
        0x46, // INC ESI preserves the selected carry
        0xb1,
        1, // MOV CL,1
        0x66,
        0xd3,
        0xd9, // RCR CX,CL captures one before CL becomes zero
        0x66,
        0x83,
        0xd7,
        0, // ADC DI,0 consumes RCR carry
        0x83,
        0xdd,
        0, // SBB EBP,0 consumes ADC carry
        0xd1,
        0x14,
        0x24, // RCL dword [ESP],1 consumes SBB carry
        0xc0,
        0x1b,
        32, // RCR byte [EBX],32 still requires write permission
    ];
    let mut image = image(&code, 0);
    image.cpu.registers.eax = 0x4433_80ff;
    image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
    image.cpu.registers.edx = 0xccbb_aa01;
    image.cpu.registers.ebx = 0x5000;
    image.cpu.registers.esp = 0x4000;
    image.cpu.registers.ebp = 1;
    image.cpu.registers.esi = u32::MAX;
    image.cpu.registers.edi = 0xdead_ffff;
    image.cpu.instruction_count = 0xffff_fff9;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x7fff, &[0x5a, 0, 0, 0, 0x80, 0x5a]);
    image.data(0x9fff, &[0x5a, 0x80, 0x5a]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_8000;
    cpu.flags.status_source.kind = 2;
    cpu.flags.status_source.left = 0xff;
    cpu.flags.status_source.right = 1;
    let mut steps = vec![retire(&mut cpu, 2, &[])];
    let add_flags = Flags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 0,
        of: 0,
    };
    let rotated = bit_at_a_time_model(operation, 8, 0x80, count, add_flags);
    let flags = rotated.status.unwrap_or(add_flags);
    cpu.registers.eax = 0x4433_0000 | (rotated.value << 8);
    rotated.apply_flags(&mut cpu);
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.eax |= 1;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edx = 0xccbb_aa00 | u32::from(flags.cf);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edx = 0xccbb_0000 | (u32::from(flags.of) << 8) | u32::from(flags.cf);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.esi = 0;
    cpu.flags.status_source.kind = 0;
    cpu.flags.bytes = FlagBytes {
        cf: flags.cf,
        pf: add_flags.pf,
        af: add_flags.af,
        zf: add_flags.zf,
        sf: add_flags.sf,
        of: add_flags.of,
        ..cpu.flags.bytes
    };
    steps.push(retire(&mut cpu, 1, &[]));
    cpu.registers.ecx = 0x8877_6601;
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.ecx = 0x8877_3300 | (u32::from(flags.cf) << 15);
    cpu.flags.bytes.cf = 1;
    cpu.flags.bytes.of = flags.cf;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edi = 0xdead_0000;
    cpu.flags.bytes = FlagBytes {
        cf: add_flags.cf,
        pf: add_flags.pf,
        af: add_flags.af,
        zf: add_flags.zf,
        sf: add_flags.sf,
        of: add_flags.of,
        ..cpu.flags.bytes
    };
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.registers.ebp = 0;
    cpu.flags.bytes.cf = 0;
    cpu.flags.bytes.af = 0;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.flags.bytes.cf = 1;
    cpu.flags.bytes.of = 1;
    steps.push(retire(&mut cpu, 3, MEMORY_WRITE));
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x5000,
            error: 3,
        },
    });
    (code.to_vec(), image, steps)
}

fn fault_boundary(image: &Image, steps: &[Step<'_>]) -> Step<'static> {
    let last = steps.last().unwrap();
    let mut cpu = last.cpu;
    // Intermediate ADD operands reached interpreter boundaries only. The block
    // publishes the final concrete status when the last instruction faults.
    cpu.flags.status_source.left = image.cpu.flags.status_source.left;
    cpu.flags.status_source.right = image.cpu.flags.status_source.right;
    Step {
        cpu,
        ram: MEMORY_WRITE,
        exit: last.exit,
    }
}

#[test]
fn carry_rotates_compose_with_partial_flags_conditions_and_carry_arithmetic_before_a_fault() {
    for operation in OPERATIONS {
        for count in [0, 1, 9, 10, 17, 18, 31, 32, 33] {
            let (code, image, steps) = flags_then_fault(operation, count);
            let name = format!("{operation:?} by {count} feeds conditions, INC, RCR, ADC and SBB");
            check(TestModule::interpreter(), &name, &image, &steps);
            let block =
                TestModule::new(&compile_block_from_bytes(image.cpu.eip, &code, 12).unwrap());
            check(&block, &name, &image, &[fault_boundary(&image, &steps)]);
        }
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn carry_rotates_and_pending_flags_execute_in_optimizing_v8() {
    for (operation, count) in [
        (Operation::Rcl, 9),
        (Operation::Rcl, 10),
        (Operation::Rcr, 0),
        (Operation::Rcr, 1),
    ] {
        let (code, image, steps) = flags_then_fault(operation, count);
        assert_eq!(
            TestModule::interpreter().observe_v8(&image.input(), steps.len()),
            observe_expected(&image, &steps),
            "interpreter {operation:?} by {count}"
        );
        let block = TestModule::new(&compile_block_from_bytes(image.cpu.eip, &code, 12).unwrap());
        assert_eq!(
            block.observe_v8(&image.input(), 1),
            observe_expected(&image, &[fault_boundary(&image, &steps)]),
            "snapshot {operation:?} by {count}"
        );
    }
}
