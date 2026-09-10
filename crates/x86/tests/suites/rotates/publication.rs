use wasm86_x86::{compile_block_from_bytes, StatusFlags};

use crate::support::{
    machine::{check, expected as observe_expected, Exit, Image, Step},
    step::TestModule,
};

use super::{image, retire};

const FLAGS_THEN_FAULT: &[u8] = &[
    0x00, 0xd0, // ADD AL,DL
    0xd2, 0xc4, // ROL AH,CL: full byte turn updates CF/OF
    0x0f, 0x94, 0xc6, // SETZ DH reads the preserved ADD flag
    0x46, // INC ESI preserves rotate carry
    0xb1, 1, // MOV CL,1
    0x66, 0xd3, 0xc9, // ROR CX,CL: captures one before CL becomes zero
    0xd2, 0xc4, // ROL AH,CL: zero preserves the preceding partial effect
    0x66, 0x83, 0xd7, 0, // ADC DI,0 consumes its carry
    0xd1, 0x4d, 0, // ROR dword [EBP],1
    0xc0, 0x03, 32, // ROL byte [EBX],32 still requires write permission
];
const ROTATE_WRITE: &[(u32, &[u8])] = &[(0x8000, &[0, 0, 0, 0xc0])];

fn flags_then_fault() -> (Image, Vec<Step<'static>>) {
    let mut image = image(FLAGS_THEN_FAULT);
    image.cpu.registers.eax = 0x4433_80ff;
    image.cpu.registers.ecx = 0x8877_6608;
    image.cpu.registers.edx = 0xccbb_aa01;
    image.cpu.registers.ebx = 0x5000;
    image.cpu.registers.ebp = 0x4000;
    image.cpu.registers.esi = 0xffff_ffff;
    image.cpu.registers.edi = 0xdead_0000;
    image.cpu.instruction_count = 0xffff_fffc;
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x7fff, &[0x5a, 1, 0, 0, 0x80, 0x5a]);
    image.data(0x9fff, &[0x5a, 0x80, 0x5a]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_8000;
    cpu.flags.kind = 2;
    cpu.flags.left = 0xff;
    cpu.flags.right = 1;
    let mut steps = vec![retire(&mut cpu, 2, &[])];
    cpu.flags.kind = 0;
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 0,
        of: 0,
    };
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.edx = 0xccbb_0101;
    steps.push(retire(&mut cpu, 3, &[]));
    cpu.registers.esi = 0;
    steps.push(retire(&mut cpu, 1, &[]));
    cpu.registers.ecx = 0x8877_6601;
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.ecx = 0x8877_b300;
    cpu.flags.status.cf = 1;
    cpu.flags.status.of = 1;
    steps.push(retire(&mut cpu, 3, &[]));
    steps.push(retire(&mut cpu, 2, &[]));
    cpu.registers.edi = 0xdead_0001;
    cpu.flags.status = StatusFlags {
        cf: 0,
        pf: 0,
        af: 0,
        zf: 0,
        sf: 0,
        of: 0,
    };
    steps.push(retire(&mut cpu, 4, &[]));
    cpu.flags.status.cf = 1;
    steps.push(retire(&mut cpu, 3, ROTATE_WRITE));
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

fn fault_boundary(image: &Image, steps: &[Step<'_>]) -> Step<'static> {
    let last = steps.last().unwrap();
    let mut cpu = last.cpu;
    // Intermediate ADD operands reached interpreter boundaries only. The block
    // publishes the final concrete status while handling the write fault.
    cpu.flags.left = image.cpu.flags.left;
    cpu.flags.right = image.cpu.flags.right;
    Step {
        cpu,
        ram: ROTATE_WRITE,
        exit: last.exit,
    }
}

#[test]
fn completed_rotate_effects_and_preserved_flags_are_published_before_a_fault() {
    let (image, steps) = flags_then_fault();
    check(
        TestModule::interpreter(),
        "rotates and partial flags before write fault",
        &image,
        &steps,
    );
    let block =
        TestModule::new(&compile_block_from_bytes(image.cpu.eip, FLAGS_THEN_FAULT, 10).unwrap());
    check(
        &block,
        "snapshot rotate effects before write fault",
        &image,
        &[fault_boundary(&image, &steps)],
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn rotates_and_partial_flags_execute_in_optimizing_v8() {
    let (image, steps) = flags_then_fault();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        observe_expected(&image, &steps),
        "interpreter"
    );
    let block =
        TestModule::new(&compile_block_from_bytes(image.cpu.eip, FLAGS_THEN_FAULT, 10).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        observe_expected(&image, &[fault_boundary(&image, &steps)]),
        "snapshot block"
    );
}
