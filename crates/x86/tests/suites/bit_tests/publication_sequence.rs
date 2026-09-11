//! Exact flag-record publication across interpreter checkpoints and a faulting block.
//! Architectural results and flag guarantees are covered by `sequences`.

use wasm86_x86::{compile_block_from_bytes, CpuState, StatusFlags};

use crate::support::{
    machine::{check, expected as observe_expected, Exit, Image, Step},
    step::TestModule,
};

use super::image;

const MEMORY_WRITE: &[(u32, &[u8])] = &[(0x8000, &[1, 0, 0, 0])];

fn flags_then_fault(index: u32) -> (Vec<u8>, Image, Vec<Step<'static>>) {
    let code = [
        0x00, 0xd0, // ADD AL,DL produces zero, carry and auxiliary carry
        0x0f, 0xa3, 0xcd, // BT EBP,ECX replaces only carry
        0x0f, 0x94, 0xc0, // SETZ AL still reads the ADD result
        0x0f, 0x92, 0xc2, // SETC DL reads the old tested bit
        0x46, // INC ESI retains carry
        0x66, 0x83, 0xd7, 0, // ADC DI,0 consumes that carry
        0x66, 0x0f, 0xab, 0xd0, // BTS AX,DX uses the just-written low index
        0x0f, 0xba, 0x34, 0x24, 31, // BTR dword [ESP],31
        0x66, 0x0f, 0xbb, 0xc9, // BTC CX,CX captures its own old index
        0x0f, 0x92, 0xc6, // SETC DH records the complemented bit's old value
        0x83, 0xd5, 0, // ADC EBP,0 consumes that old bit
        0x66, 0x0f, 0xa3, 0x3b, // BT word [EBX],DI reads a signed indexed, read-only unit
        0x66, 0x0f, 0xba, 0x2b, 255, // BTS word [EBX],255 requires write permission
    ];
    let mut image = image(&code);
    image.cpu.registers.eax = 0x4433_80ff;
    image.cpu.registers.ecx = 0x8877_0000 | index;
    image.cpu.registers.edx = 0xccbb_aa01;
    image.cpu.registers.ebx = 0x5002;
    image.cpu.registers.esp = 0x4000;
    image.cpu.registers.ebp = 0x8000_0001;
    image.cpu.registers.esi = u32::MAX;
    image.cpu.registers.edi = 0xdead_ffff;
    image.cpu.instruction_count = 0xffff_fff9;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x7fff, &[0x5a, 1, 0, 0, 0x80, 0x5a]);
    image.data(0x9fff, &[0x5a, 0, 0x80, 1, 0, 0x5a]);
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
    let (carry, complemented_ecx, complemented_carry) = match index {
        0 => (1, 0x8877_0001, 0),
        1 => (0, 0x8877_0003, 0),
        15 => (0, 0x8877_800f, 0),
        16 => (0, 0x8877_0011, 0),
        31 => (1, 0x8877_801f, 0),
        32 => (1, 0x8877_0021, 0),
        33 => (0, 0x8877_0023, 0),
        0xffff => (1, 0x8877_7fff, 1),
        u32::MAX => (1, 0xffff_7fff, 1),
        _ => panic!("missing literal bit-test sequence"),
    };
    cpu.flags.kind = 0;
    cpu.flags.status = StatusFlags {
        cf: carry,
        ..add_flags
    };
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.eax = 0x4433_8001;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.edx = 0xccbb_aa00 | u32::from(carry);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.esi = 0;
    steps.push(retire(&mut cpu, 1, &[]));
    cpu.registers.edi = if carry == 1 { 0xdead_0000 } else { 0xdead_ffff };
    cpu.flags.status = StatusFlags {
        cf: carry,
        pf: 1,
        af: carry,
        zf: carry,
        sf: 1 - carry,
        of: 0,
    };
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.registers.eax = 0x4433_8001 | (u32::from(carry) << 1);
    cpu.flags.status.cf = 1 - carry;
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.flags.status.cf = 1;
    steps.push(retire(&mut cpu, 5, MEMORY_WRITE));
    cpu.registers.ecx = complemented_ecx;
    cpu.flags.status.cf = complemented_carry;
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.registers.edx = 0xccbb_0000 | (u32::from(complemented_carry) << 8) | u32::from(carry);
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.ebp = 0x8000_0001 + u32::from(complemented_carry);
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 0,
        af: 0,
        zf: 0,
        sf: 1,
        of: 0,
    };
    steps.push(retire(&mut cpu, 3, &[]));
    // DI is either -1 or zero. Both selected words contain a set tested bit.
    cpu.flags.status.cf = 1;
    steps.push(retire(&mut cpu, 4, &[]));
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x5002,
            error: 3,
        },
    });
    (code.to_vec(), image, steps)
}

fn fault_boundary(image: &Image, steps: &[Step<'_>]) -> Step<'static> {
    let last = steps.last().unwrap();
    let mut cpu = last.cpu;
    // Intermediate ADD operands reached interpreter boundaries only. The block
    // publishes concrete flags and leaves the previous lazy payload untouched.
    cpu.flags.left = image.cpu.flags.left;
    cpu.flags.right = image.cpu.flags.right;
    Step {
        cpu,
        ram: MEMORY_WRITE,
        exit: last.exit,
    }
}

#[test]
fn interpreter_and_block_publish_exact_flag_records_before_a_bit_write_fault() {
    for index in [0, 1, 15, 16, 31, 32, 33, 0xffff, u32::MAX] {
        let (code, image, steps) = flags_then_fault(index);
        let name = format!("bit index {index:x} feeds conditions, partial flags and ADC");
        check(TestModule::interpreter(), &name, &image, &steps);
        let block = TestModule::new(&compile_block_from_bytes(image.cpu.eip, &code, 13).unwrap());
        check(&block, &name, &image, &[fault_boundary(&image, &steps)]);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn exact_flag_record_publication_executes_in_optimizing_v8() {
    for index in [0, 1, 31, u32::MAX] {
        let (code, image, steps) = flags_then_fault(index);
        assert_eq!(
            TestModule::interpreter().observe_v8(&image.input(), steps.len()),
            observe_expected(&image, &steps),
            "interpreter bit index {index:x}"
        );
        let block = TestModule::new(&compile_block_from_bytes(image.cpu.eip, &code, 13).unwrap());
        assert_eq!(
            block.observe_v8(&image.input(), 1),
            observe_expected(&image, &[fault_boundary(&image, &steps)]),
            "snapshot bit index {index:x}"
        );
    }
}

fn retire(cpu: &mut CpuState, length: u32, ram: &'static [(u32, &'static [u8])]) -> Step<'static> {
    cpu.eip += length;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    Step {
        cpu: *cpu,
        ram,
        exit: Exit::Dispatch(cpu.eip),
    }
}
