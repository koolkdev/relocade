use wasm86_x86::compile_block_from_bytes;

use crate::support::{
    arithmetic,
    machine::{self, both, Exit, Image, Step},
    step::TestModule,
};

#[test]
fn readonly_sources_use_the_operand_width_for_both_outcomes() {
    for (word, address, replacement) in [(true, 0x4ffe, 0x4433_1234), (false, 0x4ffc, 0x1234_88a1)]
    {
        for zero in [0, 1] {
            let mut code = if word { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, 0x44, 0x03]);
            let mut image = arithmetic::image(&code);
            image.cpu.flags.status.zf = zero;
            image.cpu.registers.eax = 0x4433_2211;
            image.cpu.registers.ebx = address;
            image.map(4, 0x8000, false);
            image.data(0x8ffb, &[0x5a, 0xa1, 0x88, 0x34, 0x12]);
            let mut cpu = image.cpu;
            if zero != 0 {
                cpu.registers.eax = replacement;
            }
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("read-only page end, word {word}, ZF={zero}"),
                &code,
                1,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}

#[test]
fn sources_cross_scattered_pages_before_testing_the_condition() {
    for (word, zero, address, eax) in [
        (true, 0, 0x4fff, 0x4433_2211),
        (false, 1, 0x4ffe, 0x1234_88a1),
    ] {
        let mut code = if word { vec![0x66] } else { vec![] };
        code.extend_from_slice(&[0x0f, 0x44, 0x03]);
        let mut image = arithmetic::image(&code);
        image.cpu.flags.status.zf = zero;
        image.cpu.registers.eax = 0x4433_2211;
        image.cpu.registers.ebx = address;
        image.map(4, 0x8000, false);
        image.map(5, 0xa000, false);
        image.data(0x8ffd, &[0x5a, 0xa1, 0x88]);
        image.data(0xa000, &[0x34, 0x12, 0x5a]);
        let mut cpu = image.cpu;
        cpu.registers.eax = eax;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "scattered source pages",
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn word_destination_keeps_high_bits_and_reads_its_old_full_address() {
    let code = [0x66, 0x0f, 0x44, 0x00]; // CMOVE AX, word [EAX]
    for (zero, eax) in [(0, 0x8000_4020), (1, 0x8000_88a1)] {
        let mut image = arithmetic::image(&code);
        image.cpu.flags.status.zf = zero;
        image.cpu.registers.eax = 0x8000_4020;
        image.map(0x80004, 0x8000, false);
        image.data(0x801f, &[0x5a, 0xa1, 0x88, 0x5a]);
        let mut cpu = image.cpu;
        cpu.registers.eax = eax;
        cpu.eip = 0x1004;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "word destination is also the address base",
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn false_conditions_do_not_suppress_source_faults() {
    for (word, address, frame, fault) in [
        (
            true,
            0x4020,
            None,
            Exit::PageFault {
                address: 0x4020,
                error: 0,
            },
        ),
        (
            true,
            0x4fff,
            Some(0x8000),
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        ),
        (
            false,
            0x4ffe,
            Some(0x8000),
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        ),
    ] {
        let mut code = if word { vec![0x66] } else { vec![] };
        code.extend_from_slice(&[0x0f, 0x45, 0x03]); // CMOVNE EAX/AX, [EBX], ZF=1
        let mut image = arithmetic::image(&code);
        image.cpu.registers.ebx = address;
        if let Some(frame) = frame {
            image.map(4, frame, false);
        }
        image.data(0x8ffe, &[0xa1, 0x88]);
        both(
            TestModule::interpreter(),
            &format!("false CMOV source at {address:04x}, word {word}"),
            &code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: fault,
            }],
        );
    }
}

const KNOWN_FALSE_READ: &[u8] = &[0x31, 0xc0, 0x0f, 0x45, 0x0b]; // XOR EAX,EAX; CMOVNE ECX,[EBX]

fn known_false_source_fault() -> (Image, [Step<'static>; 2]) {
    let mut image = arithmetic::image(KNOWN_FALSE_READ);
    image.cpu.registers.ebx = 0x4000;
    let mut cpu = image.cpu;
    cpu.registers.eax = 0;
    cpu.flags.kind = 11;
    cpu.flags.left = 0;
    cpu.eip = 0x1002;
    cpu.instruction_count = 0;
    (
        image,
        [
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x4000,
                    error: 0,
                },
            },
        ],
    )
}

#[test]
fn known_false_condition_still_checks_source_access() {
    let (image, steps) = known_false_source_fault();
    both(
        TestModule::interpreter(),
        "false memory CMOV still reports an absent source page after XOR",
        KNOWN_FALSE_READ,
        2,
        &image,
        &steps,
    );
}

const CONDITIONAL_READ_THEN_FAULT: &[u8] = &[
    0x39, 0xd8, // CMP EAX, EBX, signed greater but unsigned below
    0x66, 0x0f, 0x4f, 0xd1, // CMOVG DX, CX, true
    0x0f, 0x4c, 0xc1, // CMOVL EAX, ECX, false
    0x0f, 0x42, 0x36, // CMOVB ESI, dword [ESI], true, old destination is the base
    0x66, 0x0f, 0x4c, 0x13, // CMOVL DX, word [EBX], false but unmapped
];

fn conditional_read_then_fault() -> (Image, Vec<Step<'static>>) {
    let mut image = arithmetic::image(CONDITIONAL_READ_THEN_FAULT);
    image.cpu.registers.eax = 0x7fff_fffe;
    image.cpu.registers.ebx = 0xffff_fffe;
    image.cpu.registers.ecx = 0x8877_6655;
    image.cpu.registers.edx = 0xccbb_aa99;
    image.cpu.registers.esi = 0x8000_4020;
    image.map(0x80004, 0x8000, false);
    image.data(0x801f, &[0x5a, 0x78, 0x56, 0x34, 0x12, 0x5a]);
    let mut cpu = image.cpu;
    cpu.flags.kind = 9;
    cpu.flags.left = 0x7fff_fffe;
    cpu.flags.right = 0xffff_fffe;
    let mut steps = Vec::new();
    for (count, (next, edx, esi)) in [
        (0x1002, 0xccbb_aa99, 0x8000_4020),
        (0x1006, 0xccbb_6655, 0x8000_4020),
        (0x1009, 0xccbb_6655, 0x8000_4020),
        (0x100c, 0xccbb_6655, 0x1234_5678),
    ]
    .into_iter()
    .enumerate()
    {
        cpu.registers.edx = edx;
        cpu.registers.esi = esi;
        cpu.eip = next;
        cpu.instruction_count = count as u32;
        steps.push(Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(next),
        });
    }
    steps.push(Step {
        cpu,
        ram: &[],
        exit: Exit::PageFault {
            address: 0xffff_fffe,
            error: 0,
        },
    });
    (image, steps)
}

#[test]
fn local_conditions_preserve_flags_and_publish_completed_progress_before_faulting() {
    let (image, steps) = conditional_read_then_fault();
    both(
        TestModule::interpreter(),
        "local CMOV conditions before a fault",
        CONDITIONAL_READ_THEN_FAULT,
        5,
        &image,
        &steps,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn conditional_moves_and_false_source_faults_execute_in_optimizing_v8() {
    let (image, steps) = conditional_read_then_fault();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), steps.len()),
        machine::expected(&image, &steps),
        "interpreter",
    );
    let block =
        TestModule::new(&compile_block_from_bytes(0x1000, CONDITIONAL_READ_THEN_FAULT, 5).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: steps.last().unwrap().cpu,
                ram: &[],
                exit: steps.last().unwrap().exit
            }]
        ),
        "snapshot block",
    );
    let (image, steps) = known_false_source_fault();
    assert_eq!(
        TestModule::interpreter().observe_v8(&image.input(), 2),
        machine::expected(&image, &steps),
        "interpreter checks a known-false source",
    );
    let block = TestModule::new(&compile_block_from_bytes(0x1000, KNOWN_FALSE_READ, 2).unwrap());
    assert_eq!(
        block.observe_v8(&image.input(), 1),
        machine::expected(
            &image,
            &[Step {
                cpu: steps[1].cpu,
                ram: &[],
                exit: steps[1].exit,
            }]
        ),
        "snapshot block checks a known-false source",
    );
}
