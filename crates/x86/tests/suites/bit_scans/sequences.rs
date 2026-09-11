use wasm86_x86::{compile_block_from_bytes, Gpr32};

use crate::support::{
    machine::{check, expected as observe_expected, Exit, Image, Step},
    step::TestModule,
};

use super::{expected, image, retire, Operation};

const SCANS_THEN_FAULT: &[u8] = &[
    0xb8, 0x5b, 0xa5, 0x33, 0x44, // MOV EAX,0x4433a55b
    0xb4, 0x7c, // MOV AH,0x7c leaves a pending partial destination
    0x80, 0xc3, 1, // ADD BL,1 supplies a pending lazy flag source
    0x66, 0x0f, 0xbc, 0xc2, // BSF AX,DX
    0x0f, 0x9a, 0xc4, // SETP AH consumes parity of the full word source
    0x0f, 0x94, 0xc1, // SETZ CL observes whether the word source was zero
    0x0f, 0x95, 0xc5, // SETNZ CH observes the same scan
    0x0f, 0xba, 0xed, 0, // BTS EBP,0 supplies a partial carry change
    0x0f, 0xbd, 0xfa, // BSR EDI,EDX replaces that carry and the other flags
    0x0f, 0x9a, 0xc7, // SETP BH consumes parity of the full dword source
    0x0f, 0x94, 0xc3, // SETZ BL observes the dword source
    0xba, 0, 0, 0, 0, // MOV EDX,0 supplies a known-zero source
    0x0f, 0xbc, 0xc2, // BSF EAX,EDX preserves the current full destination
    0x0f, 0x94, 0xc6, // SETZ DH changes the known-zero source to 0x100
    0x66, 0x0f, 0xbd, 0xd2, // BSR DX,DX captures its own old source
    0x0f, 0x95, 0xc0, // SETNZ AL changes a byte of the preserved destination
    0x66, 0x0f, 0xbc, 0x34, 0x24, // BSF SI,word [ESP] reads a read-only zero
    0x0f, 0xbd, 0x6d, 0, // BSR EBP,dword [EBP] faults before any effects
];

fn scans_then_fault(source: u32) -> (Image, Vec<Step<'static>>) {
    let mut image = image(SCANS_THEN_FAULT);
    image.cpu.registers.ebx = 0xccbb_00ff;
    image.cpu.registers.ecx = 0x8877_6655;
    image.cpu.registers.edx = source;
    image.cpu.registers.edi = 0x1122_a55b;
    image.cpu.registers.esi = 0x5566_a55b;
    image.cpu.registers.esp = 0x4000;
    image.cpu.registers.ebp = 0x5001;
    image.cpu.instruction_count = 0xffff_fff5;
    image.map(4, 0x8000, false);
    image.data(0x7fff, &[0x5a, 0, 0, 0x5a]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_a55b;
    let mut steps = vec![retire(&mut cpu, 5)];
    cpu.registers.eax = 0x4433_7c5b;
    steps.push(retire(&mut cpu, 2));
    cpu.registers.ebx = 0xccbb_0000;
    cpu.flags.kind = 2;
    cpu.flags.left = 0xff;
    cpu.flags.right = 1;
    steps.push(retire(&mut cpu, 3));
    expected(Operation::Bsf, 16, source, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
    steps.push(retire(&mut cpu, 4));
    cpu.registers.eax = (cpu.registers.eax & 0xffff_00ff) | (u32::from(cpu.flags.status.pf) << 8);
    steps.push(retire(&mut cpu, 3));
    let word_zero = u32::from(cpu.flags.status.zf);
    cpu.registers.ecx = 0x8877_6600 | word_zero;
    steps.push(retire(&mut cpu, 3));
    cpu.registers.ecx = 0x8877_0000 | ((1 - word_zero) << 8) | word_zero;
    steps.push(retire(&mut cpu, 3));
    cpu.flags.status.cf = 1;
    steps.push(retire(&mut cpu, 4));
    expected(Operation::Bsr, 32, source, cpu.registers.edi).apply(&mut cpu, Gpr32::Edi);
    steps.push(retire(&mut cpu, 3));
    cpu.registers.ebx = 0xccbb_0000 | (u32::from(cpu.flags.status.pf) << 8);
    steps.push(retire(&mut cpu, 3));
    cpu.registers.ebx = (cpu.registers.ebx & 0xffff_ff00) | u32::from(cpu.flags.status.zf);
    steps.push(retire(&mut cpu, 3));
    cpu.registers.edx = 0;
    steps.push(retire(&mut cpu, 5));
    expected(Operation::Bsf, 32, 0, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
    steps.push(retire(&mut cpu, 3));
    cpu.registers.edx = 0x100;
    steps.push(retire(&mut cpu, 3));
    expected(Operation::Bsr, 16, 0x100, cpu.registers.edx).apply(&mut cpu, Gpr32::Edx);
    steps.push(retire(&mut cpu, 4));
    cpu.registers.eax = (cpu.registers.eax & 0xffff_ff00) | 1;
    steps.push(retire(&mut cpu, 3));
    expected(Operation::Bsf, 16, 0, cpu.registers.esi).apply(&mut cpu, Gpr32::Esi);
    steps.push(retire(&mut cpu, 5));
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0x5001,
            error: 0,
        },
    });
    (image, steps)
}

fn fault_boundary(image: &Image, steps: &[Step<'_>]) -> Step<'static> {
    let last = steps.last().unwrap();
    let mut cpu = last.cpu;
    // Only the interpreter published the intermediate ADD operands. A block
    // publishes the final concrete flags while retaining its old unused payload.
    cpu.flags.left = image.cpu.flags.left;
    cpu.flags.right = image.cpu.flags.right;
    Step {
        cpu,
        ram: &[],
        exit: last.exit,
    }
}

#[test]
fn scans_preserve_pending_destinations_replace_flags_and_feed_conditions_before_a_fault() {
    for source in [
        0,
        1,
        0x100,
        0x8000,
        0xffff_0000,
        0x8000_0000,
        0x8008,
        0x8000_0001,
    ] {
        let (image, steps) = scans_then_fault(source);
        let name = format!("scan source {source:08x}, aliases and pending flags before a fault");
        check(TestModule::interpreter(), &name, &image, &steps);
        let block = TestModule::new(
            &compile_block_from_bytes(image.cpu.eip, SCANS_THEN_FAULT, steps.len() as u32).unwrap(),
        );
        check(&block, &name, &image, &[fault_boundary(&image, &steps)]);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn bit_scans_and_zero_destination_preservation_execute_in_optimizing_v8() {
    for source in [0, 0x100, 0x8000, 0x8000_0000, 0x8008, 0x8000_0001] {
        let (image, steps) = scans_then_fault(source);
        assert_eq!(
            TestModule::interpreter().observe_v8(&image.input(), steps.len()),
            observe_expected(&image, &steps),
            "interpreter scan source {source:08x}"
        );
        let block = TestModule::new(
            &compile_block_from_bytes(image.cpu.eip, SCANS_THEN_FAULT, steps.len() as u32).unwrap(),
        );
        assert_eq!(
            block.observe_v8(&image.input(), 1),
            observe_expected(&image, &[fault_boundary(&image, &steps)]),
            "snapshot scan source {source:08x}"
        );
    }
}
