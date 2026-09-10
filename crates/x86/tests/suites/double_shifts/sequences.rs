use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    machine::{check, expected as observe_expected, Exit, Image, Step},
    step::TestModule,
};

use super::{expected, image, retire, Operation, OPERATIONS};

const MEMORY_WRITE: &[(u32, &[u8])] = &[(0x8000, &[1, 0, 0, 0])];

fn flags_then_fault(operation: Operation, count: u8) -> (Vec<u8>, Image, Vec<Step<'static>>) {
    let shift_opcode = operation.opcode(true);
    let code = [
        0x00,
        0xd0, // ADD AL,DL produces zero, carry and auxiliary carry
        0x66,
        0x0f,
        shift_opcode,
        0xd0, // SHLD/SHRD AX,DX,CL
        0x0f,
        0x94,
        0xc0, // SETZ AL reads the chosen arithmetic result
        0x0f,
        0x92,
        0xc2, // SETC DL reads the shifted-out destination bit
        0x0f,
        0x90,
        0xc6, // SETO DH reads the masked-count rule
        0x46, // INC ESI preserves the chosen carry
        0x66,
        0x83,
        0xd7,
        0, // ADC DI,0 consumes that carry
        0x83,
        0xdd,
        0, // SBB EBP,0 consumes ADC carry
        0x66,
        0x0f,
        0xac,
        0xd1,
        16, // SHRD CX,DX,16 replaces all status flags
        0x0f,
        0xa4,
        0x3c,
        0x24,
        1, // SHLD dword [ESP],EDI,1
        0x0f,
        0xac,
        0x3b,
        32, // SHRD dword [EBX],EDI,32 still needs write permission
    ];
    let mut image = image(&code);
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
    image.data(0x9fff, &[0x5a, 0, 0, 0, 0x80, 0x5a]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_8000;
    cpu.flags.kind = 2;
    cpu.flags.left = 0xff;
    cpu.flags.right = 1;
    let mut steps = vec![retire(&mut cpu, 2, &[])];
    let add_flags = StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 0,
        of: 0,
    };
    let shifted = expected(operation, 16, 0x8000, 0xaa01, count);
    let flags = shifted.status.unwrap_or(add_flags);
    cpu.registers.eax = 0x4433_0000 | shifted.value;
    shifted.apply_flags(&mut cpu);
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.registers.eax = (cpu.registers.eax & !0xff) | u32::from(flags.zf);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edx = 0xccbb_aa00 | u32::from(flags.cf);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edx = 0xccbb_0000 | (u32::from(flags.of) << 8) | u32::from(flags.cf);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.esi = 0;
    cpu.flags.kind = 0;
    cpu.flags.status = StatusFlags {
        cf: flags.cf,
        ..add_flags
    };
    steps.push(retire(&mut cpu, 1, &[]));
    cpu.registers.edi = if flags.cf == 1 {
        0xdead_0000
    } else {
        0xdead_ffff
    };
    cpu.flags.status = StatusFlags {
        cf: flags.cf,
        pf: 1,
        af: flags.cf,
        zf: flags.cf,
        sf: 1 - flags.cf,
        of: 0,
    };
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.registers.ebp = 1 - u32::from(flags.cf);
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: flags.cf,
        af: 0,
        zf: flags.cf,
        sf: 0,
        of: 0,
    };
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.ecx = 0x8877_0000 | (cpu.registers.edx & 0xffff);
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 1 - flags.cf,
        af: 0,
        zf: u8::from(flags.cf == 0 && flags.of == 0),
        sf: 0,
        of: 0,
    };
    steps.push(retire(&mut cpu, 5, &[]));
    cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 0,
        af: 0,
        zf: 0,
        sf: 0,
        of: 1,
    };
    steps.push(retire(&mut cpu, 5, MEMORY_WRITE));
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
    cpu.flags.left = image.cpu.flags.left;
    cpu.flags.right = image.cpu.flags.right;
    Step {
        cpu,
        ram: MEMORY_WRITE,
        exit: last.exit,
    }
}

#[test]
fn double_shifts_compose_with_pending_flags_conditions_and_carry_arithmetic_before_a_fault() {
    for operation in OPERATIONS {
        for count in [0, 1, 2, 15, 16, 17, 31, 32, 33, 48] {
            let (code, image, steps) = flags_then_fault(operation, count);
            let name = format!("{operation:?} by {count} feeds conditions, INC, ADC and SBB");
            check(TestModule::interpreter(), &name, &image, &steps);
            let block =
                TestModule::new(&compile_block_from_bytes(image.cpu.eip, &code, 11).unwrap());
            check(&block, &name, &image, &[fault_boundary(&image, &steps)]);
        }
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn double_shifts_and_pending_flags_execute_in_optimizing_v8() {
    for (operation, count) in [
        (Operation::Shld, 0),
        (Operation::Shld, 16),
        (Operation::Shrd, 1),
        (Operation::Shrd, 17),
    ] {
        let (code, image, steps) = flags_then_fault(operation, count);
        assert_eq!(
            TestModule::interpreter().observe_v8(&image.input(), steps.len()),
            observe_expected(&image, &steps),
            "interpreter {operation:?} by {count}"
        );
        let block = TestModule::new(&compile_block_from_bytes(image.cpu.eip, &code, 11).unwrap());
        assert_eq!(
            block.observe_v8(&image.input(), 1),
            observe_expected(&image, &[fault_boundary(&image, &steps)]),
            "snapshot {operation:?} by {count}"
        );
    }
}
